//! The async [`Client`] and its [`ClientBuilder`].

use std::sync::Arc;
use std::time::Duration;

use crate::archive::{ArchiveEntry, ArchiveFile, ArchivePage, ArchiveQuery};
use crate::auth::TokenProvider;
use crate::catalog::{parse_catalog, parse_product, Product};
use crate::config::{Config, Credentials, RetryPolicy};
use crate::error::{Error, Result};
use crate::fuel_mix::FuelMix;
use crate::http::{Http, RawResponse};
use crate::page::{PageMeta, ReportPage};
use crate::request::{Query, Report, ReportRequest};

/// Header carrying the API Management subscription key.
const SUBSCRIPTION_KEY_HEADER: &str = "Ocp-Apim-Subscription-Key";

/// Rows per page used by the `*_all` helpers when the query sets no size.
const BULK_PAGE_SIZE: u32 = 50_000;

/// One typed page: paging block plus converted rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// Paging block of the response.
    pub meta: PageMeta,
    /// Rows converted to `T`.
    pub records: Vec<T>,
}

/// Builder for [`Client`]. Every setter is optional; defaults target production ERCOT.
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    credentials: Credentials,
    config: Config,
}

impl ClientBuilder {
    /// Overrides the report-service base URL (for a proxy or a test server).
    #[must_use]
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.config.base_url = url.into();
        self
    }

    /// Overrides the Azure B2C token endpoint.
    #[must_use]
    pub fn token_url(mut self, url: impl Into<String>) -> Self {
        self.config.token_url = url.into();
        self
    }

    /// Overrides the dashboard fuel-mix feed URL.
    #[must_use]
    pub fn fuel_mix_url(mut self, url: impl Into<String>) -> Self {
        self.config.fuel_mix_url = url.into();
        self
    }

    /// Overrides the OAuth client id.
    #[must_use]
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.config.client_id = id.into();
        self
    }

    /// Overrides the User-Agent. Keep it browser-like: bare agents get bot-checked.
    #[must_use]
    pub fn user_agent(mut self, agent: impl Into<String>) -> Self {
        self.config.user_agent = agent.into();
        self
    }

    /// Sets the per-request timeout.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Sets the retry policy.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.config.retry = retry;
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when the HTTP stack cannot be initialised.
    pub fn build(self) -> Result<Client> {
        let http = Http::new(&self.config)?;
        let tokens = TokenProvider::new(self.credentials, &self.config);
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                tokens,
                config: self.config,
            }),
        })
    }
}

#[derive(Debug)]
struct Inner {
    http: Http,
    tokens: TokenProvider,
    config: Config,
}

/// Async client for the ERCOT Public API.
///
/// Cheap to clone; clones share one connection pool, cookie jar and token cache.
/// Tokens are fetched on first use, cached, refreshed before they expire and
/// re-acquired after an HTTP 401.
///
/// ```no_run
/// use gridapi_ercot::{Client, Credentials, Query, RealTimeSpp};
///
/// # async fn run() -> Result<(), gridapi_ercot::Error> {
/// let client = Client::new(Credentials::from_env()?)?;
/// let latest = client
///     .fetch(&Query::<RealTimeSpp>::new().settlement_point("HB_NORTH").size(4))
///     .await?;
/// for r in &latest.records {
///     println!("{} h{} i{} {:.2} $/MWh", r.delivery_date, r.delivery_hour, r.delivery_interval, r.price);
/// }
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<Client>();
};

impl Client {
    /// A client with default configuration.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when the HTTP stack cannot be initialised.
    pub fn new(credentials: Credentials) -> Result<Self> {
        Self::builder(credentials).build()
    }

    /// Starts a [`ClientBuilder`].
    #[must_use]
    pub fn builder(credentials: Credentials) -> ClientBuilder {
        ClientBuilder {
            credentials,
            config: Config::default(),
        }
    }

    fn url(&self, path: &str) -> String {
        let base = self.inner.config.base_url.trim_end_matches('/');
        if path.is_empty() {
            base.to_owned()
        } else {
            format!("{base}/{}", path.trim_start_matches('/'))
        }
    }

    /// Sends an authenticated request, re-logging in once if the token is rejected.
    async fn authed<F>(&self, build: F) -> Result<RawResponse>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        let mut relogged = false;
        loop {
            let bearer = self.inner.tokens.bearer(&self.inner.http).await?;
            let key = self.inner.tokens.subscription_key();
            let result = self
                .inner
                .http
                .execute(|c| {
                    build(c)
                        .bearer_auth(&bearer)
                        .header(SUBSCRIPTION_KEY_HEADER, key)
                })
                .await;
            match result {
                Err(Error::Api { status: 401, .. }) if !relogged => {
                    relogged = true;
                    self.inner.tokens.invalidate().await;
                }
                other => return other,
            }
        }
    }

    async fn get(&self, path: &str, params: &[(String, String)]) -> Result<RawResponse> {
        let url = self.url(path);
        self.authed(|c| c.get(&url).query(params)).await
    }

    // ------------------------------------------------------------------ catalog

    /// Lists every report product (EMIL id) the API publishes.
    ///
    /// # Errors
    ///
    /// Authentication, transport or API errors; [`Error::Decode`] on an unexpected body.
    pub async fn products(&self) -> Result<Vec<Product>> {
        parse_catalog(&self.get("", &[]).await?.body)
    }

    /// Fetches one product by EMIL id (case-insensitive), including its data endpoints.
    ///
    /// # Errors
    ///
    /// An [`Error::Api`] with status 404 for an unknown id, plus the usual failures.
    pub async fn product(&self, emil_id: &str) -> Result<Product> {
        let raw = self.get(&emil_id.to_ascii_lowercase(), &[]).await?;
        parse_product(&raw.body)
    }

    /// Lists products whose id, name or description contains `term` (case-insensitive).
    ///
    /// # Errors
    ///
    /// Same as [`Client::products`].
    pub async fn search_products(&self, term: &str) -> Result<Vec<Product>> {
        Ok(self
            .products()
            .await?
            .into_iter()
            .filter(|p| p.matches(term))
            .collect())
    }

    // ------------------------------------------------------------------ reports

    /// Runs a generic report query and returns one page of untyped rows.
    ///
    /// # Errors
    ///
    /// [`Error::Api`] with `unknown_params` filled when a filter is not valid for the
    /// report; the usual transport and decode errors otherwise.
    pub async fn query(&self, request: &ReportRequest) -> Result<ReportPage> {
        let raw = self.get(&request.path(), &request.params()).await?;
        ReportPage::from_json(&raw.body)
    }

    /// Fetches every page of a generic query and concatenates the rows.
    ///
    /// Uses a page size of 50 000 unless the request sets one. The returned page's
    /// `meta` describes the merged result (`total_pages` is 1).
    ///
    /// # Errors
    ///
    /// [`Error::TooManyPages`] (before fetching further) when the query spans more than
    /// `max_pages` pages; otherwise as [`Client::query`].
    pub async fn query_all(&self, request: &ReportRequest, max_pages: u32) -> Result<ReportPage> {
        let request = if request.page_size().is_some() {
            request.clone()
        } else {
            request.clone().size(BULK_PAGE_SIZE)
        };
        let mut merged = self.query(&request.clone().page(1)).await?;
        if merged.meta.total_pages > max_pages {
            return Err(Error::TooManyPages {
                limit: max_pages,
                total_pages: merged.meta.total_pages,
            });
        }
        for page_no in 2..=merged.meta.total_pages {
            let next = self.query(&request.clone().page(page_no)).await?;
            merged.data.extend(next.data);
        }
        merged.meta.current_page = u32::from(!merged.data.is_empty());
        merged.meta.total_pages = merged.meta.current_page;
        Ok(merged)
    }

    /// Runs a typed query and converts one page of rows.
    ///
    /// # Errors
    ///
    /// As [`Client::query`], plus [`Error::Row`] if a row does not match the report's record type.
    pub async fn fetch<R: Report>(&self, query: &Query<R>) -> Result<Page<R::Record>> {
        let page = self.query(query.request()).await?;
        Ok(Page {
            records: page.records()?,
            meta: page.meta,
        })
    }

    /// Fetches and converts every page of a typed query.
    ///
    /// # Errors
    ///
    /// As [`Client::query_all`], plus [`Error::Row`] on a malformed row.
    pub async fn fetch_all<R: Report>(
        &self,
        query: &Query<R>,
        max_pages: u32,
    ) -> Result<Vec<R::Record>> {
        self.query_all(query.request(), max_pages).await?.records()
    }

    // ------------------------------------------------------------------ archive

    /// Lists archived documents for a product.
    ///
    /// # Errors
    ///
    /// The usual authentication, transport, API and decode errors.
    pub async fn archives(&self, emil_id: &str, query: &ArchiveQuery) -> Result<ArchivePage> {
        let path = format!("archive/{}", emil_id.to_ascii_lowercase());
        let raw = self.get(&path, &query.params()).await?;
        ArchivePage::from_json(&raw.body)
    }

    /// Lists every archived document matching `query`, following all pages.
    ///
    /// # Errors
    ///
    /// [`Error::TooManyPages`] when more than `max_pages` pages match; otherwise as
    /// [`Client::archives`].
    pub async fn archives_all(
        &self,
        emil_id: &str,
        query: &ArchiveQuery,
        max_pages: u32,
    ) -> Result<Vec<ArchiveEntry>> {
        let query = if query.page_size().is_some() {
            query.clone()
        } else {
            query.clone().size(BULK_PAGE_SIZE)
        };
        let first = self.archives(emil_id, &query.clone().page(1)).await?;
        if first.meta.total_pages > max_pages {
            return Err(Error::TooManyPages {
                limit: max_pages,
                total_pages: first.meta.total_pages,
            });
        }
        let total_pages = first.meta.total_pages;
        let mut entries = first.entries;
        for page_no in 2..=total_pages {
            entries.extend(
                self.archives(emil_id, &query.clone().page(page_no))
                    .await?
                    .entries,
            );
        }
        Ok(entries)
    }

    /// Downloads one archived document as a zip.
    ///
    /// # Download cap: at most 3 downloads of the same report per 12 months
    ///
    /// ERCOT's Data Access Portal terms say: "You may not download the same
    /// report from the Data Access Portal more than three times within a
    /// 12-month period." Do not call this (or [`Client::download_archives`]) in
    /// a loop for the same document, and do not re-download a report you
    /// already hold: keep the bytes. Terms:
    /// <https://www.ercot.com/help/terms/data-portal>.
    ///
    /// # Errors
    ///
    /// The usual authentication, transport and API errors; an HTML body is
    /// [`Error::BotCheck`] (retried first).
    pub async fn download_archive(&self, emil_id: &str, doc_id: u64) -> Result<ArchiveFile> {
        let path = format!("archive/{}", emil_id.to_ascii_lowercase());
        let params = [("download".to_owned(), doc_id.to_string())];
        let raw = self.get(&path, &params).await?;
        Ok(ArchiveFile {
            file_name: raw.file_name,
            bytes: raw.body,
        })
    }

    /// Downloads several documents in one request; the result is a zip of per-document zips
    /// (use [`ArchiveFile::extract`], which unpacks both levels).
    ///
    /// # Download cap: at most 3 downloads of the same report per 12 months
    ///
    /// Every document id in `doc_ids` counts as a download of that report under
    /// ERCOT's rule ("You may not download the same report from the Data Access
    /// Portal more than three times within a 12-month period",
    /// <https://www.ercot.com/help/terms/data-portal>). Fetch each report once
    /// and keep it.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when `doc_ids` is empty; otherwise as [`Client::download_archive`].
    pub async fn download_archives(&self, emil_id: &str, doc_ids: &[u64]) -> Result<ArchiveFile> {
        if doc_ids.is_empty() {
            return Err(Error::Config(
                "download_archives needs at least one doc id".into(),
            ));
        }
        let url = self.url(&format!(
            "archive/{}/download",
            emil_id.to_ascii_lowercase()
        ));
        let body = serde_json::json!({ "docIds": doc_ids });
        let raw = self.authed(|c| c.post(&url).json(&body)).await?;
        Ok(ArchiveFile {
            file_name: raw.file_name,
            bytes: raw.body,
        })
    }

    // ------------------------------------------------------------------ fuel mix

    /// Fetches the live fuel mix from ERCOT's dashboard feed (no credentials needed).
    ///
    /// The feed is outside the documented API; see [`FuelMix`].
    ///
    /// # Errors
    ///
    /// Transport or API errors; [`Error::Decode`] if the feed changed shape.
    pub async fn fuel_mix(&self) -> Result<FuelMix> {
        let url = &self.inner.config.fuel_mix_url;
        let raw = self.inner.http.execute(|c| c.get(url)).await?;
        FuelMix::from_json(&raw.body)
    }
}

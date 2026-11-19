//! The MISO Data Exchange API (`apim.misoenergy.org`), authenticated with an `Ocp-Apim-Subscription-Key`.
//!
//! **Untested against the live service**: signing up needs a MISO account and no key was
//! available when this client was written. What is verified: the gateway answers `401
//! Access denied due to missing subscription key` on the four pricing paths below and
//! `404` on made-up ones, so the paths exist. The response envelope (`data` plus `page`)
//! follows MISO's published API Management conventions; rows are returned as raw JSON so a
//! schema difference cannot make a call fail to decode.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{join, Client};
use crate::error::{Error, Result};

/// The header the gateway reads the key from.
const KEY_HEADER: &str = "Ocp-Apim-Subscription-Key";

/// Hard stop for [`Client::data_exchange_all_pages`], so a misbehaving server cannot loop forever.
const MAX_PAGES: u32 = 1_000;

/// Paging information from a Data Exchange response.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PageInfo {
    /// Zero-based page number.
    pub page_number: Option<u32>,
    /// Rows per page.
    pub page_size: Option<u32>,
    /// Total rows across pages.
    pub total_elements: Option<u64>,
    /// Total pages.
    pub total_pages: Option<u32>,
    /// True on the last page.
    pub last_page: Option<bool>,
}

/// One page of Data Exchange rows.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Page {
    /// The rows, as the service sent them.
    #[serde(default)]
    pub data: Vec<Value>,
    /// Paging information, when present.
    #[serde(default)]
    pub page: PageInfo,
}

impl Page {
    /// True when no further page should be requested.
    pub fn is_last(&self) -> bool {
        match (
            self.page.last_page,
            self.page.page_number,
            self.page.total_pages,
        ) {
            (Some(last), _, _) => last,
            (None, Some(n), Some(total)) => n + 1 >= total,
            _ => true,
        }
    }
}

/// A Data Exchange pricing product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PricingProduct {
    /// Day-ahead ex-post LMPs (`pricing/v1/day-ahead/{date}/lmp-expost`).
    DayAheadExPost,
    /// Day-ahead ex-ante LMPs (`pricing/v1/day-ahead/{date}/lmp-exante`).
    DayAheadExAnte,
    /// Real-time ex-ante LMPs (`pricing/v1/real-time/{date}/lmp-exante`).
    RealTimeExAnte,
    /// Real-time ex-post LMPs (`pricing/v1/real-time/{date}/lmp-expost`).
    RealTimeExPost,
}

impl PricingProduct {
    /// The request path for `date`.
    pub fn path(self, date: NaiveDate) -> String {
        let (market, product) = match self {
            Self::DayAheadExPost => ("day-ahead", "lmp-expost"),
            Self::DayAheadExAnte => ("day-ahead", "lmp-exante"),
            Self::RealTimeExAnte => ("real-time", "lmp-exante"),
            Self::RealTimeExPost => ("real-time", "lmp-expost"),
        };
        format!("pricing/v1/{market}/{}/{product}", date.format("%Y-%m-%d"))
    }
}

/// Query parameters for a Data Exchange call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataExchangeQuery {
    params: Vec<(String, String)>,
}

impl DataExchangeQuery {
    /// An empty query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a parameter such as `nodeType` or `timeResolution`; the gateway's own names apply.
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((name.into(), value.into()));
        self
    }

    /// Request a specific zero-based page (`pageNumber`).
    #[must_use]
    pub fn page_number(self, page: u32) -> Self {
        self.param("pageNumber", page.to_string())
    }

    fn with_page(&self, page: u32) -> Vec<(String, String)> {
        self.params
            .iter()
            .filter(|(k, _)| k != "pageNumber")
            .cloned()
            .chain(std::iter::once(("pageNumber".to_owned(), page.to_string())))
            .collect()
    }
}

impl Client {
    /// GET one page from the Data Exchange gateway. Needs an API key.
    pub async fn data_exchange_page(&self, path: &str, query: &DataExchangeQuery) -> Result<Page> {
        let key = self.api_key().ok_or(Error::MissingApiKey)?;
        let url = join(&self.endpoints().data_exchange, path);
        self.get_json(&url, &query.params, &[(KEY_HEADER, key.to_owned())])
            .await
    }

    /// GET every page of a Data Exchange path and concatenate the rows.
    pub async fn data_exchange_all_pages(
        &self,
        path: &str,
        query: &DataExchangeQuery,
    ) -> Result<Vec<Value>> {
        let key = self.api_key().ok_or(Error::MissingApiKey)?;
        let url = join(&self.endpoints().data_exchange, path);
        let headers = [(KEY_HEADER, key.to_owned())];
        let mut rows = Vec::new();
        for page_number in 0..MAX_PAGES {
            let page: Page = self
                .get_json(&url, &query.with_page(page_number), &headers)
                .await?;
            let last = page.is_last();
            rows.extend(page.data);
            if last {
                break;
            }
        }
        Ok(rows)
    }

    /// One page of a pricing product for `date`. Needs an API key.
    pub async fn pricing_page(
        &self,
        product: PricingProduct,
        date: NaiveDate,
        query: &DataExchangeQuery,
    ) -> Result<Page> {
        self.data_exchange_page(&product.path(date), query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_paths() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 16).expect("date");
        assert_eq!(
            PricingProduct::DayAheadExPost.path(d),
            "pricing/v1/day-ahead/2026-11-16/lmp-expost"
        );
        assert_eq!(
            PricingProduct::RealTimeExAnte.path(d),
            "pricing/v1/real-time/2026-11-16/lmp-exante"
        );
    }

    #[test]
    fn last_page_detection() {
        let mk = |last, n, total| Page {
            data: vec![],
            page: PageInfo {
                last_page: last,
                page_number: n,
                total_pages: total,
                ..PageInfo::default()
            },
        };
        assert!(mk(Some(true), Some(0), Some(5)).is_last());
        assert!(!mk(Some(false), Some(0), Some(5)).is_last());
        assert!(!mk(None, Some(1), Some(5)).is_last());
        assert!(mk(None, Some(4), Some(5)).is_last());
        assert!(mk(None, None, None).is_last());
    }

    #[test]
    fn page_override_replaces_an_earlier_page_param() {
        let q = DataExchangeQuery::new().param("a", "1").page_number(9);
        let got = q.with_page(2);
        assert_eq!(
            got,
            vec![("a".into(), "1".into()), ("pageNumber".into(), "2".into())]
        );
    }
}

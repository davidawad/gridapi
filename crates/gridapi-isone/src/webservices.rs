//! Client for the ISO New England Web Services API v1.1
//! (`https://webservices.iso-ne.com/api/v1.1`).
//!
//! The API needs an ISO Express account and HTTP basic authentication; see
//! [`Credentials`]. Responses are JSON in the form the API's JAXB serializer
//! emits: a root object holding one plural container, which holds the records
//! under their singular name. That form has three quirks this module absorbs:
//! a result with a single record is an object instead of a one-element array, an
//! empty result is `""`, and XML attributes appear as `@`-prefixed keys. Record
//! field names and types come from the API's published schema
//! (`https://webservices.iso-ne.com/docs/v1.1/ns0.xsd`).

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{
    error::Error,
    express::Revision,
    http::{Call, Credentials, RetryPolicy, Transport},
};

/// The Web Services API root.
pub const DEFAULT_BASE_URL: &str = "https://webservices.iso-ne.com/api/v1.1";

/// Which locations a query covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Every location.
    All,
    /// One location id, e.g. 4000 for the Internal Hub.
    Location(u32),
}

/// Which time a query covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Window {
    /// The most recent interval the API holds.
    Current,
    /// One operating day (Eastern).
    Day(NaiveDate),
}

/// The hourly LMP feeds the API serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HourlyLmpFeed {
    /// Final day-ahead prices (`/hourlylmp/da/final`).
    DayAheadFinal,
    /// Preliminary real-time prices (`/hourlylmp/rt/prelim`).
    RealTimePreliminary,
    /// Final real-time prices (`/hourlylmp/rt/final`); no "current" window.
    RealTimeFinal,
}

impl HourlyLmpFeed {
    const fn path(self) -> &'static str {
        match self {
            Self::DayAheadFinal => "/hourlylmp/da/final",
            Self::RealTimePreliminary => "/hourlylmp/rt/prelim",
            Self::RealTimeFinal => "/hourlylmp/rt/final",
        }
    }
}

/// A location reference as the API embeds it in records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocationRef {
    /// Location id, e.g. 4000.
    pub id: u32,
    /// `HUB`, `LOAD ZONE`, `NETWORK NODE`, ...
    pub kind: Option<String>,
    /// Location name, e.g. `.H.INTERNAL_HUB`.
    pub name: String,
}

impl<'de> Deserialize<'de> for LocationRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "@LocId", deserialize_with = "flexible_u32")]
            id: u32,
            #[serde(rename = "@LocType", default)]
            kind: Option<String>,
            #[serde(rename = "$", default)]
            name: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            id: raw.id,
            kind: raw.kind,
            name: raw.name.unwrap_or_default(),
        })
    }
}

/// A number or a numeric string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Flexible {
    Number(f64),
    Text(String),
}

fn flexible_f64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    match Option::<Flexible>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Flexible::Number(n)) => Ok(Some(n)),
        Some(Flexible::Text(s)) if s.trim().is_empty() => Ok(None),
        Some(Flexible::Text(s)) => s
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| serde::de::Error::custom(format!("not a number: {s:?}"))),
    }
}

/// `n` as a `u32` when it is a whole number in range.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "guarded by the whole-number and range checks in the same expression"
)]
fn exact_u32(n: f64) -> Option<u32> {
    (n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n)).then_some(n as u32)
}

fn flexible_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    match Flexible::deserialize(deserializer)? {
        Flexible::Number(n) => exact_u32(n),
        Flexible::Text(s) => s.trim().parse().ok(),
    }
    .ok_or_else(|| serde::de::Error::custom("not an unsigned integer"))
}

fn flexible_opt_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    Ok(flexible_f64(deserializer)?.and_then(exact_u32))
}

/// A real-time or day-ahead LMP at one location and interval.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Lmp {
    /// Start of the interval.
    pub begin_date: DateTime<FixedOffset>,
    /// The location priced.
    pub location: LocationRef,
    /// Locational marginal price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub lmp_total: Option<f64>,
    /// Energy component in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub energy_component: Option<f64>,
    /// Congestion component in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub congestion_component: Option<f64>,
    /// Loss component in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub loss_component: Option<f64>,
}

/// A dispatch fuel-mix sample.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct FuelMix {
    /// When the sample was taken.
    pub begin_date: DateTime<FixedOffset>,
    /// Generation in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub gen_mw: Option<f64>,
    /// Fuel category rollup, e.g. `Renewables`.
    pub fuel_category_rollup: String,
    /// Fuel category, e.g. `Natural Gas`.
    #[serde(default)]
    pub fuel_category: Option<String>,
    /// `Y`, `N` or `-`.
    #[serde(default)]
    pub marginal_flag: Option<String>,
}

/// Five-minute system load.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct FiveMinuteSystemLoad {
    /// Start of the interval.
    pub begin_date: DateTime<FixedOffset>,
    /// Total load in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub load_mw: Option<f64>,
    /// Native load in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub native_load: Option<f64>,
    /// Asset-related demand in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub ard_demand: Option<f64>,
    /// System load including estimated behind-the-meter solar, in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub system_load_btm_pv: Option<f64>,
    /// Native load including estimated behind-the-meter solar, in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub native_load_btm_pv: Option<f64>,
}

/// Hourly system load at one location.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct HourlySystemLoad {
    /// Start of the hour.
    pub begin_date: DateTime<FixedOffset>,
    /// The location the load is for.
    pub location: LocationRef,
    /// Load in `MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub load: Option<f64>,
    /// Native load in `MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub native_load: Option<f64>,
    /// Asset-related demand in `MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub ard_demand: Option<f64>,
}

/// Day-ahead cleared hourly demand at one location.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct HourlyDemand {
    /// Start of the hour.
    pub begin_date: DateTime<FixedOffset>,
    /// The location the demand is for.
    pub location: LocationRef,
    /// Demand in `MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub load: Option<f64>,
}

/// Five-minute regulation clearing prices.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct RegulationPrice {
    /// Start of the interval.
    pub begin_date: DateTime<FixedOffset>,
    /// Regulation clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub reg_clearing_price: Option<f64>,
    /// Regulation service clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub reg_service_clearing_price: Option<f64>,
    /// Regulation capacity clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub reg_capacity_clearing_price: Option<f64>,
    /// Hour-ending label, when the feed gives one.
    #[serde(default)]
    pub hour_end: Option<String>,
}

/// Operating-reserve requirements, designations and clearing prices for a reserve zone.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ReservePrice {
    /// Start of the interval.
    pub begin_date: DateTime<FixedOffset>,
    /// Reserve zone id.
    #[serde(default, deserialize_with = "flexible_opt_u32")]
    pub reserve_zone_id: Option<u32>,
    /// Reserve zone name.
    #[serde(default)]
    pub reserve_zone_name: Option<String>,
    /// Ten-minute spinning requirement in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub ten_min_spin_requirement: Option<f64>,
    /// Total ten-minute requirement in MW.
    #[serde(
        default,
        deserialize_with = "flexible_f64",
        rename = "Total10MinRequirement"
    )]
    pub total_10_min_requirement: Option<f64>,
    /// Total thirty-minute requirement in MW.
    #[serde(
        default,
        deserialize_with = "flexible_f64",
        rename = "Total30MinRequirement"
    )]
    pub total_30_min_requirement: Option<f64>,
    /// Designated ten-minute spinning reserve in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmsr_designated_mw: Option<f64>,
    /// Designated ten-minute non-spinning reserve in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmnsr_designated_mw: Option<f64>,
    /// Designated thirty-minute operating reserve in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmor_designated_mw: Option<f64>,
    /// Ten-minute spinning reserve clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmsr_clearing_price: Option<f64>,
    /// Ten-minute non-spinning reserve clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmnsr_clearing_price: Option<f64>,
    /// Thirty-minute operating reserve clearing price in $/`MWh`.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub tmor_clearing_price: Option<f64>,
}

/// An hourly system load forecast.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct LoadForecast {
    /// Start of the forecast hour.
    pub begin_date: DateTime<FixedOffset>,
    /// When the forecast was made.
    pub creation_date: DateTime<FixedOffset>,
    /// Forecast load in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub load_mw: Option<f64>,
    /// Load adjustment in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub load_adjustment_mw: Option<f64>,
    /// Net load in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub net_load_mw: Option<f64>,
}

/// Flow, limits and schedule on an external interface.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExternalFlow {
    /// Start of the interval.
    pub begin_date: DateTime<FixedOffset>,
    /// The interface.
    pub location: LocationRef,
    /// Actual flow in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub actual_flow: Option<f64>,
    /// Import limit in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub import_limit: Option<f64>,
    /// Export limit in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub export_limit: Option<f64>,
    /// Current schedule in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub current_schedule: Option<f64>,
    /// Purchases in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub purchase: Option<f64>,
    /// Sales in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub sale: Option<f64>,
    /// Total exports in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub total_exports: Option<f64>,
    /// Total imports in MW.
    #[serde(default, deserialize_with = "flexible_f64")]
    pub total_imports: Option<f64>,
}

/// A pricing or reporting location.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    /// Location id, e.g. 4000.
    #[serde(rename = "LocationID", deserialize_with = "flexible_u32")]
    pub location_id: u32,
    /// `HUB`, `LOAD ZONE`, `NETWORK NODE`, ...
    pub location_type: String,
    /// Location name, e.g. `.H.INTERNAL_HUB`.
    pub location_name: String,
    /// `UNIT` or `LOAD` for network nodes.
    #[serde(default)]
    pub network_node_type: Option<String>,
    /// `INTERNAL` or `EXTERNAL`.
    #[serde(default)]
    pub area_type: Option<String>,
}

/// Settings for a [`WebServices`] client.
#[derive(Debug, Clone)]
pub struct WebServicesBuilder {
    credentials: Credentials,
    base_url: String,
    user_agent: String,
    retry: RetryPolicy,
    timeout: Duration,
    connect_timeout: Duration,
}

impl WebServicesBuilder {
    fn new(credentials: Credentials) -> Self {
        Self {
            credentials,
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!("gridapi-isone/{}", env!("CARGO_PKG_VERSION")),
            retry: RetryPolicy::default(),
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
        }
    }

    /// Point the client at a different root, e.g. a mirror or a test server.
    pub fn base_url(&mut self, base_url: impl Into<String>) -> &mut Self {
        self.base_url = base_url.into();
        self
    }

    /// The `User-Agent` header sent with every request.
    pub fn user_agent(&mut self, user_agent: impl Into<String>) -> &mut Self {
        self.user_agent = user_agent.into();
        self
    }

    /// How transient failures are retried.
    pub fn retry(&mut self, retry: RetryPolicy) -> &mut Self {
        self.retry = retry;
        self
    }

    /// Whole-request timeout, including reading the body.
    pub fn timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = timeout;
        self
    }

    /// Validate the settings and build the client.
    pub fn build(&self) -> Result<WebServices, Error> {
        let base_url = self.base_url.trim_end_matches('/').to_owned();
        if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
            return Err(Error::InvalidBaseUrl(self.base_url.clone()));
        }
        let http = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|e| Error::ClientBuild(Box::new(e)))?;
        Ok(WebServices {
            inner: Arc::new(Inner {
                transport: Transport::new(http, self.retry),
                base_url,
                credentials: self.credentials.clone(),
            }),
        })
    }
}

#[derive(Debug)]
struct Inner {
    transport: Transport,
    base_url: String,
    credentials: Credentials,
}

/// A handle to the ISO-NE Web Services API. Cheap to clone; clones share one
/// connection pool.
#[derive(Debug, Clone)]
pub struct WebServices {
    inner: Arc<Inner>,
}

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WebServices>();
};

fn ymd(day: NaiveDate) -> String {
    day.format("%Y%m%d").to_string()
}

fn unsupported<T>(what: &str) -> Result<T, Error> {
    Err(Error::UnsupportedQuery(what.to_owned()))
}

/// `base` + `/current[/all|/location/<id>]` or `/day/<day>[/location/<id>]`.
fn scoped_path(base: &str, window: Window, scope: Scope, current_all: &str) -> String {
    match (window, scope) {
        (Window::Current, Scope::All) => format!("{base}/current{current_all}"),
        (Window::Current, Scope::Location(id)) => format!("{base}/current/location/{id}"),
        (Window::Day(d), Scope::All) => format!("{base}/day/{}", ymd(d)),
        (Window::Day(d), Scope::Location(id)) => format!("{base}/day/{}/location/{id}", ymd(d)),
    }
}

/// `base` + `/current` or `/day/<day>`.
fn windowed_path(base: &str, window: Window) -> String {
    match window {
        Window::Current => format!("{base}/current"),
        Window::Day(d) => format!("{base}/day/{}", ymd(d)),
    }
}

impl WebServices {
    /// A client with default settings for these credentials.
    pub fn new(credentials: Credentials) -> Result<Self, Error> {
        Self::builder(credentials).build()
    }

    /// A client with credentials from `ISONE_USERNAME` and `ISONE_PASSWORD`.
    pub fn from_env() -> Result<Self, Error> {
        Self::new(Credentials::from_env()?)
    }

    /// Settings for a customised client.
    #[must_use]
    pub fn builder(credentials: Credentials) -> WebServicesBuilder {
        WebServicesBuilder::new(credentials)
    }

    /// Five-minute LMPs. A whole day is only served for one location.
    pub async fn five_minute_lmp(
        &self,
        revision: Revision,
        window: Window,
        scope: Scope,
    ) -> Result<Vec<Lmp>, Error> {
        if matches!((window, scope), (Window::Day(_), Scope::All)) {
            return unsupported("five-minute LMPs for a whole day need a single location");
        }
        let base = format!("/fiveminutelmp/{}", revision_word(revision));
        self.records(&scoped_path(&base, window, scope, "/all"))
            .await
    }

    /// Hourly LMPs from one of the three hourly feeds.
    pub async fn hourly_lmp(
        &self,
        feed: HourlyLmpFeed,
        window: Window,
        scope: Scope,
    ) -> Result<Vec<Lmp>, Error> {
        if feed == HourlyLmpFeed::RealTimeFinal && window == Window::Current {
            return unsupported(
                "final real-time hourly LMPs have no current window; ask for a day",
            );
        }
        self.records(&scoped_path(feed.path(), window, scope, "/all"))
            .await
    }

    /// Dispatch fuel mix.
    pub async fn fuel_mix(&self, window: Window) -> Result<Vec<FuelMix>, Error> {
        self.records(&windowed_path("/genfuelmix", window)).await
    }

    /// Five-minute system load.
    pub async fn five_minute_system_load(
        &self,
        window: Window,
    ) -> Result<Vec<FiveMinuteSystemLoad>, Error> {
        self.records(&windowed_path("/fiveminutesystemload", window))
            .await
    }

    /// Hourly system load.
    pub async fn hourly_system_load(
        &self,
        window: Window,
        scope: Scope,
    ) -> Result<Vec<HourlySystemLoad>, Error> {
        self.records(&scoped_path("/hourlysysload", window, scope, ""))
            .await
    }

    /// Day-ahead cleared hourly demand. A whole day is only served for one location.
    pub async fn day_ahead_hourly_demand(
        &self,
        window: Window,
        scope: Scope,
    ) -> Result<Vec<HourlyDemand>, Error> {
        if matches!((window, scope), (Window::Day(_), Scope::All)) {
            return unsupported("day-ahead demand for a whole day needs a single location");
        }
        self.records(&scoped_path("/dayaheadhourlydemand", window, scope, ""))
            .await
    }

    /// Five-minute regulation clearing prices.
    pub async fn regulation_prices(
        &self,
        revision: Revision,
        window: Window,
    ) -> Result<Vec<RegulationPrice>, Error> {
        let base = format!("/fiveminutercp/{}", revision_word(revision));
        self.records(&windowed_path(&base, window)).await
    }

    /// Five-minute reserve zone prices and requirements.
    pub async fn reserve_prices(
        &self,
        revision: Revision,
        window: Window,
    ) -> Result<Vec<ReservePrice>, Error> {
        let base = format!("/fiveminutereserveprice/{}", revision_word(revision));
        self.records(&windowed_path(&base, window)).await
    }

    /// Hourly system load forecast.
    pub async fn load_forecast(&self, window: Window) -> Result<Vec<LoadForecast>, Error> {
        self.records(&windowed_path("/hourlyloadforecast", window))
            .await
    }

    /// Five-minute external interface flows.
    pub async fn external_flows(
        &self,
        window: Window,
        scope: Scope,
    ) -> Result<Vec<ExternalFlow>, Error> {
        self.records(&scoped_path("/fiveminuteexternalflow", window, scope, ""))
            .await
    }

    /// Every pricing and reporting location.
    pub async fn locations(&self) -> Result<Vec<Location>, Error> {
        self.records("/locations/all").await
    }

    /// Any endpoint's records as raw JSON values, for endpoints without a typed
    /// method. `path` is relative to the API root and has no extension, e.g.
    /// `/ams/current`.
    pub async fn raw_records(&self, path: &str) -> Result<Vec<Value>, Error> {
        let (url, body) = self.get_json(path).await?;
        records_of(&url, body)
    }

    /// GET `path` and decode its records as `T`.
    async fn records<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, Error> {
        let (url, body) = self.get_json(path).await?;
        let values = records_of(&url, body)?;
        let count = values.len();
        let decoded = values
            .into_iter()
            .map(|v| {
                serde_json::from_value(v).map_err(|e| Error::Json {
                    url: url.clone(),
                    source: Box::new(e),
                })
            })
            .collect::<Result<Vec<T>, _>>()?;
        tracing::info!(url, records = count, "fetched iso-ne web services records");
        Ok(decoded)
    }

    async fn get_json(&self, path: &str) -> Result<(String, Value), Error> {
        let url = format!("{}{path}.json", self.inner.base_url);
        let call = Call {
            credentials: Some(&self.inner.credentials),
            json: true,
        };
        let bytes = self.inner.transport.get(&url, call).await?;
        let value = serde_json::from_slice(&bytes).map_err(|e| Error::Json {
            url: url.clone(),
            source: Box::new(e),
        })?;
        Ok((url, value))
    }
}

const fn revision_word(revision: Revision) -> &'static str {
    match revision {
        Revision::Preliminary => "prelim",
        Revision::Final => "final",
    }
}

/// The first key of `map` that is data rather than an XML attribute or text node.
fn first_element(map: serde_json::Map<String, Value>) -> Option<Value> {
    map.into_iter()
        .find(|(key, _)| !key.starts_with('@') && key != "$")
        .map(|(_, value)| value)
}

fn is_empty_marker(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        _ => false,
    }
}

/// Unwrap `{ "<Plural>": { "<Singular>": [..] } }` into the records, tolerating a
/// single record given as an object and an empty result given as `""`.
fn records_of(url: &str, body: Value) -> Result<Vec<Value>, Error> {
    let shape = |detail: &str| Error::UnexpectedShape {
        url: url.to_owned(),
        detail: detail.to_owned(),
    };
    let Value::Object(root) = body else {
        return Err(shape("the response is not a JSON object"));
    };
    let Some(container) = first_element(root) else {
        return Ok(Vec::new());
    };
    let inner = match container {
        value if is_empty_marker(&value) => return Ok(Vec::new()),
        Value::Object(map) => match first_element(map) {
            Some(value) => value,
            None => return Ok(Vec::new()),
        },
        Value::Array(items) => return Ok(items),
        _ => return Err(shape("the record container is neither an object nor empty")),
    };
    match inner {
        value if is_empty_marker(&value) => Ok(Vec::new()),
        Value::Array(items) => Ok(items),
        record @ Value::Object(_) => Ok(vec![record]),
        _ => Err(shape("the records are neither an array nor an object")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const URL: &str = "https://example.test/x.json";

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 17).expect("date")
    }

    #[test]
    fn unwraps_an_array_of_records() {
        let body = json!({"FiveMinLmps": {"FiveMinLmp": [{"a": 1}, {"a": 2}]}});
        assert_eq!(records_of(URL, body).expect("ok").len(), 2);
    }

    #[test]
    fn a_single_record_arrives_as_an_object() {
        let body = json!({"FiveMinLmps": {"FiveMinLmp": {"a": 1}}});
        assert_eq!(records_of(URL, body).expect("ok"), [json!({"a": 1})]);
    }

    #[test]
    fn empty_results_are_empty() {
        for body in [
            json!({"FiveMinLmps": ""}),
            json!({"FiveMinLmps": null}),
            json!({"FiveMinLmps": {}}),
            json!({"FiveMinLmps": {"FiveMinLmp": ""}}),
            json!({"FiveMinLmps": {"FiveMinLmp": []}}),
            json!({}),
        ] {
            assert!(
                records_of(URL, body.clone()).expect("ok").is_empty(),
                "{body}"
            );
        }
    }

    #[test]
    fn xml_attribute_keys_are_skipped() {
        let body =
            json!({"HourlyLmps": {"@xmlns": "http://WEBSERV.iso-ne.com", "HourlyLmp": [{"a": 1}]}});
        assert_eq!(records_of(URL, body).expect("ok"), [json!({"a": 1})]);
    }

    #[test]
    fn a_non_object_response_is_an_unexpected_shape() {
        for body in [json!([1]), json!("x"), json!(7)] {
            let err = records_of(URL, body).expect_err("not an object");
            assert!(matches!(err, Error::UnexpectedShape { .. }), "got {err:?}");
        }
    }

    #[test]
    fn location_refs_read_attributes_and_text() {
        let loc: LocationRef = serde_json::from_value(
            json!({"@LocId": 4000, "@LocType": "HUB", "$": ".H.INTERNAL_HUB"}),
        )
        .expect("decodes");
        assert_eq!(
            loc,
            LocationRef {
                id: 4000,
                kind: Some("HUB".into()),
                name: ".H.INTERNAL_HUB".into()
            }
        );
        let stringy: LocationRef =
            serde_json::from_value(json!({"@LocId": "4001", "$": ".Z.MAINE"})).expect("string id");
        assert_eq!(stringy.id, 4001);
        assert_eq!(stringy.kind, None);
    }

    #[test]
    fn lmp_accepts_numbers_strings_and_blanks() {
        let lmp: Lmp = serde_json::from_value(json!({
            "BeginDate": "2026-11-17T20:00:00.000-05:00",
            "Location": {"@LocId": 4000, "@LocType": "HUB", "$": ".H.INTERNAL_HUB"},
            "LmpTotal": 52.19, "EnergyComponent": "79.55", "CongestionComponent": "", "LossComponent": null
        }))
        .expect("decodes");
        assert_eq!(lmp.lmp_total, Some(52.19));
        assert_eq!(lmp.energy_component, Some(79.55));
        assert_eq!(lmp.congestion_component, None, "blank string is absent");
        assert_eq!(lmp.loss_component, None);
        assert_eq!(lmp.begin_date.to_rfc3339(), "2026-11-17T20:00:00-05:00");
    }

    #[test]
    fn a_non_numeric_price_is_an_error_not_zero() {
        let err = serde_json::from_value::<Lmp>(json!({
            "BeginDate": "2026-11-17T20:00:00.000-05:00",
            "Location": {"@LocId": 4000, "$": "x"},
            "LmpTotal": "n/a"
        }))
        .expect_err("n/a");
        assert!(err.to_string().contains("not a number"), "got {err}");
    }

    #[test]
    fn location_master_data_uses_the_schema_names() {
        let loc: Location = serde_json::from_value(json!({
            "LocationID": 4000, "LocationType": "HUB", "LocationName": ".H.INTERNAL_HUB",
            "NetworkNodeType": "LOAD", "AreaType": "INTERNAL"
        }))
        .expect("decodes");
        assert_eq!(loc.location_id, 4000);
        assert_eq!(loc.location_name, ".H.INTERNAL_HUB");
    }

    #[test]
    fn reserve_price_reads_the_digit_bearing_names() {
        let r: ReservePrice = serde_json::from_value(json!({
            "BeginDate": "2026-11-17T20:00:00.000-05:00",
            "ReserveZoneId": 7000, "Total10MinRequirement": 1982.4, "Total30MinRequirement": "2500",
            "TmsrClearingPrice": 0
        }))
        .expect("decodes");
        assert_eq!(r.reserve_zone_id, Some(7000));
        assert_eq!(r.total_10_min_requirement, Some(1982.4));
        assert_eq!(r.total_30_min_requirement, Some(2500.0));
        assert_eq!(r.tmsr_clearing_price, Some(0.0));
    }

    #[test]
    fn paths_follow_the_documented_resources() {
        let d = day();
        let five = |w, s| scoped_path("/fiveminutelmp/prelim", w, s, "/all");
        assert_eq!(
            five(Window::Current, Scope::All),
            "/fiveminutelmp/prelim/current/all"
        );
        assert_eq!(
            five(Window::Current, Scope::Location(4000)),
            "/fiveminutelmp/prelim/current/location/4000"
        );
        assert_eq!(
            five(Window::Day(d), Scope::Location(4000)),
            "/fiveminutelmp/prelim/day/20261117/location/4000"
        );
        assert_eq!(
            scoped_path("/hourlysysload", Window::Current, Scope::All, ""),
            "/hourlysysload/current"
        );
        assert_eq!(
            scoped_path("/hourlysysload", Window::Day(d), Scope::All, ""),
            "/hourlysysload/day/20261117"
        );
        assert_eq!(
            windowed_path("/genfuelmix", Window::Day(d)),
            "/genfuelmix/day/20261117"
        );
        assert_eq!(
            windowed_path("/genfuelmix", Window::Current),
            "/genfuelmix/current"
        );
        assert_eq!(HourlyLmpFeed::DayAheadFinal.path(), "/hourlylmp/da/final");
        assert_eq!(
            HourlyLmpFeed::RealTimePreliminary.path(),
            "/hourlylmp/rt/prelim"
        );
        assert_eq!(HourlyLmpFeed::RealTimeFinal.path(), "/hourlylmp/rt/final");
    }

    #[test]
    fn rejects_a_base_url_without_a_scheme() {
        let err = WebServices::builder(Credentials::new("a", "b"))
            .base_url("webservices.iso-ne.com/api/v1.1")
            .build()
            .expect_err("no scheme");
        assert!(matches!(err, Error::InvalidBaseUrl(_)), "got {err:?}");
    }
}

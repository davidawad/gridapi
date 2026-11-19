//! Response types: catalog, metadata, usage, dynamic records and paging.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::response::ResponseMeta;

/// One column of a dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// Column name as used in `columns` and `filter_column`.
    pub name: String,
    /// SQL-ish type name (`TIMESTAMP`, `VARCHAR`, `DOUBLE PRECISION`, ...).
    #[serde(rename = "type", default)]
    pub sql_type: String,
    /// True for numeric columns.
    #[serde(default)]
    pub is_numeric: bool,
    /// True for date columns.
    #[serde(default)]
    pub is_date: bool,
    /// True for timestamp columns.
    #[serde(default)]
    pub is_datetime: bool,
}

/// Dataset metadata. The catalog (`GET /datasets`), the single-dataset
/// endpoint and the `dataset_metadata` block of a query response all use this
/// shape, with the query block carrying fewer fields, so everything but the id
/// is optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    /// Dataset id, e.g. `ercot_spp_day_ahead_hourly`.
    pub id: String,
    /// Human name.
    #[serde(default)]
    pub name: Option<String>,
    /// Markdown description.
    #[serde(default)]
    pub description: Option<String>,
    /// Source system, e.g. `ercot`.
    #[serde(default)]
    pub source: Option<String>,
    /// Earliest row time.
    #[serde(default)]
    pub earliest_available_time_utc: Option<DateTime<Utc>>,
    /// Latest row time.
    #[serde(default)]
    pub latest_available_time_utc: Option<DateTime<Utc>>,
    /// When Grid Status last polled the source.
    #[serde(default)]
    pub last_checked_time_utc: Option<DateTime<Utc>>,
    /// Primary key columns.
    #[serde(default)]
    pub primary_key_columns: Option<Vec<String>>,
    /// Column holding the forecast publish time, if any.
    #[serde(default)]
    pub publish_time_column: Option<String>,
    /// Column `start_time`/`end_time` apply to.
    #[serde(default)]
    pub time_index_column: Option<String>,
    /// Column that distinguishes series (usually `location`).
    #[serde(default)]
    pub subseries_index_column: Option<String>,
    /// Timezone of naive timestamps in the source.
    #[serde(default)]
    pub data_timezone: Option<String>,
    /// All columns with types.
    #[serde(default)]
    pub all_columns: Option<Vec<Column>>,
    /// Approximate row count. Signed: the live catalog uses `-1` for "unknown".
    #[serde(default)]
    pub number_of_rows_approximate: Option<i64>,
    /// Row cadence, e.g. `1_HOUR`, `5_MINUTES`.
    #[serde(default)]
    pub data_frequency: Option<String>,
    /// Upstream documentation URL.
    #[serde(default)]
    pub source_url: Option<String>,
    /// `active`, `deprecated` or `retired`.
    #[serde(default)]
    pub status: Option<String>,
    /// Popularity ranking (1 is most used). Signed: the live catalog can carry `-1`.
    #[serde(default)]
    pub popularity_rank: Option<i64>,
}

impl Dataset {
    /// Names of the columns, in table order.
    pub fn column_names(&self) -> Vec<&str> {
        self.all_columns
            .iter()
            .flatten()
            .map(|c| c.name.as_str())
            .collect()
    }
}

/// The catalog of datasets visible to the key.
#[derive(Debug, Clone, PartialEq)]
pub struct DatasetList {
    /// Every dataset, in server order.
    pub datasets: Vec<Dataset>,
    /// Response headers.
    pub response: ResponseMeta,
}

impl DatasetList {
    /// Look a dataset up by exact id.
    pub fn get(&self, id: &str) -> Option<&Dataset> {
        self.datasets.iter().find(|d| d.id == id)
    }

    /// Datasets whose id starts with `prefix` (e.g. `"ercot_"`).
    pub fn with_prefix<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a Dataset> + 'a {
        self.datasets
            .iter()
            .filter(move |d| d.id.starts_with(prefix))
    }
}

/// Plan limits from `GET /api_usage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLimits {
    /// Rows per billing period.
    pub api_rows_returned_limit: u64,
    /// Requests per billing period.
    pub api_requests_limit: u64,
    /// Max rows in one response.
    pub api_rows_per_response_limit: u64,
    /// Requests per second.
    pub per_second_api_rate_limit: u64,
    /// Requests per minute.
    pub per_minute_api_rate_limit: u64,
    /// Requests per hour.
    pub per_hour_api_rate_limit: u64,
}

/// Usage so far in the current billing period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodUsage {
    /// Requests made.
    pub total_requests: u64,
    /// Rows returned.
    pub total_api_rows_returned: u64,
}

/// `GET /api_usage`: the authoritative quota readout (the API sends no
/// quota headers on the free tier).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Plan name, e.g. `Free`.
    pub plan_name: String,
    /// Plan limits.
    pub limits: UsageLimits,
    /// Billing period start.
    pub current_usage_period_start: DateTime<Utc>,
    /// Billing period end.
    pub current_usage_period_end: DateTime<Utc>,
    /// Consumption so far.
    pub current_period_usage: PeriodUsage,
}

impl Usage {
    /// Requests left this period (saturating at zero).
    pub fn remaining_requests(&self) -> u64 {
        self.limits
            .api_requests_limit
            .saturating_sub(self.current_period_usage.total_requests)
    }

    /// Rows left this period (saturating at zero).
    pub fn remaining_rows(&self) -> u64 {
        self.limits
            .api_rows_returned_limit
            .saturating_sub(self.current_period_usage.total_api_rows_returned)
    }
}

/// One dataset update event from `GET /dataset-updates/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetUpdate {
    /// Update id.
    #[serde(deserialize_with = "string_or_number")]
    pub id: String,
    /// Dataset id.
    pub dataset: String,
    /// When the update landed.
    pub time_utc: DateTime<Utc>,
    /// Rows changed.
    #[serde(default)]
    pub num_rows_updated: u64,
    /// Rows added.
    #[serde(default)]
    pub num_rows_inserted: u64,
    /// True when the update is a historical backfill.
    #[serde(default)]
    pub is_backfill: Option<bool>,
}

fn string_or_number<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match Value::deserialize(d)? {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number id, got {other}"
        ))),
    }
}

/// Paging block of a query response.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PageMeta {
    /// 1-based page number.
    #[serde(default)]
    pub page: Option<u64>,
    /// The request's overall row limit.
    #[serde(default)]
    pub limit: Option<u64>,
    /// Rows per page.
    #[serde(default)]
    pub page_size: Option<u64>,
    /// True when another page exists.
    #[serde(default, rename = "hasNextPage")]
    pub has_next_page: Option<bool>,
    /// Cursor to pass for the next page.
    #[serde(default)]
    pub cursor: Option<String>,
}

/// One row as a dynamic record: column name to JSON value.
///
/// Use the typed accessors, or [`Record::as_map`] for raw access.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Record(Map<String, Value>);

impl Record {
    /// Raw column map.
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.0
    }

    /// Consume into the raw column map.
    pub fn into_map(self) -> Map<String, Value> {
        self.0
    }

    /// Raw value of a column.
    pub fn value(&self, column: &str) -> Option<&Value> {
        self.0.get(column)
    }

    /// Column as a string slice (`None` when absent, null or not a string).
    pub fn str(&self, column: &str) -> Option<&str> {
        self.0.get(column).and_then(Value::as_str)
    }

    /// Column as `f64` (integers widen; `None` when absent, null or non-numeric).
    pub fn f64(&self, column: &str) -> Option<f64> {
        self.0.get(column).and_then(Value::as_f64)
    }

    /// Column as `i64`.
    pub fn i64(&self, column: &str) -> Option<i64> {
        self.0.get(column).and_then(Value::as_i64)
    }

    /// Column as a UTC timestamp, parsing RFC 3016.8 with any offset.
    pub fn timestamp(&self, column: &str) -> Option<DateTime<Utc>> {
        self.str(column)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
    }

    /// All numeric columns as a sorted map (useful for fuel mixes).
    pub fn numeric_columns(&self) -> BTreeMap<&str, f64> {
        self.0
            .iter()
            .filter_map(|(k, v)| v.as_f64().map(|n| (k.as_str(), n)))
            .collect()
    }
}

/// One page of query results.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPage<R> {
    /// The rows.
    pub rows: Vec<R>,
    /// Paging block from the body.
    pub meta: PageMeta,
    /// Dataset metadata echoed by the server.
    pub dataset: Option<Dataset>,
    /// Response headers (rows in response, request id, rate-limit headers if any).
    pub response: ResponseMeta,
}

impl<R> QueryPage<R> {
    /// True when the server reports another page.
    pub fn has_next_page(&self) -> bool {
        self.meta.has_next_page.unwrap_or(false)
    }

    /// Transform each row, keeping the paging and header metadata.
    pub fn map_rows<T>(self, f: impl FnMut(R) -> T) -> QueryPage<T> {
        QueryPage {
            rows: self.rows.into_iter().map(f).collect(),
            meta: self.meta,
            dataset: self.dataset,
            response: self.response,
        }
    }
}

/// Wire envelope of a query response.
#[derive(Debug, Deserialize)]
pub(crate) struct QueryEnvelope<R> {
    pub(crate) data: Vec<R>,
    #[serde(default)]
    pub(crate) meta: PageMeta,
    #[serde(default)]
    pub(crate) dataset_metadata: Option<Dataset>,
}

/// Wire envelope of the catalog response.
#[derive(Debug, Deserialize)]
pub(crate) struct CatalogEnvelope {
    pub(crate) data: Vec<Dataset>,
}

/// Wire envelope of the dataset-updates response.
#[derive(Debug, Deserialize)]
pub(crate) struct UpdatesEnvelope {
    pub(crate) data: Vec<DatasetUpdate>,
}

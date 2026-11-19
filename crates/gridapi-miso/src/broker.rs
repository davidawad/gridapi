//! The legacy real-time data broker (`api.misoenergy.org/MISORTWDDataBroker/DataBrokerServices.asmx`).
//!
//! MISO moved its real-time feeds to the public API on 2025-12-12 (see
//! <https://www.misoenergy.org/markets-and-operations/rtdataapis>). The old endpoint still
//! answers HTTP 200, but with `{"error": "no data", ...}` for every message type, which this
//! module surfaces as [`Error::NoData`]. It is kept so code written against the broker fails
//! with a clear error instead of a decode mystery; use [`Client::fuel_mix`],
//! [`Client::total_load`] and friends instead.

use serde_json::Value;

use crate::client::{join, Client};
use crate::error::{Error, Result};

/// The broker's message type for each modern equivalent, for migration.
///
/// `(message type, replacement method on [`Client`])`.
pub const REPLACEMENTS: &[(&str, &str)] = &[
    ("getlmpconsolidatedtable", "lmp_consolidated_table"),
    ("getexantelmp", "ex_ante_lmp"),
    ("getfuelmix", "fuel_mix"),
    ("gettotalload", "total_load"),
    ("getWindForecast", "renewable_forecast(Renewable::Wind)"),
    ("getnai", "net_actual_interchange"),
];

impl Client {
    /// Request a message type from the legacy data broker.
    ///
    /// # Errors
    /// [`Error::NoData`] when the broker answers `{"error": "no data"}`, which is currently always.
    pub async fn data_broker(&self, message_type: &str) -> Result<Value> {
        let url = join(&self.endpoints().data_broker, "DataBrokerServices.asmx");
        let query = [
            ("messageType".to_owned(), message_type.to_owned()),
            ("returnType".to_owned(), "json".to_owned()),
        ];
        let value: Value = self.get_json(&url, &query, &[]).await?;
        match value.get("error").and_then(Value::as_str) {
            Some(msg) if msg.eq_ignore_ascii_case("no data") => Err(Error::NoData { url }),
            _ => Ok(value),
        }
    }
}

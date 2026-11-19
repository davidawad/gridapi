//! System conditions from the keyless public API: fuel mix, load, wind and solar, interchange, constraints.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{join, Client};
use crate::error::{Error, Result};
use crate::serde_util::{f64_loose, opt_f64_loose, opt_timestamp, timestamp, u32_loose};
use crate::time::parse_timestamp;

// ----------------------------------------------------------------- fuel mix

/// Generation by fuel category in one interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuelEntry {
    /// Interval on the EST market clock.
    #[serde(rename = "INTERVALEST", deserialize_with = "timestamp")]
    pub interval: NaiveDateTime,
    /// Fuel category: `Coal`, `Natural Gas`, `Nuclear`, `Wind`, `Solar`, `Battery Storage`, `Imports`, `Other`.
    #[serde(rename = "CATEGORY")]
    pub category: String,
    /// Output in MW (negative for charging storage or exports).
    #[serde(rename = "ACT", deserialize_with = "f64_loose")]
    pub mw: f64,
}

/// MISO generation by fuel (`/api/FuelMix`, `/api/FuelMix/Today`, `/api/FuelMix/Yesterday`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "FuelMixWire")]
pub struct FuelMix {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// Total generation in MW as published (the daily views publish `0`).
    pub total_mw: f64,
    /// One entry per fuel per interval.
    pub entries: Vec<FuelEntry>,
}

impl FuelMix {
    /// The entries of the most recent interval only.
    pub fn latest(&self) -> Vec<&FuelEntry> {
        let latest = self.entries.iter().map(|e| e.interval).max();
        self.entries
            .iter()
            .filter(|e| Some(e.interval) == latest)
            .collect()
    }
}

#[derive(Deserialize)]
struct FuelMixWire {
    #[serde(rename = "RefId")]
    ref_id: String,
    #[serde(rename = "TotalMW", deserialize_with = "f64_loose")]
    total_mw: f64,
    #[serde(rename = "Fuel")]
    fuel: FuelTypes,
}
#[derive(Deserialize)]
struct FuelTypes {
    #[serde(rename = "Type")]
    entries: Vec<FuelEntry>,
}
impl From<FuelMixWire> for FuelMix {
    fn from(w: FuelMixWire) -> Self {
        Self {
            ref_id: w.ref_id,
            total_mw: w.total_mw,
            entries: w.fuel.entries,
        }
    }
}

/// Which day of fuel mix to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FuelMixView {
    /// The latest interval.
    Latest,
    /// Every interval today.
    Today,
    /// Every interval yesterday.
    Yesterday,
}

impl FuelMixView {
    fn path(self) -> &'static str {
        match self {
            Self::Latest => "api/FuelMix",
            Self::Today => "api/FuelMix/Today",
            Self::Yesterday => "api/FuelMix/Yesterday",
        }
    }
}

// --------------------------------------------------------------------- load

/// A value for one hour ending.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HourlyMw {
    /// Hour ending, 1 through 24, on the EST market clock.
    pub hour_ending: u32,
    /// Megawatts.
    pub mw: f64,
}

/// A five-minute load observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadPoint {
    /// `HH:MM` on the EST market clock.
    pub time: String,
    /// Megawatts.
    pub mw: f64,
}

/// MISO load: day-ahead cleared, medium-term forecast and five-minute actuals (`/api/RealTimeTotalLoad`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "LoadWire")]
pub struct TotalLoad {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// Day-ahead cleared demand by hour.
    pub cleared: Vec<HourlyMw>,
    /// Medium-term load forecast by hour.
    pub forecast: Vec<HourlyMw>,
    /// Five-minute actual load so far today.
    pub actual: Vec<LoadPoint>,
}

#[derive(Deserialize)]
struct LoadWire {
    #[serde(rename = "LoadInfo")]
    info: LoadInfo,
}
#[derive(Deserialize)]
struct LoadInfo {
    #[serde(rename = "RefId")]
    ref_id: String,
    #[serde(rename = "ClearedMW", default)]
    cleared: Vec<ClearedWrap>,
    #[serde(rename = "MediumTermLoadForecast", default)]
    forecast: Vec<ForecastWrap>,
    #[serde(rename = "FiveMinTotalLoad", default)]
    actual: Vec<ActualWrap>,
}
#[derive(Deserialize)]
struct ClearedWrap {
    #[serde(rename = "ClearedMWHourly")]
    v: HourValue,
}
#[derive(Deserialize)]
struct HourValue {
    #[serde(rename = "Hour", deserialize_with = "u32_loose")]
    hour: u32,
    #[serde(rename = "Value", deserialize_with = "f64_loose")]
    value: f64,
}
#[derive(Deserialize)]
struct ForecastWrap {
    #[serde(rename = "Forecast")]
    v: ForecastValue,
}
#[derive(Deserialize)]
struct ForecastValue {
    #[serde(rename = "HourEnding", deserialize_with = "u32_loose")]
    hour: u32,
    #[serde(rename = "LoadForecast", deserialize_with = "f64_loose")]
    value: f64,
}
#[derive(Deserialize)]
struct ActualWrap {
    #[serde(rename = "Load")]
    v: ActualValue,
}
#[derive(Deserialize)]
struct ActualValue {
    #[serde(rename = "Time")]
    time: String,
    #[serde(rename = "Value", deserialize_with = "f64_loose")]
    value: f64,
}
impl From<LoadWire> for TotalLoad {
    fn from(w: LoadWire) -> Self {
        let i = w.info;
        Self {
            ref_id: i.ref_id,
            cleared: i
                .cleared
                .into_iter()
                .map(|c| HourlyMw {
                    hour_ending: c.v.hour,
                    mw: c.v.value,
                })
                .collect(),
            forecast: i
                .forecast
                .into_iter()
                .map(|f| HourlyMw {
                    hour_ending: f.v.hour,
                    mw: f.v.value,
                })
                .collect(),
            actual: i
                .actual
                .into_iter()
                .map(|a| LoadPoint {
                    time: a.v.time,
                    mw: a.v.value,
                })
                .collect(),
        }
    }
}

// --------------------------------------------------------------- wind/solar

/// Wind or solar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Renewable {
    /// Wind generation.
    Wind,
    /// Solar generation.
    Solar,
}

impl Renewable {
    fn slug(self) -> &'static str {
        match self {
            Self::Wind => "wind",
            Self::Solar => "solar",
        }
    }
}

/// An hourly MW value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyPoint {
    /// Hour start on the EST market clock.
    #[serde(rename = "DateTimeEST", deserialize_with = "timestamp")]
    pub time: NaiveDateTime,
    /// Hour ending, 1 through 24.
    #[serde(rename = "HourEndingEST", deserialize_with = "u32_loose")]
    pub hour_ending: u32,
    /// Megawatts.
    #[serde(rename = "Value", deserialize_with = "f64_loose")]
    pub mw: f64,
}

/// An hour's forecast with the actual alongside once it is known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastVsActual {
    /// Hour start on the EST market clock.
    #[serde(rename = "ForecastDateTimeEST", deserialize_with = "timestamp")]
    pub time: NaiveDateTime,
    /// Hour ending, 1 through 24.
    #[serde(rename = "ForecastHourEndingEST", deserialize_with = "u32_loose")]
    pub hour_ending: u32,
    /// Forecast MW.
    #[serde(rename = "ForecastValue", deserialize_with = "f64_loose")]
    pub forecast_mw: f64,
    /// Actual hour start, once reported.
    #[serde(
        rename = "ActualDateTimeEST",
        default,
        deserialize_with = "opt_timestamp"
    )]
    pub actual_time: Option<NaiveDateTime>,
    /// Actual MW, once reported.
    #[serde(rename = "ActualValue", default, deserialize_with = "opt_f64_loose")]
    pub actual_mw: Option<f64>,
}

#[derive(Deserialize)]
struct Instance<T> {
    instance: Vec<T>,
}
#[derive(Deserialize)]
struct Forecast<T> {
    #[serde(rename = "Forecast")]
    forecast: Vec<T>,
}

// ------------------------------------------------------------ interchange

/// MISO's net actual interchange (`/api/Interchange/GetNai`); positive is export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "NaiWire")]
pub struct NetActualInterchange {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// `HH:MM` on the EST market clock.
    pub hour_and_min: String,
    /// Net interchange in MW.
    pub mw: f64,
}

#[derive(Deserialize)]
struct NaiWire {
    #[serde(rename = "Date")]
    date: String,
    #[serde(rename = "Actual")]
    actual: NaiActual,
}
#[derive(Deserialize)]
struct NaiActual {
    #[serde(rename = "HourandMin")]
    hour_and_min: String,
    #[serde(rename = "TieFlow")]
    tie_flow: NaiTie,
}
#[derive(Deserialize)]
struct NaiTie {
    #[serde(rename = "Value", deserialize_with = "f64_loose")]
    value: f64,
}
impl From<NaiWire> for NetActualInterchange {
    fn from(w: NaiWire) -> Self {
        Self {
            ref_id: w.date,
            hour_and_min: w.actual.hour_and_min,
            mw: w.actual.tie_flow.value,
        }
    }
}

/// MISO's net scheduled interchange with each neighbour (`/api/Interchange/GetNsi`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NetScheduledInterchange {
    /// MISO's interval stamp.
    pub ref_id: String,
    /// The schedule timestamp on the EST market clock.
    pub timestamp: Option<NaiveDateTime>,
    /// MW by neighbouring balancing authority (`PJM`, `SWPP`, `TVA`, ...), in name order.
    pub flows_mw: BTreeMap<String, f64>,
}

// ----------------------------------------------------------------- misc

/// One line of the public dashboard snapshot (`/api/Snapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotItem {
    /// Title, e.g. `Current Demand (MW)`.
    #[serde(rename = "t")]
    pub title: String,
    /// Display value, e.g. `70,688` or `$14.39`.
    #[serde(rename = "v")]
    pub value: String,
    /// As-of stamp as MISO displays it.
    #[serde(rename = "d")]
    pub as_of: String,
}

/// A real-time binding constraint and its shadow price (`/api/BindingConstraints/RealTime`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingConstraint {
    /// Constraint name.
    #[serde(rename = "Name")]
    pub name: String,
    /// Interval on the EST market clock.
    #[serde(rename = "Period", default, deserialize_with = "opt_timestamp")]
    pub period: Option<NaiveDateTime>,
    /// Shadow price, $/MWh.
    #[serde(rename = "Price", default, deserialize_with = "opt_f64_loose")]
    pub price: Option<f64>,
    /// Constraint curve type, e.g. `PERCENT`.
    #[serde(rename = "CURVETYPE", default)]
    pub curve_type: Option<String>,
}

#[derive(Deserialize)]
struct ConstraintsWire {
    #[serde(rename = "RefId")]
    _ref_id: String,
    #[serde(rename = "Constraint")]
    constraints: Vec<BindingConstraint>,
}

/// One area-control-error sample (`/api/Ace`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcePoint {
    /// Sample time on the EST market clock.
    #[serde(rename = "instantEST", deserialize_with = "timestamp")]
    pub time: NaiveDateTime,
    /// ACE in MW.
    #[serde(rename = "value", deserialize_with = "f64_loose")]
    pub mw: f64,
}

#[derive(Deserialize)]
struct AceWire {
    #[serde(rename = "ACE")]
    ace: Vec<AcePoint>,
}

// ------------------------------------------------------------------- client

impl Client {
    /// Generation by fuel category.
    pub async fn fuel_mix(&self, view: FuelMixView) -> Result<FuelMix> {
        let url = join(&self.endpoints().public_api, view.path());
        self.get_json(&url, &[], &[]).await
    }

    /// Day-ahead cleared load, forecast and five-minute actual load.
    pub async fn total_load(&self) -> Result<TotalLoad> {
        let url = join(&self.endpoints().public_api, "api/RealTimeTotalLoad");
        self.get_json(&url, &[], &[]).await
    }

    /// Hourly forecast with actuals for wind or solar, side by side.
    pub async fn forecast_vs_actual(&self, kind: Renewable) -> Result<Vec<ForecastVsActual>> {
        let url = join(
            &self.endpoints().public_api,
            &format!("api/WindSolar/get{}", kind.slug()),
        );
        let body: Instance<ForecastVsActual> = self.get_json(&url, &[], &[]).await?;
        Ok(body.instance)
    }

    /// The hourly day-ahead forecast for wind or solar.
    pub async fn renewable_forecast(&self, kind: Renewable) -> Result<Vec<HourlyPoint>> {
        let url = join(
            &self.endpoints().public_api,
            &format!("api/WindSolar/get{}forecast", kind.slug()),
        );
        let body: Forecast<HourlyPoint> = self.get_json(&url, &[], &[]).await?;
        Ok(body.forecast)
    }

    /// The hourly actual output reported so far today for wind or solar.
    pub async fn renewable_actual(&self, kind: Renewable) -> Result<Vec<HourlyPoint>> {
        let url = join(
            &self.endpoints().public_api,
            &format!("api/WindSolar/get{}actual", kind.slug()),
        );
        let body: Instance<HourlyPoint> = self.get_json(&url, &[], &[]).await?;
        Ok(body.instance)
    }

    /// MISO's latest net actual interchange.
    pub async fn net_actual_interchange(&self) -> Result<NetActualInterchange> {
        let url = join(&self.endpoints().public_api, "api/Interchange/GetNai");
        self.get_json(&url, &[], &[]).await
    }

    /// MISO's latest net scheduled interchange, by neighbour.
    pub async fn net_scheduled_interchange(&self) -> Result<NetScheduledInterchange> {
        let url = join(&self.endpoints().public_api, "api/Interchange/GetNsi");
        let value: Value = self.get_json(&url, &[], &[]).await?;
        decode_nsi(&value, &url)
    }

    /// The headline numbers MISO shows on its dashboard.
    pub async fn snapshot(&self) -> Result<Vec<SnapshotItem>> {
        let url = join(&self.endpoints().public_api, "api/Snapshot");
        self.get_json(&url, &[], &[]).await
    }

    /// Real-time binding constraints with shadow prices.
    pub async fn binding_constraints(&self) -> Result<Vec<BindingConstraint>> {
        let url = join(
            &self.endpoints().public_api,
            "api/BindingConstraints/RealTime",
        );
        let wire: ConstraintsWire = self.get_json(&url, &[], &[]).await?;
        Ok(wire.constraints)
    }

    /// Area control error samples for the last stretch of the day.
    pub async fn ace(&self) -> Result<Vec<AcePoint>> {
        let url = join(&self.endpoints().public_api, "api/Ace");
        let wire: AceWire = self.get_json(&url, &[], &[]).await?;
        Ok(wire.ace)
    }

    /// GET any path under the public API host and return the raw JSON.
    ///
    /// The escape hatch for endpoints without a typed method (`api/CsatSupplyDemand`,
    /// `api/RegionalDirectionalTransfer`, ...).
    pub async fn public_api_value(&self, path: &str) -> Result<Value> {
        let url = join(&self.endpoints().public_api, path);
        self.get_json(&url, &[], &[]).await
    }
}

fn decode_nsi(value: &Value, what: &str) -> Result<NetScheduledInterchange> {
    let row = value
        .get("instance")
        .and_then(|i| i.get(0))
        .and_then(Value::as_object)
        .ok_or_else(|| Error::decode(what, "no instance row"))?;
    let flows_mw = row
        .iter()
        .filter(|(k, _)| k.as_str() != "timestamp")
        .filter_map(|(k, v)| {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            n.map(|n| (k.clone(), n))
        })
        .collect();
    Ok(NetScheduledInterchange {
        ref_id: value
            .get("RefId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        timestamp: row
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
        flows_mw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nsi_flows_are_numeric_and_sorted() {
        let v: Value = serde_json::from_str(
            r#"{"instance":[{"timestamp":"2026-11-18 01:18:00","PJM":"483","AECI":"277","X":"n/a"}],"RefId":"r"}"#,
        )
        .expect("json");
        let nsi = decode_nsi(&v, "t").expect("nsi");
        assert_eq!(nsi.flows_mw.keys().collect::<Vec<_>>(), vec!["AECI", "PJM"]);
        assert_eq!(nsi.flows_mw["PJM"], 483.0);
        assert!(nsi.timestamp.is_some());
    }

    #[test]
    fn nsi_without_rows_is_a_decode_error() {
        let v: Value = serde_json::from_str(r#"{"instance":[]}"#).expect("json");
        assert!(decode_nsi(&v, "t").is_err());
    }
}

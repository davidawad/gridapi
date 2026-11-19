//! Hourly load by control zone.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::browser::HOURLY_LOAD;
use crate::error::{Error, Result};
use crate::Client;

/// Timestamp format of the load files (`01/05/2026 07:00:00`).
const STAMP: &str = "%m/%d/%Y %H:%M:%S";

/// One control zone's load for one hour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneLoad {
    /// The end of the market hour, in UTC (the files label hours by their GMT end).
    pub hour_end_utc: DateTime<Utc>,
    /// Balancing authority area (`SPP`, `SWPW`); `None` in the daily files, which hold SPP zones only.
    pub balancing_area: Option<String>,
    /// Control zone, e.g. `CSWS`, `OKGE`, `WAUE`.
    pub zone: String,
    /// Forecast area type as published (monthly files only), e.g. `CF` or `NC`.
    pub forecast_area_type: Option<String>,
    /// Average load for the hour, MW.
    pub mw: f64,
}

/// Parse a daily (wide: one column per zone) or monthly (long) hourly-load file.
pub fn parse_hourly_load(bytes: &[u8]) -> Result<Vec<ZoneLoad>> {
    let what = "SPP hourly load file";
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| Error::decode(what, e))?
        .clone();
    let names: Vec<String> = headers.iter().map(|h| h.trim().to_owned()).collect();
    let lower: HashMap<String, usize> = names
        .iter()
        .enumerate()
        .map(|(i, h)| (h.to_ascii_lowercase(), i))
        .collect();
    let hour_col = lower
        .get("market hour")
        .or_else(|| lower.get("markethour"))
        .copied()
        .ok_or_else(|| Error::decode(what, "no market hour column"))?;
    let long = lower.contains_key("load mw");
    let mut out = Vec::new();
    for (n, rec) in reader.records().enumerate() {
        let rec = rec.map_err(|e| Error::decode(what, format!("row {}: {e}", n + 2)))?;
        let stamp = rec.get(hour_col).unwrap_or_default().trim();
        let hour_end_utc = NaiveDateTime::parse_from_str(stamp, STAMP)
            .map_err(|e| Error::decode(what, format!("row {}: {stamp:?}: {e}", n + 2)))?
            .and_utc();
        if long {
            out.push(
                long_row(&rec, &lower, hour_end_utc)
                    .map_err(|m| Error::decode(what, format!("row {}: {m}", n + 2)))?,
            );
        } else {
            for (i, zone) in names.iter().enumerate().filter(|(i, _)| *i != hour_col) {
                match rec.get(i).map(str::trim) {
                    None | Some("") => {}
                    Some(cell) => out.push(ZoneLoad {
                        hour_end_utc,
                        balancing_area: None,
                        zone: zone.clone(),
                        forecast_area_type: None,
                        mw: cell.parse().map_err(|e| {
                            Error::decode(what, format!("row {} {zone}: {cell:?}: {e}", n + 2))
                        })?,
                    }),
                }
            }
        }
    }
    Ok(out)
}

fn long_row(
    rec: &csv::StringRecord,
    cols: &HashMap<String, usize>,
    hour_end_utc: DateTime<Utc>,
) -> std::result::Result<ZoneLoad, String> {
    let get = |name: &str| {
        cols.get(name)
            .and_then(|i| rec.get(*i))
            .map(str::trim)
            .ok_or_else(|| format!("missing {name}"))
    };
    Ok(ZoneLoad {
        hour_end_utc,
        balancing_area: Some(get("balancing area name")?.to_owned()),
        zone: get("control zone name")?.to_owned(),
        forecast_area_type: get("forecast area type").ok().map(str::to_owned),
        mw: get("load mw")?.parse().map_err(|e| format!("load: {e}"))?,
    })
}

/// The posted path of the daily load file for `date`.
pub fn daily_load_path(date: NaiveDate) -> String {
    format!(
        "/{}/DAILY_HOURLY_LOAD-{}.csv",
        date.format("%Y"),
        date.format("%Y%m%d")
    )
}

/// The posted path of the monthly load file.
pub fn monthly_load_path(year: i32, month: u32) -> String {
    format!("/{year:04}/HOURLY_LOAD-{year:04}{month:02}.csv")
}

impl Client {
    /// Hourly load by SPP control zone for one operating day (posted the next day).
    pub async fn hourly_load_daily(&self, date: NaiveDate) -> Result<Vec<ZoneLoad>> {
        parse_hourly_load(&self.download(&HOURLY_LOAD, &daily_load_path(date)).await?)
    }

    /// Hourly load by balancing area and control zone for a whole month (posted after month end).
    pub async fn hourly_load_monthly(&self, year: i32, month: u32) -> Result<Vec<ZoneLoad>> {
        parse_hourly_load(
            &self
                .download(&HOURLY_LOAD, &monthly_load_path(year, month))
                .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_wide_daily_layout() {
        let csv = "MarketHour, CSWS, EDE, WR\n01/05/2026 07:00:00,6437.384,802.897,\n01/05/2026 08:00:00,4999.111,531.832,1.5\n";
        let rows = parse_hourly_load(csv.as_bytes()).expect("wide");
        assert_eq!(rows.len(), 5, "the blank cell is skipped, not zeroed");
        assert_eq!(rows[0].zone, "CSWS");
        assert_eq!(
            rows[0].hour_end_utc.to_rfc3339(),
            "2026-01-05T07:00:00+00:00"
        );
        assert_eq!(rows[0].balancing_area, None);
        assert_eq!(
            rows.last().map(|r| (r.zone.as_str(), r.mw)),
            Some(("WR", 1.5))
        );
    }

    #[test]
    fn parses_the_long_monthly_layout() {
        let csv = "Market Hour,Balancing Area Name,Control Zone Name,Forecast Area Type,Load MW\n10/01/2026 06:00:00,SPP,CSWS,CF,6727.006\n11/01/2026 05:00:00,SWPW,WACM,NC,262.641\n";
        let rows = parse_hourly_load(csv.as_bytes()).expect("long");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].balancing_area.as_deref(), Some("SWPW"));
        assert_eq!(rows[1].forecast_area_type.as_deref(), Some("NC"));
        assert_eq!(rows[1].mw, 262.641);
    }

    #[test]
    fn rejects_non_load_files() {
        assert!(parse_hourly_load(b"a,b\n1,2\n").is_err());
        assert!(parse_hourly_load(b"MarketHour,CSWS\nnot-a-time,1\n").is_err());
    }

    #[test]
    fn paths() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 5).expect("date");
        assert_eq!(daily_load_path(d), "/2026/DAILY_HOURLY_LOAD-20260105.csv");
        assert_eq!(monthly_load_path(2026, 10), "/2026/HOURLY_LOAD-202610.csv");
    }
}

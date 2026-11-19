//! Day-ahead and real-time LMPs by settlement location.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::browser::{Entry, DA_LMP_BY_LOCATION, RTBM_LMP_BY_LOCATION};
use crate::error::{Error, Result};
use crate::Client;

/// SPP's two trading hubs, as named in the LMP files.
pub const SPP_HUBS: [&str; 2] = ["SPPNORTH_HUB", "SPPSOUTH_HUB"];

/// Timestamp format of the LMP files (`11/18/2026 01:00:00`).
const STAMP: &str = "%m/%d/%Y %H:%M:%S";

/// One settlement location's price for one interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationalPrice {
    /// Interval end in Central Prevailing Time, as printed in the file.
    pub interval_end: NaiveDateTime,
    /// Interval end in UTC (from the file's GMT column).
    pub interval_end_utc: DateTime<Utc>,
    /// Balancing authority area: `SPP` (the Integrated Marketplace) or `SWPW` (Western Energy Imbalance Service).
    pub balancing_authority: String,
    /// Settlement location, e.g. `SPPNORTH_HUB` or `CRSP_HUB`.
    pub settlement_location: String,
    /// The pricing node the location settles at.
    pub pnode: String,
    /// Locational marginal price, $/MWh.
    pub lmp: f64,
    /// Marginal loss component, $/MWh.
    pub mlc: Option<f64>,
    /// Marginal congestion component, $/MWh.
    pub mcc: Option<f64>,
    /// Marginal energy component, $/MWh.
    pub mec: Option<f64>,
}

impl LocationalPrice {
    /// True for settlement locations whose name ends in `_HUB` (SPP's hubs and the WEIS hubs).
    pub fn is_hub(&self) -> bool {
        self.settlement_location.ends_with("_HUB")
    }
}

/// The prices in `prices` for `settlement_location`, in file order.
pub fn at_location<'a>(
    prices: &'a [LocationalPrice],
    settlement_location: &'a str,
) -> impl Iterator<Item = &'a LocationalPrice> {
    prices
        .iter()
        .filter(move |p| p.settlement_location == settlement_location)
}

/// Parse an LMP file in either layout SPP publishes (by-day, by-interval or the daily real-time roll-up).
///
/// Columns are found by header name, so their order does not matter.
pub fn parse_lmp_csv(bytes: &[u8]) -> Result<Vec<LocationalPrice>> {
    let what = "SPP LMP file";
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| Error::decode(what, e))?
        .clone();
    let index: HashMap<String, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
        .collect();
    let find = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| index.get(*n).copied())
            .ok_or_else(|| Error::decode(what, format!("missing column {}", names.join(" / "))))
    };
    let cols = Columns {
        interval: find(&["interval"])?,
        gmt: find(&["gmtintervalend", "gmt interval"])?,
        baa: find(&["baa"])?,
        location: find(&["settlement location", "settlement location name"])?,
        pnode: find(&["pnode", "pnode name"])?,
        lmp: find(&["lmp"])?,
        mlc: index.get("mlc").copied(),
        mcc: index.get("mcc").copied(),
        mec: index.get("mec").copied(),
    };
    reader
        .records()
        .enumerate()
        .map(|(n, rec)| {
            let rec = rec.map_err(|e| Error::decode(what, format!("row {}: {e}", n + 2)))?;
            cols.row(&rec)
                .map_err(|m| Error::decode(what, format!("row {}: {m}", n + 2)))
        })
        .collect()
}

struct Columns {
    interval: usize,
    gmt: usize,
    baa: usize,
    location: usize,
    pnode: usize,
    lmp: usize,
    mlc: Option<usize>,
    mcc: Option<usize>,
    mec: Option<usize>,
}

impl Columns {
    fn row(&self, rec: &csv::StringRecord) -> std::result::Result<LocationalPrice, String> {
        let text = |i: usize| {
            rec.get(i)
                .map(str::trim)
                .ok_or_else(|| format!("short row (column {i})"))
        };
        let stamp = |i: usize| {
            text(i).and_then(|s| {
                NaiveDateTime::parse_from_str(s, STAMP).map_err(|e| format!("{s:?}: {e}"))
            })
        };
        let number =
            |i: usize| text(i).and_then(|s| s.parse::<f64>().map_err(|e| format!("{s:?}: {e}")));
        let optional = |i: Option<usize>| -> std::result::Result<Option<f64>, String> {
            match i.and_then(|i| rec.get(i)).map(str::trim) {
                None | Some("") => Ok(None),
                Some(s) => s
                    .parse::<f64>()
                    .map(Some)
                    .map_err(|e| format!("{s:?}: {e}")),
            }
        };
        Ok(LocationalPrice {
            interval_end: stamp(self.interval)?,
            interval_end_utc: stamp(self.gmt)?.and_utc(),
            balancing_authority: text(self.baa)?.to_owned(),
            settlement_location: text(self.location)?.to_owned(),
            pnode: text(self.pnode)?.to_owned(),
            lmp: number(self.lmp)?,
            mlc: optional(self.mlc)?,
            mcc: optional(self.mcc)?,
            mec: optional(self.mec)?,
        })
    }
}

/// The posted path of the day-ahead file for operating day `date`.
pub fn da_lmp_path(date: NaiveDate) -> String {
    format!(
        "/{}/{}/By_Day/DA-LMP-SL-{}0100.csv",
        date.format("%Y"),
        date.format("%m"),
        date.format("%Y%m%d")
    )
}

/// The posted path of the real-time five-minute file for the interval ending at `interval_end` (Central time).
pub fn rt_lmp_path(interval_end: NaiveDateTime) -> String {
    format!(
        "/{}/{}/By_Interval/{}/RTBM-LMP-SL-{}.csv",
        interval_end.format("%Y"),
        interval_end.format("%m"),
        interval_end.format("%d"),
        interval_end.format("%Y%m%d%H%M")
    )
}

/// The posted path of the real-time daily roll-up for `date` (large; posted a few days late).
pub fn rt_lmp_daily_path(date: NaiveDate) -> String {
    format!(
        "/{}/{}/By_Day/RTBM-LMP-DAILY-SL-{}.csv",
        date.format("%Y"),
        date.format("%m"),
        date.format("%Y%m%d")
    )
}

impl Client {
    /// Day-ahead hourly LMPs at every settlement location for operating day `date`.
    ///
    /// SPP posts the file early in the afternoon of the day before. A 404 ([`Error::is_not_found`])
    /// means it is not posted yet.
    pub async fn da_lmp_by_location(&self, date: NaiveDate) -> Result<Vec<LocationalPrice>> {
        let bytes = self
            .download(&DA_LMP_BY_LOCATION, &da_lmp_path(date))
            .await?;
        parse_lmp_csv(&bytes)
    }

    /// The newest day-ahead file posted, with its listing entry.
    pub async fn latest_da_lmp_by_location(&self) -> Result<(Entry, Vec<LocationalPrice>)> {
        let entry = self.newest_file(&DA_LMP_BY_LOCATION, "By_Day").await?;
        let bytes = self.download(&DA_LMP_BY_LOCATION, &entry.path).await?;
        Ok((entry, parse_lmp_csv(&bytes)?))
    }

    /// Real-time five-minute LMPs at every settlement location for the interval ending at
    /// `interval_end` (Central Prevailing Time, on a five-minute boundary).
    pub async fn rt_lmp_by_location(
        &self,
        interval_end: NaiveDateTime,
    ) -> Result<Vec<LocationalPrice>> {
        let bytes = self
            .download(&RTBM_LMP_BY_LOCATION, &rt_lmp_path(interval_end))
            .await?;
        parse_lmp_csv(&bytes)
    }

    /// The newest real-time five-minute file posted, with its listing entry.
    pub async fn latest_rt_lmp_by_location(&self) -> Result<(Entry, Vec<LocationalPrice>)> {
        let entry = self
            .newest_file(&RTBM_LMP_BY_LOCATION, "By_Interval")
            .await?;
        let bytes = self.download(&RTBM_LMP_BY_LOCATION, &entry.path).await?;
        Ok((entry, parse_lmp_csv(&bytes)?))
    }

    /// Every real-time five-minute LMP for `date` in one roll-up file (about 50 MB).
    pub async fn rt_lmp_daily(&self, date: NaiveDate) -> Result<Vec<LocationalPrice>> {
        let bytes = self
            .download(&RTBM_LMP_BY_LOCATION, &rt_lmp_daily_path(date))
            .await?;
        parse_lmp_csv(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DA: &str = "Interval,GMTIntervalEnd,BAA,Settlement Location,Pnode,LMP,MLC,MCC,MEC\n11/18/2026 01:00:00,11/18/2026 07:00:00,SWPW,CRSP_HUB,CRSP_HUB_H,17.4286,-0.06,0.0009,9.9626\n11/18/2026 01:00:00,11/18/2026 07:00:00,SPP,SPPNORTH_HUB,SPPNORTH_H,-6.4036,0.0176,-3.8128,-2.1220\n";
    const RT: &str = "Interval,GMTIntervalEnd,Settlement Location,Pnode,LMP,MLC,MCC,MEC,BAA\n11/17/2026 23:55:00,11/18/2026 05:55:00,AEC,SOUC,6.7062,-0.2184,13.4957,-6.0504,SPP\n";
    const DAILY: &str = "Interval,GMT Interval,Settlement Location Name,PNODE Name,LMP,MLC,MCC,MEC,BAA\n11/12/2026 00:05:00,11/12/2026 06:05:00,AEC,SOUC,32.3407,0.0073,0.7989,27.3299,SPP\n";

    #[test]
    fn parses_the_day_ahead_layout() {
        let rows = parse_lmp_csv(DA.as_bytes()).expect("parse");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].settlement_location, "CRSP_HUB");
        assert_eq!(rows[0].balancing_authority, "SWPW");
        assert_eq!(rows[0].lmp, 17.4286);
        assert_eq!(
            rows[0].interval_end_utc.to_rfc3339(),
            "2026-11-18T07:00:00+00:00"
        );
        assert!(rows.iter().all(LocationalPrice::is_hub));
        assert_eq!(at_location(&rows, "SPPNORTH_HUB").count(), 1);
    }

    #[test]
    fn column_order_is_irrelevant_between_da_and_rt_layouts() {
        let rt = parse_lmp_csv(RT.as_bytes()).expect("rt");
        assert_eq!(
            (
                rt[0].settlement_location.as_str(),
                rt[0].balancing_authority.as_str()
            ),
            ("AEC", "SPP")
        );
        assert_eq!(rt[0].mec, Some(-6.0504));
        assert!(!rt[0].is_hub());
    }

    #[test]
    fn the_daily_rollup_header_aliases_work() {
        let rows = parse_lmp_csv(DAILY.as_bytes()).expect("daily");
        assert_eq!(rows[0].pnode, "SOUC");
        assert_eq!(rows[0].interval_end.to_string(), "2026-11-12 00:05:00");
    }

    #[test]
    fn bad_files_are_decode_errors() {
        assert!(parse_lmp_csv(b"<html>login</html>").is_err());
        let bad = "Interval,GMTIntervalEnd,BAA,Settlement Location,Pnode,LMP\n11/18/2026 01:00:00,11/18/2026 07:00:00,SPP,A,B,xyz\n";
        let err = parse_lmp_csv(bad.as_bytes()).expect_err("bad number");
        assert!(err.to_string().contains("row 2"), "{err}");
    }

    #[test]
    fn paths_follow_the_portal_layout() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 17).expect("date");
        assert_eq!(da_lmp_path(d), "/2026/11/By_Day/DA-LMP-SL-202611170100.csv");
        assert_eq!(
            rt_lmp_daily_path(d),
            "/2026/11/By_Day/RTBM-LMP-DAILY-SL-20261117.csv"
        );
        let t = d.and_hms_opt(23, 55, 0).expect("time");
        assert_eq!(
            rt_lmp_path(t),
            "/2026/11/By_Interval/17/RTBM-LMP-SL-202611172355.csv"
        );
    }
}

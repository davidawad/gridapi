//! Generation mix by fuel (five-minute, with system load).

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::browser::GENERATION_MIX;
use crate::error::{Error, Result};
use crate::Client;

/// Timestamp format of the generation mix files (`2026-11-18T04:20:00Z`).
const STAMP: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Which balancing authority area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BalancingArea {
    /// The SPP Integrated Marketplace (SPP East).
    Spp,
    /// The Western Energy Imbalance Service (SWPW, SPP West).
    Swpw,
}

impl BalancingArea {
    fn code(self) -> &'static str {
        match self {
            Self::Spp => "SPP",
            Self::Swpw => "SWPW",
        }
    }
}

/// How far back the generation mix file reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GenMixWindow {
    /// The last two hours at five-minute resolution (a few kilobytes).
    TwoHour,
    /// Year to date (about 15 MB).
    YearToDate,
    /// The rolling 365 days (about 18 MB).
    Rolling365,
}

impl GenMixWindow {
    /// The posted path for `area`.
    pub fn path(self, area: BalancingArea) -> String {
        let code = area.code();
        let stem = match self {
            Self::TwoHour => "GenMix2Hour",
            Self::YearToDate => "GenMixYTD",
            Self::Rolling365 => "GenMix365",
        };
        format!("/{code}/{stem}_{code}.csv")
    }
}

/// One fuel's output in one interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuelOutput {
    /// Fuel name, e.g. `Coal`, `Natural Gas`, `Wind`, `Energy Storage`.
    pub fuel: String,
    /// Output cleared in the market, MW.
    pub market_mw: Option<f64>,
    /// Self-scheduled output, MW.
    pub self_mw: Option<f64>,
}

impl FuelOutput {
    /// Market plus self-scheduled output, counting a missing part as zero.
    pub fn total_mw(&self) -> f64 {
        self.market_mw.unwrap_or(0.0) + self.self_mw.unwrap_or(0.0)
    }
}

/// Generation by fuel for one five-minute interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenMixSample {
    /// The interval, in UTC.
    pub interval_utc: DateTime<Utc>,
    /// Balancing authority area code as published.
    pub balancing_area: String,
    /// One entry per fuel, in file column order.
    pub fuels: Vec<FuelOutput>,
    /// System load, MW.
    pub load_mw: Option<f64>,
}

impl GenMixSample {
    /// Output of the fuel called `name`, if the file has it.
    pub fn fuel(&self, name: &str) -> Option<&FuelOutput> {
        self.fuels.iter().find(|f| f.fuel == name)
    }

    /// Total generation across all fuels, MW.
    pub fn total_mw(&self) -> f64 {
        self.fuels.iter().map(FuelOutput::total_mw).sum()
    }
}

/// Parse a `GenMix*.csv` file.
pub fn parse_gen_mix(bytes: &[u8]) -> Result<Vec<GenMixSample>> {
    let what = "SPP generation mix file";
    let mut reader = csv::ReaderBuilder::new().from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| Error::decode(what, e))?
        .clone();
    let find = |name: &str| {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::decode(what, format!("missing column {name}")))
    };
    let (i_time, i_baa) = (find("GMT MKT Interval")?, find("BAA")?);
    let i_load = headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case("load"));
    // Fuel columns come as "<Fuel> Market" / "<Fuel> Self"; remember each fuel's two column indexes.
    let mut fuel_cols: Vec<(String, Option<usize>, Option<usize>)> = Vec::new();
    for (i, h) in headers.iter().enumerate() {
        let h = h.trim();
        let (fuel, is_market) = match (h.strip_suffix(" Market"), h.strip_suffix(" Self")) {
            (Some(f), _) => (f, true),
            (_, Some(f)) => (f, false),
            _ => continue,
        };
        match fuel_cols.iter_mut().find(|(name, _, _)| name == fuel) {
            Some((_, market, own)) => *(if is_market { market } else { own }) = Some(i),
            None => fuel_cols.push((
                fuel.to_owned(),
                is_market.then_some(i),
                (!is_market).then_some(i),
            )),
        }
    }
    reader
        .records()
        .enumerate()
        .map(|(n, rec)| {
            let row = n + 2;
            let rec = rec.map_err(|e| Error::decode(what, format!("row {row}: {e}")))?;
            let cell = |i: usize| rec.get(i).map(str::trim).filter(|s| !s.is_empty());
            let number = |i: Option<usize>| -> Result<Option<f64>> {
                i.and_then(cell)
                    .map(|s| {
                        s.parse::<f64>()
                            .map_err(|e| Error::decode(what, format!("row {row}: {s:?}: {e}")))
                    })
                    .transpose()
            };
            let stamp = cell(i_time).unwrap_or_default();
            Ok(GenMixSample {
                interval_utc: NaiveDateTime::parse_from_str(stamp, STAMP)
                    .map_err(|e| Error::decode(what, format!("row {row}: {stamp:?}: {e}")))?
                    .and_utc(),
                balancing_area: cell(i_baa).unwrap_or_default().to_owned(),
                fuels: fuel_cols
                    .iter()
                    .map(|(fuel, market, own)| {
                        Ok(FuelOutput {
                            fuel: fuel.clone(),
                            market_mw: number(*market)?,
                            self_mw: number(*own)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
                load_mw: number(i_load)?,
            })
        })
        .collect()
}

impl Client {
    /// Generation mix and system load for `area` over `window`.
    pub async fn generation_mix(
        &self,
        area: BalancingArea,
        window: GenMixWindow,
    ) -> Result<Vec<GenMixSample>> {
        parse_gen_mix(&self.download(&GENERATION_MIX, &window.path(area)).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "GMT MKT Interval,BAA,Coal Market,Coal Self,Wind Market,Wind Self,Load\n2026-11-18T04:20:00Z,SPP,1543.49,1457.34,0.00,15275.27,23279.389\n2026-11-18T04:25:00Z,SPP,1364.63,,0.00,29170.01,\n";

    #[test]
    fn parses_fuels_and_load() {
        let rows = parse_gen_mix(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].interval_utc.to_rfc3339(),
            "2026-11-18T04:20:00+00:00"
        );
        let coal = rows[0]
            .fuel("Coal")
            .map(FuelOutput::total_mw)
            .expect("coal");
        assert!((coal - 3000.83).abs() < 1e-9, "coal total {coal}");
        assert_eq!(rows[0].fuel("Wind").and_then(|f| f.self_mw), Some(15275.27));
        assert_eq!(rows[0].load_mw, Some(23279.389));
        assert!((rows[0].total_mw() - 18276.1).abs() < 1e-6);
    }

    #[test]
    fn blanks_stay_missing_instead_of_becoming_zero() {
        let rows = parse_gen_mix(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(rows[1].fuel("Coal").map(|f| f.self_mw), Some(None));
        assert_eq!(rows[1].load_mw, None);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse_gen_mix(b"a,b\n1,2\n").is_err());
        assert!(parse_gen_mix(b"GMT MKT Interval,BAA\nnope,SPP\n").is_err());
    }

    #[test]
    fn window_paths() {
        assert_eq!(
            GenMixWindow::TwoHour.path(BalancingArea::Spp),
            "/SPP/GenMix2Hour_SPP.csv"
        );
        assert_eq!(
            GenMixWindow::Rolling365.path(BalancingArea::Swpw),
            "/SWPW/GenMix365_SWPW.csv"
        );
    }
}

//! Live generation fuel mix from ERCOT's public dashboard feed.
//!
//! The Public API has no fuel-mix report. ERCOT's website dashboard publishes the
//! 5-minute fuel mix as an unauthenticated JSON feed, which this module parses. The
//! feed is not part of the documented API and may change without notice.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset};
use serde::Deserialize;

use crate::error::{Error, Result};

/// Generation per fuel at one instant.
#[derive(Debug, Clone, PartialEq)]
pub struct FuelMixSnapshot {
    /// Interval timestamp with its UTC offset (ERCOT publishes `-0600` / `-0500`).
    pub at: DateTime<FixedOffset>,
    /// Generation in MW keyed by fuel name (`Natural Gas`, `Wind`, `Solar`, ...).
    ///
    /// Storage fuels report negative values while charging.
    pub generation_mw: BTreeMap<String, f64>,
}

impl FuelMixSnapshot {
    /// Sum of the base fuels, excluding the charging/discharging split rows.
    ///
    /// `Power Storage` already nets charge and discharge, so those two detail rows
    /// are skipped to avoid double counting.
    #[must_use]
    pub fn net_total_mw(&self) -> f64 {
        self.generation_mw
            .iter()
            .filter(|(fuel, _)| !fuel.starts_with("Power Storage "))
            .map(|(_, mw)| mw)
            .sum()
    }
}

/// The dashboard fuel-mix feed: recent 5-minute snapshots plus installed capacity.
#[derive(Debug, Clone, PartialEq)]
pub struct FuelMix {
    /// Feed-reported time of the last update.
    pub last_updated: String,
    /// Installed capacity in MW by fuel for the current month.
    pub monthly_capacity_mw: BTreeMap<String, f64>,
    /// Snapshots in chronological order.
    pub snapshots: Vec<FuelMixSnapshot>,
}

#[derive(Debug, Deserialize)]
struct Wire {
    #[serde(rename = "lastUpdated")]
    last_updated: String,
    #[serde(rename = "monthlyCapacity", default)]
    monthly_capacity: BTreeMap<String, f64>,
    data: BTreeMap<String, BTreeMap<String, BTreeMap<String, Cell>>>,
}

#[derive(Debug, Deserialize)]
struct Cell {
    gen: Option<f64>,
}

impl FuelMix {
    /// Parses the dashboard JSON body.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] when the body is not the expected shape or a timestamp is malformed.
    pub fn from_json(body: &[u8]) -> Result<Self> {
        let wire: Wire = serde_json::from_slice(body).map_err(|e| Error::decode("fuel mix", e))?;
        let mut snapshots = wire
            .data
            .into_values()
            .flatten()
            .map(|(stamp, fuels)| {
                let at = DateTime::parse_from_str(&stamp, "%Y-%m-%d %H:%M:%S%z")
                    .map_err(|e| Error::decode("fuel mix timestamp", format!("{stamp:?}: {e}")))?;
                let generation_mw = fuels
                    .into_iter()
                    .filter_map(|(fuel, cell)| cell.gen.map(|mw| (fuel, mw)))
                    .collect();
                Ok(FuelMixSnapshot { at, generation_mw })
            })
            .collect::<Result<Vec<_>>>()?;
        snapshots.sort_by_key(|s| s.at);
        Ok(Self {
            last_updated: wire.last_updated,
            monthly_capacity_mw: wire.monthly_capacity,
            snapshots,
        })
    }

    /// The most recent snapshot.
    #[must_use]
    pub fn latest(&self) -> Option<&FuelMixSnapshot> {
        self.snapshots.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FuelMix {
        FuelMix::from_json(include_bytes!("../tests/fixtures/fuel_mix.json")).expect("fixture")
    }

    #[test]
    fn parses_snapshots_in_time_order_with_offsets() {
        let mix = fixture();
        assert_eq!(mix.snapshots.len(), 2);
        assert!(mix.snapshots[0].at < mix.snapshots[1].at);
        assert_eq!(mix.snapshots[0].at.offset().local_minus_utc(), -6 * 3600);
        assert_eq!(mix.monthly_capacity_mw.get("Natural Gas"), Some(&46_858.0));
    }

    #[test]
    fn latest_snapshot_carries_per_fuel_generation() {
        let mix = fixture();
        let last = mix.latest().expect("non-empty");
        assert!(last.generation_mw["Natural Gas"] > 20_000.0);
        assert!(last.generation_mw["Wind"] > 0.0);
        assert!(last.generation_mw.contains_key("Nuclear"));
    }

    #[test]
    fn net_total_skips_storage_detail_rows() {
        let mix = fixture();
        let snap = &mix.snapshots[0];
        let manual: f64 = [
            "Coal and Lignite",
            "Hydro",
            "Nuclear",
            "Other",
            "Power Storage",
            "Solar",
            "Wind",
            "Natural Gas",
        ]
        .iter()
        .map(|f| snap.generation_mw[*f])
        .sum();
        assert!((snap.net_total_mw() - manual).abs() < 1e-6);
    }

    #[test]
    fn bad_timestamp_is_a_decode_error() {
        let body =
            br#"{"lastUpdated":"x","data":{"2026-11-17":{"yesterday":{"Wind":{"gen":1.0}}}}}"#;
        assert!(matches!(
            FuelMix::from_json(body),
            Err(Error::Decode { .. })
        ));
    }
}

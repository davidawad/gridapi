//! Typed rows for the ISO Express CSV reports, and the decoders that produce them.
//!
//! Every timestamp is a [`DateTime<FixedOffset>`] carrying the Eastern UTC offset
//! in force at that instant (-05:00 EST or -04:00 EDT), so the repeated hour on
//! the fall-back day stays unambiguous.

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::Serialize;

use crate::{
    error::Error,
    report::{Report, Row},
    time::{hour_ending_start, on_day, parse_date, parse_date_time, EasternClock},
};

/// The Internal Hub, the usual reference price point for the region.
pub const INTERNAL_HUB_ID: u32 = 4000;
/// The Internal Hub's location name.
pub const INTERNAL_HUB_NAME: &str = ".H.INTERNAL_HUB";

/// Load zone `.Z.MAINE`.
pub const ZONE_MAINE: u32 = 4001;
/// Load zone `.Z.NEWHAMPSHIRE`.
pub const ZONE_NEW_HAMPSHIRE: u32 = 4002;
/// Load zone `.Z.VERMONT`.
pub const ZONE_VERMONT: u32 = 4003;
/// Load zone `.Z.CONNECTICUT`.
pub const ZONE_CONNECTICUT: u32 = 4004;
/// Load zone `.Z.RHODEISLAND`.
pub const ZONE_RHODE_ISLAND: u32 = 4005;
/// Load zone `.Z.SEMASS` (southeast Massachusetts).
pub const ZONE_SEMASS: u32 = 4006;
/// Load zone `.Z.WCMASS` (western and central Massachusetts).
pub const ZONE_WCMASS: u32 = 4007;
/// Load zone `.Z.NEMASSBOST` (northeast Massachusetts and Boston).
pub const ZONE_NEMASS_BOSTON: u32 = 4008;

/// Which reported rows to keep. Filtering happens while decoding, so a day of
/// five-minute prices (about 350,000 rows) can be narrowed to one hub without
/// holding the rest in memory.
///
/// An empty filter keeps every row. Otherwise a row is kept when its location id
/// is in `ids` or its location name is in `names`. A report that carries only one
/// of the two keys (the preliminary hourly real-time file has names only; the
/// five-minute files have ids only) can only match on that key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocationFilter {
    ids: BTreeSet<u32>,
    names: BTreeSet<String>,
}

impl LocationFilter {
    /// A filter that keeps everything.
    #[must_use]
    pub fn all() -> Self {
        Self::default()
    }

    /// A filter for one location id, e.g. [`INTERNAL_HUB_ID`].
    #[must_use]
    pub fn id(id: u32) -> Self {
        Self::all().with_id(id)
    }

    /// Also keep this location id.
    #[must_use]
    pub fn with_id(mut self, id: u32) -> Self {
        self.ids.insert(id);
        self
    }

    /// Also keep this location name, e.g. [`INTERNAL_HUB_NAME`].
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.names.insert(name.into());
        self
    }

    /// Whether the filter keeps everything.
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.ids.is_empty() && self.names.is_empty()
    }

    pub(crate) fn keeps(&self, id: Option<u32>, name: Option<&str>) -> bool {
        self.is_all()
            || id.is_some_and(|i| self.ids.contains(&i))
            || name.is_some_and(|n| self.names.contains(n))
    }
}

/// Day-ahead or real-time energy price for one location and hour.
///
/// `lmp = energy + congestion + loss`, all in $/`MWh`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyLmp {
    /// Start of the hour, in Eastern time.
    pub interval_start: DateTime<FixedOffset>,
    /// The published hour label: `01`..`24`, or `02X` for the repeated hour.
    pub hour_ending: String,
    /// Location id, e.g. 4000 for the Internal Hub. Absent in the preliminary
    /// hourly real-time file, which names locations only.
    pub location_id: Option<u32>,
    /// Location name, e.g. `.H.INTERNAL_HUB`. Absent in the "current" feeds,
    /// which carry ids only.
    pub location_name: Option<String>,
    /// `HUB`, `LOAD ZONE`, `NETWORK NODE`, ... Only the day-ahead and final
    /// real-time files carry it.
    pub location_type: Option<String>,
    /// Locational marginal price in $/`MWh`.
    pub lmp: f64,
    /// Energy component in $/`MWh`.
    pub energy: f64,
    /// Congestion component in $/`MWh`.
    pub congestion: f64,
    /// Marginal loss component in $/`MWh`.
    pub loss: f64,
}

/// Five-minute real-time energy price for one location.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiveMinuteLmp {
    /// Start of the interval, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Location id, e.g. 4000 for the Internal Hub.
    pub location_id: u32,
    /// Locational marginal price in $/`MWh`.
    pub lmp: f64,
    /// Energy component in $/`MWh`.
    pub energy: f64,
    /// Congestion component in $/`MWh`.
    pub congestion: f64,
    /// Loss component in $/`MWh`.
    pub loss: f64,
}

/// One dispatch fuel-mix sample. Samples arrive about every five minutes at
/// irregular seconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FuelMix {
    /// When the sample was taken, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Fuel category, e.g. `Natural Gas`, `Solar`.
    pub fuel_category: String,
    /// Fuel category rollup, e.g. `Renewables`.
    pub rollup: String,
    /// Generation in MW.
    pub gen_mw: f64,
    /// Whether this fuel was on the margin.
    pub marginal: bool,
}

/// Hourly system energy: real-time total load, or day-ahead cleared demand,
/// depending on the request that produced it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HourlyDemand {
    /// Start of the hour, in Eastern time.
    pub interval_start: DateTime<FixedOffset>,
    /// The published hour label: `01`..`24`, or `02X` for the repeated hour.
    pub hour_ending: String,
    /// Energy for the hour in `MWh`.
    pub mwh: f64,
}

/// Five-minute real-time system load.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FiveMinuteSystemLoad {
    /// Start of the interval, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Total load in MW.
    pub total_load_mw: f64,
    /// Native load in MW.
    pub native_load_mw: f64,
    /// Asset-related load in MW.
    pub asset_related_load_mw: f64,
    /// Total load including estimated behind-the-meter solar, when published.
    pub total_load_with_solar_mw: Option<f64>,
    /// Native load including estimated behind-the-meter solar, when published.
    pub native_load_with_solar_mw: Option<f64>,
}

/// Five-minute estimated load for one load zone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZonalLoad {
    /// Start of the interval, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Load zone id, 4001..=4008.
    pub zone_id: u32,
    /// Load zone name, e.g. `.Z.CONNECTICUT`.
    pub zone_name: String,
    /// Estimated native load in MW.
    pub native_load_mw: f64,
    /// Estimated behind-the-meter solar in MW.
    pub behind_the_meter_solar_mw: f64,
}

/// Five-minute regulation clearing prices, in $/`MWh`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegulationPrice {
    /// When the price applies, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Regulation service clearing price.
    pub service_price: f64,
    /// Regulation capacity clearing price.
    pub capacity_price: f64,
}

/// Hourly operating-reserve requirement for one reserve zone.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReserveRequirement {
    /// Start of the hour, in Eastern time.
    pub interval_start: DateTime<FixedOffset>,
    /// The published hour label: `01`..`24`, or `02X` for the repeated hour.
    pub hour_ending: String,
    /// Reserve zone id, e.g. 7000 for the system.
    pub reserve_zone_id: u32,
    /// Ten-minute spinning requirement in MW.
    pub ten_minute_spinning_mw: f64,
    /// Ten-minute requirement in MW.
    pub ten_minute_mw: f64,
    /// Total requirement in MW.
    pub total_mw: f64,
}

/// Decoded rows plus the number of data rows the file held before filtering.
#[derive(Debug)]
pub(crate) struct Decoded<T> {
    pub(crate) rows: Vec<T>,
    pub(crate) file_rows: usize,
}

/// Decodes one report body.
pub(crate) type Decoder<T> = fn(&[u8], NaiveDate, &LocationFilter) -> Result<Decoded<T>, Error>;

/// Decode every row, keeping those the closure maps to `Some`.
fn decode_with<T>(
    report: &Report,
    mut per_row: impl FnMut(Row<'_>) -> Result<Option<T>, Error>,
) -> Result<Decoded<T>, Error> {
    let rows = report
        .rows()
        .map(&mut per_row)
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Decoded {
        rows,
        file_rows: report.len(),
    })
}

fn optional_id(row: &Row<'_>, column: Option<usize>) -> Result<Option<u32>, Error> {
    column.map(|c| row.uint(c)).transpose()
}

fn optional_text(row: &Row<'_>, column: Option<usize>) -> Option<String> {
    column.map(|c| row.text(c).to_owned())
}

const DATE: &[&str] = &["date", "local date"];
const HOUR: &[&str] = &["hour ending", "local time"];

fn day_and_hour(
    row: &Row<'_>,
    date: usize,
    hour: usize,
) -> Result<(NaiveDate, DateTime<FixedOffset>), Error> {
    let day = parse_date(row.report_name(), row.text(date))?;
    Ok((day, hour_ending_start(day, row.text(hour))?))
}

/// Hourly LMP files: day-ahead and final real-time (id, name and type), the
/// preliminary real-time file (name only), and the "current" feeds (id only).
pub(crate) fn hourly_lmp(
    bytes: &[u8],
    _day: NaiveDate,
    filter: &LocationFilter,
) -> Result<Decoded<HourlyLmp>, Error> {
    let report = Report::parse("hourly lmp", bytes)?;
    let (date, hour) = (report.column(DATE)?, report.column(HOUR)?);
    let id = report.find(&["location id"]);
    let name = report.find(&["location name", "location"]);
    let kind = report.find(&["location type"]);
    let lmp = report.column(&["locational marginal price", "lmp"])?;
    let energy = report.column(&["energy component", "energy"])?;
    let congestion = report.column(&["congestion component", "congestion"])?;
    let loss = report.column(&["marginal loss component", "loss component", "loss"])?;
    decode_with(&report, |row| {
        let location_id = optional_id(&row, id)?;
        let location_name = optional_text(&row, name);
        if !filter.keeps(location_id, location_name.as_deref()) {
            return Ok(None);
        }
        let (_, interval_start) = day_and_hour(&row, date, hour)?;
        Ok(Some(HourlyLmp {
            interval_start,
            hour_ending: row.text(hour).to_owned(),
            location_id,
            location_name,
            location_type: optional_text(&row, kind),
            lmp: row.float(lmp)?,
            energy: row.float(energy)?,
            congestion: row.float(congestion)?,
            loss: row.float(loss)?,
        }))
    })
}

/// Five-minute LMP files. The `Local Time` cell is a bare time in the block files
/// (the day comes from the request) and a full date-time in the "current" feeds.
pub(crate) fn five_minute_lmp(
    bytes: &[u8],
    day: NaiveDate,
    filter: &LocationFilter,
) -> Result<Decoded<FiveMinuteLmp>, Error> {
    let report = Report::parse("five-minute lmp", bytes)?;
    let id = report.column(&["location id"])?;
    let time = report.column(&["local time"])?;
    let lmp = report.column(&["lmp"])?;
    let energy = report.column(&["energy component"])?;
    let congestion = report.column(&["congestion component"])?;
    let loss = report.column(&["loss component"])?;
    let mut clock = EasternClock::default();
    decode_with(&report, |row| {
        let location_id = row.uint(id)?;
        if !filter.keeps(Some(location_id), None) {
            return Ok(None);
        }
        let cell = row.text(time);
        let local = if cell.contains(['-', '/']) {
            parse_date_time(row.report_name(), cell)?
        } else {
            on_day(row.report_name(), day, cell)?
        };
        Ok(Some(FiveMinuteLmp {
            timestamp: clock.resolve(location_id, local)?,
            location_id,
            lmp: row.float(lmp)?,
            energy: row.float(energy)?,
            congestion: row.float(congestion)?,
            loss: row.float(loss)?,
        }))
    })
}

pub(crate) fn fuel_mix(
    bytes: &[u8],
    _day: NaiveDate,
    _filter: &LocationFilter,
) -> Result<Decoded<FuelMix>, Error> {
    let report = Report::parse("fuel mix", bytes)?;
    let (date, time) = (report.column(&["date"])?, report.column(&["time"])?);
    let fuel = report.column(&["fuel category"])?;
    let rollup = report.column(&["fuel category rollup"])?;
    let gen_mw = report.column(&["gen mw"])?;
    let marginal = report.find(&["marginal flag"]);
    let mut clock = EasternClock::default();
    decode_with(&report, |row| {
        let day = parse_date(row.report_name(), row.text(date))?;
        let local = on_day(row.report_name(), day, row.text(time))?;
        Ok(Some(FuelMix {
            timestamp: clock.resolve(row.text(fuel).to_owned(), local)?,
            fuel_category: row.text(fuel).to_owned(),
            rollup: row.text(rollup).to_owned(),
            gen_mw: row.float(gen_mw)?,
            marginal: marginal
                .is_some_and(|c| matches!(row.text(c).to_ascii_lowercase().as_str(), "yes" | "y")),
        }))
    })
}

fn hourly_demand(
    name: &str,
    bytes: &[u8],
    value_aliases: &[&str],
) -> Result<Decoded<HourlyDemand>, Error> {
    let report = Report::parse(name, bytes)?;
    let (date, hour) = (report.column(DATE)?, report.column(HOUR)?);
    let value = report.column(value_aliases)?;
    decode_with(&report, |row| {
        let (_, interval_start) = day_and_hour(&row, date, hour)?;
        Ok(Some(HourlyDemand {
            interval_start,
            hour_ending: row.text(hour).to_owned(),
            mwh: row.float(value)?,
        }))
    })
}

pub(crate) fn hourly_system_load(
    bytes: &[u8],
    _day: NaiveDate,
    _filter: &LocationFilter,
) -> Result<Decoded<HourlyDemand>, Error> {
    hourly_demand("hourly system load", bytes, &["total load"])
}

pub(crate) fn day_ahead_demand(
    bytes: &[u8],
    _day: NaiveDate,
    _filter: &LocationFilter,
) -> Result<Decoded<HourlyDemand>, Error> {
    hourly_demand("day-ahead demand", bytes, &["day-ahead cleared demand"])
}

pub(crate) fn five_minute_system_load(
    bytes: &[u8],
    _day: NaiveDate,
    _filter: &LocationFilter,
) -> Result<Decoded<FiveMinuteSystemLoad>, Error> {
    let report = Report::parse("five-minute system load", bytes)?;
    let time = report.column(&["date/time"])?;
    let total = report.column(&["total load"])?;
    let native = report.column(&["native load"])?;
    let ard = report.column(&["asset related load"])?;
    let total_solar = report.find(&["total load with estimated solar"]);
    let native_solar = report.find(&["native load with estimated solar"]);
    let mut clock = EasternClock::default();
    decode_with(&report, |row| {
        let local = parse_date_time(row.report_name(), row.text(time))?;
        Ok(Some(FiveMinuteSystemLoad {
            timestamp: clock.resolve((), local)?,
            total_load_mw: row.float(total)?,
            native_load_mw: row.float(native)?,
            asset_related_load_mw: row.float(ard)?,
            total_load_with_solar_mw: total_solar.map(|c| row.opt_float(c)).transpose()?.flatten(),
            native_load_with_solar_mw: native_solar
                .map(|c| row.opt_float(c))
                .transpose()?
                .flatten(),
        }))
    })
}

pub(crate) fn zonal_load(
    bytes: &[u8],
    _day: NaiveDate,
    filter: &LocationFilter,
) -> Result<Decoded<ZonalLoad>, Error> {
    let report = Report::parse("zonal load", bytes)?;
    let time = report.column(&["date/time"])?;
    let id = report.column(&["load zone id"])?;
    let name = report.column(&["load zone name"])?;
    let native = report.column(&["estimated native load"])?;
    let solar = report.column(&["estimated behind-the-meter solar"])?;
    let mut clock = EasternClock::default();
    decode_with(&report, |row| {
        let zone_id = row.uint(id)?;
        if !filter.keeps(Some(zone_id), Some(row.text(name))) {
            return Ok(None);
        }
        let local = parse_date_time(row.report_name(), row.text(time))?;
        Ok(Some(ZonalLoad {
            timestamp: clock.resolve(zone_id, local)?,
            zone_id,
            zone_name: row.text(name).to_owned(),
            native_load_mw: row.float(native)?,
            behind_the_meter_solar_mw: row.float(solar)?,
        }))
    })
}

pub(crate) fn regulation_price(
    bytes: &[u8],
    _day: NaiveDate,
    _filter: &LocationFilter,
) -> Result<Decoded<RegulationPrice>, Error> {
    let report = Report::parse("regulation clearing price", bytes)?;
    let time = report.column(&["local time"])?;
    let service = report.column(&["regulation service clearing price"])?;
    let capacity = report.column(&["regulation capacity clearing price"])?;
    let mut clock = EasternClock::default();
    decode_with(&report, |row| {
        let local = parse_date_time(row.report_name(), row.text(time))?;
        Ok(Some(RegulationPrice {
            timestamp: clock.resolve((), local)?,
            service_price: row.float(service)?,
            capacity_price: row.float(capacity)?,
        }))
    })
}

pub(crate) fn reserve_requirement(
    bytes: &[u8],
    _day: NaiveDate,
    filter: &LocationFilter,
) -> Result<Decoded<ReserveRequirement>, Error> {
    let report = Report::parse("reserve requirement", bytes)?;
    let (date, hour) = (report.column(DATE)?, report.column(HOUR)?);
    let zone = report.column(&["location id"])?;
    let spinning = report.column(&["ten-minute spinning"])?;
    let ten = report.column(&["ten-minute"])?;
    let total = report.column(&["total"])?;
    decode_with(&report, |row| {
        let reserve_zone_id = row.uint(zone)?;
        if !filter.keeps(Some(reserve_zone_id), None) {
            return Ok(None);
        }
        let (_, interval_start) = day_and_hour(&row, date, hour)?;
        Ok(Some(ReserveRequirement {
            interval_start,
            hour_ending: row.text(hour).to_owned(),
            reserve_zone_id,
            ten_minute_spinning_mw: row.float(spinning)?,
            ten_minute_mw: row.float(ten)?,
            total_mw: row.float(total)?,
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_keeps_everything() {
        let f = LocationFilter::all();
        assert!(f.is_all());
        assert!(f.keeps(None, None));
        assert!(f.keeps(Some(1), Some("x")));
    }

    #[test]
    fn filter_matches_on_either_key() {
        let f = LocationFilter::id(4000).with_name(".Z.MAINE");
        assert!(f.keeps(Some(4000), None), "id match");
        assert!(f.keeps(None, Some(".Z.MAINE")), "name match");
        assert!(!f.keeps(Some(4001), Some(".Z.VERMONT")));
        assert!(!f.keeps(None, None), "a row with neither key cannot match");
    }

    #[test]
    fn hub_and_zone_constants_match_the_published_ids() {
        assert_eq!(INTERNAL_HUB_ID, 4000);
        assert_eq!(
            [
                ZONE_MAINE,
                ZONE_NEW_HAMPSHIRE,
                ZONE_VERMONT,
                ZONE_CONNECTICUT,
                ZONE_RHODE_ISLAND,
                ZONE_SEMASS,
                ZONE_WCMASS,
                ZONE_NEMASS_BOSTON
            ],
            [4001, 4002, 4003, 4004, 4005, 4006, 4007, 4008]
        );
    }
}

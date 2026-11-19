//! Typed rows for each NYISO dataset, and the CSV decoding that produces them.
//!
//! Every timestamp is a [`DateTime<FixedOffset>`] carrying the Eastern UTC offset
//! in force at that instant (-05:00 EST or -04:00 EDT), so the repeated hour on
//! the fall-back day stays unambiguous and values compare correctly across days.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, NaiveDateTime};
use serde::Serialize;

use crate::{
    dataset::Dataset,
    error::Error,
    table::{Row, Table},
    time::{parse_local, with_zone_cell, EasternClock},
};

mod sealed {
    pub trait Sealed {}
}

/// A typed row that can be decoded from a NYISO CSV file.
///
/// Sealed: the set of decodable rows is fixed by this crate.
pub trait Record: sealed::Sealed + Sized + Send + 'static {
    #[doc(hidden)]
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error>;
}

/// Locational based marginal price for one zone or generator and interval.
///
/// `lbmp = energy + marginal_cost_losses + marginal_cost_congestion`, all in $/`MWh`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lbmp {
    /// Start of the interval, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Zone or generator name, e.g. `CAPITL`.
    pub name: String,
    /// NYISO point identifier.
    pub ptid: u32,
    /// The LBMP in $/`MWh`.
    pub lbmp: f64,
    /// Marginal cost of losses in $/`MWh`.
    pub marginal_cost_losses: f64,
    /// Marginal cost of congestion in $/`MWh`.
    pub marginal_cost_congestion: f64,
}

impl Lbmp {
    /// The marginal energy component in $/`MWh`: the LBMP minus losses and congestion.
    #[must_use]
    pub fn energy_component(&self) -> f64 {
        self.lbmp - self.marginal_cost_losses - self.marginal_cost_congestion
    }
}

/// Zonal actual load for one interval (`pal` five-minute, `palIntegrated` hourly).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Load {
    /// Interval timestamp, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Zone name, e.g. `N.Y.C.`.
    pub name: String,
    /// NYISO point identifier.
    pub ptid: u32,
    /// Load in MW.
    pub load_mw: f64,
}

/// Hourly load forecast: one value per zone plus the NYISO total.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadForecast {
    /// Hour the forecast applies to, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Forecast MW by zone column as published (`Capitl`, `N.Y.C.`, ...).
    pub zones: BTreeMap<String, f64>,
    /// The `NYISO` system total in MW, when the file carries it.
    pub nyiso_mw: Option<f64>,
}

/// Generation by fuel category for one five-minute interval.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FuelMix {
    /// Interval timestamp, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Fuel category, e.g. `Natural Gas`.
    pub fuel_category: String,
    /// Generation in MW.
    pub gen_mw: f64,
}

/// Ancillary service prices for one zone and interval, all in $/`MWh` unless noted.
/// A price is `None` when the cell is blank.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AncillaryPrice {
    /// Interval timestamp, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Zone name, e.g. `CAPITL`.
    pub name: String,
    /// NYISO point identifier.
    pub ptid: u32,
    /// 10-minute spinning reserve price.
    pub spinning_10_min: Option<f64>,
    /// 10-minute non-synchronous reserve price.
    pub non_synchronous_10_min: Option<f64>,
    /// 30-minute operating reserve price.
    pub operating_30_min: Option<f64>,
    /// NYCA regulation capacity price.
    pub regulation_capacity: Option<f64>,
    /// NYCA regulation movement price in $/MW. Real-time files only.
    pub regulation_movement: Option<f64>,
}

/// Flow and limits on one interface or external scheduling point.
///
/// NYISO publishes `-12099` / `9999` style values to mean "no limit"; they are
/// passed through unchanged.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InterfaceFlow {
    /// Interval timestamp, in Eastern time.
    pub timestamp: DateTime<FixedOffset>,
    /// Interface name, e.g. `CENTRAL EAST - VC`.
    pub interface_name: String,
    /// NYISO point identifier.
    pub point_id: u32,
    /// Actual flow in `MWh` for the interval.
    pub flow_mw: Option<f64>,
    /// Positive-direction limit in `MWh`.
    pub positive_limit_mw: Option<f64>,
    /// Negative-direction limit in `MWh`.
    pub negative_limit_mw: Option<f64>,
}

/// Resolve a row's timestamp: from its `Time Zone` cell when the file has one,
/// else from the Eastern calendar via the per-series clock.
fn stamp(
    row: &Row<'_>,
    time_col: usize,
    zone_col: Option<usize>,
    clock: &mut EasternClock<String>,
    series: &str,
) -> Result<DateTime<FixedOffset>, Error> {
    let local: NaiveDateTime = parse_local(&row.dataset().to_string(), row.text(time_col))?;
    match zone_col {
        Some(z) => with_zone_cell(local, row.text(z)),
        None => clock.resolve(series.to_owned(), local),
    }
}

const TIME: &[&str] = &["time stamp", "timestamp"];
const ZONE: &[&str] = &["time zone"];
const NAME: &[&str] = &["name"];
const PTID: &[&str] = &["ptid"];

impl sealed::Sealed for Lbmp {}
impl Record for Lbmp {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let (name, ptid) = (table.column(NAME)?, table.column(PTID)?);
        let lbmp = table.column(&["lbmp"])?;
        let losses = table.column(&["marginal cost losses"])?;
        let congestion = table.column(&["marginal cost congestion"])?;
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, row.text(name))?,
                    name: row.text(name).to_owned(),
                    ptid: row.uint(ptid)?,
                    lbmp: row.float(lbmp)?,
                    marginal_cost_losses: row.float(losses)?,
                    marginal_cost_congestion: row.float(congestion)?,
                })
            })
            .collect()
    }
}

impl sealed::Sealed for Load {}
impl Record for Load {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let (name, ptid) = (table.column(NAME)?, table.column(PTID)?);
        let load = table.column(&["load", "integrated load"])?;
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, row.text(name))?,
                    name: row.text(name).to_owned(),
                    ptid: row.uint(ptid)?,
                    load_mw: row.float(load)?,
                })
            })
            .collect()
    }
}

impl sealed::Sealed for LoadForecast {}
impl Record for LoadForecast {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let value_columns: Vec<usize> = (0..table.width())
            .filter(|i| *i != time && Some(*i) != zone)
            .collect();
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                let mut zones = value_columns
                    .iter()
                    .map(|&i| Ok((table.raw_name(i).to_owned(), row.opt_float(i)?)))
                    .collect::<Result<Vec<(String, Option<f64>)>, Error>>()?
                    .into_iter()
                    .filter_map(|(name, v)| v.map(|v| (name, v)))
                    .collect::<BTreeMap<_, _>>();
                let nyiso_mw = zones.remove("NYISO");
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, "forecast")?,
                    zones,
                    nyiso_mw,
                })
            })
            .collect()
    }
}

impl sealed::Sealed for FuelMix {}
impl Record for FuelMix {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let fuel = table.column(&["fuel category"])?;
        let gen_mw = table.column(&["gen mw", "gen mwh"])?;
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, row.text(fuel))?,
                    fuel_category: row.text(fuel).to_owned(),
                    gen_mw: row.float(gen_mw)?,
                })
            })
            .collect()
    }
}

impl sealed::Sealed for AncillaryPrice {}
impl Record for AncillaryPrice {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        // Before 2017 the files hold regional East/West or SENY/... columns, not one row per zone.
        let (Some(name), Some(ptid)) = (table.find(NAME), table.find(PTID)) else {
            return Err(Error::UnsupportedLayout {
                dataset: dataset.to_string(),
                header: table.header_text(),
            });
        };
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let spinning = table.column(&["10 min spinning reserve"])?;
        let non_sync = table.column(&["10 min non-synchronous reserve"])?;
        let operating = table.column(&["30 min operating reserve"])?;
        let reg_capacity = table.column(&["nyca regulation capacity"])?;
        let reg_movement = table.find(&["nyca regulation movement"]);
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, row.text(name))?,
                    name: row.text(name).to_owned(),
                    ptid: row.uint(ptid)?,
                    spinning_10_min: row.opt_float(spinning)?,
                    non_synchronous_10_min: row.opt_float(non_sync)?,
                    operating_30_min: row.opt_float(operating)?,
                    regulation_capacity: row.opt_float(reg_capacity)?,
                    regulation_movement: reg_movement.map_or(Ok(None), |c| row.opt_float(c))?,
                })
            })
            .collect()
    }
}

impl sealed::Sealed for InterfaceFlow {}
impl Record for InterfaceFlow {
    fn decode(dataset: Dataset, bytes: &[u8]) -> Result<Vec<Self>, Error> {
        let table = Table::read(dataset, bytes)?;
        let (time, zone) = (table.column(TIME)?, table.find(ZONE));
        let interface = table.column(&["interface name"])?;
        let point = table.column(&["point id"])?;
        let flow = table.column(&["flow"])?;
        let positive = table.column(&["positive limit"])?;
        let negative = table.column(&["negative limit"])?;
        let mut clock = EasternClock::default();
        table
            .rows()
            .map(|row| {
                Ok(Self {
                    timestamp: stamp(&row, time, zone, &mut clock, row.text(interface))?,
                    interface_name: row.text(interface).to_owned(),
                    point_id: row.uint(point)?,
                    flow_mw: row.opt_float(flow)?,
                    positive_limit_mw: row.opt_float(positive)?,
                    negative_limit_mw: row.opt_float(negative)?,
                })
            })
            .collect()
    }
}

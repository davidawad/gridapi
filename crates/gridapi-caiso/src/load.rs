//! Load forecast (`SLD_FCST`) and system load and resource schedule (`ENE_SLRS`) rows.

use chrono::{DateTime, Utc};

use crate::error::Error;
use crate::record::{FromRecord, Record};

/// One load forecast value for a TAC area and interval, in MW.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadForecastRow {
    /// Interval start, UTC (inclusive).
    pub interval_start: DateTime<Utc>,
    /// Interval end, UTC (exclusive).
    pub interval_end: DateTime<Utc>,
    /// The TAC area (`CA ISO-TAC` is the system total; also `PGE-TAC`, `AVA`, ...).
    pub tac_area: String,
    /// The market run the forecast belongs to (`DAM`, `2DA`, `7DA`, `RTM`).
    pub market_run_id: Option<String>,
    /// The forecast series, e.g. `SYS_FCST_DA_MW`.
    pub data_item: String,
    /// Human-readable series label, e.g. `Demand Forecast Day Ahead`.
    pub label: Option<String>,
    /// Forecast demand in MW.
    pub mw: f64,
}

impl FromRecord for LoadForecastRow {
    fn from_record(r: &Record) -> Result<Self, Error> {
        Ok(Self {
            interval_start: r.timestamp(&["INTERVALSTARTTIME_GMT", "INTERVAL_START_GMT"])?,
            interval_end: r.timestamp(&["INTERVALENDTIME_GMT", "INTERVAL_END_GMT"])?,
            tac_area: r.require(&["TAC_AREA_NAME", "RESOURCE_NAME"])?.to_owned(),
            market_run_id: r.first(&["MARKET_RUN_ID", "MKT_TYPE"]).map(str::to_owned),
            data_item: r.require(&["XML_DATA_ITEM", "DATA_ITEM"])?.to_owned(),
            label: r.first(&["LABEL"]).map(str::to_owned),
            mw: r.number(&["MW", "VALUE"])?,
        })
    }
}

/// One system load or resource schedule quantity for a TAC zone and interval, in MW.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemScheduleRow {
    /// Interval start, UTC (inclusive).
    pub interval_start: DateTime<Utc>,
    /// Interval end, UTC (exclusive).
    pub interval_end: DateTime<Utc>,
    /// The TAC zone (`Caiso_Totals` for the system, or a zone such as `TAC_ECA`).
    pub tac_zone: String,
    /// The schedule kind (`Generation`, `Import`, `Export`, ...), CSV only.
    pub schedule: Option<String>,
    /// The quantity, e.g. `ISO_TOT_GEN_MW`.
    pub data_item: String,
    /// The value in MW.
    pub mw: f64,
}

impl FromRecord for SystemScheduleRow {
    fn from_record(r: &Record) -> Result<Self, Error> {
        Ok(Self {
            interval_start: r.timestamp(&["INTERVALSTARTTIME_GMT", "INTERVAL_START_GMT"])?,
            interval_end: r.timestamp(&["INTERVALENDTIME_GMT", "INTERVAL_END_GMT"])?,
            tac_zone: r.require(&["TAC_ZONE_NAME", "RESOURCE_NAME"])?.to_owned(),
            schedule: r.first(&["SCHEDULE"]).map(str::to_owned),
            data_item: r.require(&["XML_DATA_ITEM", "DATA_ITEM"])?.to_owned(),
            mw: r.number(&["MW", "VALUE"])?,
        })
    }
}

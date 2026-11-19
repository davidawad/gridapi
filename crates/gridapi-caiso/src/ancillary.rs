//! Ancillary service price rows (`PRC_AS`).

use std::fmt;

use chrono::{DateTime, Utc};

use crate::error::Error;
use crate::record::{FromRecord, Record};

/// The `anc_type` parameter of `PRC_AS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AncillaryType {
    /// Every product. OASIS serves this as a whole-day group download.
    All,
    /// Regulation up (`RU`).
    RegulationUp,
    /// Regulation down (`RD`).
    RegulationDown,
    /// Spinning reserve (`SR`).
    Spinning,
    /// Non-spinning reserve (`NR`).
    NonSpinning,
}

impl AncillaryType {
    /// The wire code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::RegulationUp => "RU",
            Self::RegulationDown => "RD",
            Self::Spinning => "SR",
            Self::NonSpinning => "NR",
        }
    }
}

impl fmt::Display for AncillaryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One ancillary service price for a region and interval, in US$/MW.
#[derive(Debug, Clone, PartialEq)]
pub struct AncillaryPriceRow {
    /// Interval start, UTC (inclusive).
    pub interval_start: DateTime<Utc>,
    /// Interval end, UTC (exclusive).
    pub interval_end: DateTime<Utc>,
    /// The product code (`RU`, `RD`, `SR`, `NR`).
    pub anc_type: String,
    /// The AS region, e.g. `AS_CAISO`, `AS_CAISO_EXP`, `AS_NP26`, `AS_SP26`.
    pub anc_region: String,
    /// The price series, e.g. `RU_CLR_PRC`.
    pub data_item: String,
    /// The clearing price in US$/MW.
    pub price: f64,
}

impl FromRecord for AncillaryPriceRow {
    fn from_record(r: &Record) -> Result<Self, Error> {
        let data_item = r.require(&["XML_DATA_ITEM", "DATA_ITEM"])?.to_owned();
        // XML reports carry no ANC_TYPE column; the data item is prefixed with it.
        let anc_type = r
            .first(&["ANC_TYPE"])
            .or_else(|| data_item.split('_').next())
            .unwrap_or_default()
            .to_owned();
        Ok(Self {
            interval_start: r.timestamp(&["INTERVALSTARTTIME_GMT", "INTERVAL_START_GMT"])?,
            interval_end: r.timestamp(&["INTERVALENDTIME_GMT", "INTERVAL_END_GMT"])?,
            anc_type,
            anc_region: r.require(&["ANC_REGION", "RESOURCE_NAME"])?.to_owned(),
            data_item,
            price: r.number(&["MW", "VALUE", "PRC"])?,
        })
    }
}

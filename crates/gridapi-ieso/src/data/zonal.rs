//! Zonal price reports (XML).
//!
//! Ontario moved to a locational marginal price (LMP) market in 2025. The
//! single Ontario zonal price is now derived from the nodal prices; the
//! pre-LMP `PUB_RealtimeOntarioZonalPrice.csv` file no longer exists.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::data::xml::{parse_document, Document};
use crate::error::Result;
use crate::serde_util::{children, opt_f64, opt_i32, opt_text};
use crate::time;

/// One delivery hour of the Ontario zonal price (day-ahead and pre-dispatch).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ZonalPriceHour {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "PricingHour")]
    pub hour: u8,
    /// Ontario zonal price in CAD/MWh.
    #[serde(rename = "ZonalPrice", default, deserialize_with = "opt_f64")]
    pub zonal_price: Option<f64>,
    /// Loss component in CAD/MWh.
    #[serde(rename = "LossPriceCapped", default, deserialize_with = "opt_f64")]
    pub loss_price: Option<f64>,
    /// Congestion component in CAD/MWh.
    #[serde(
        rename = "CongestionPriceCapped",
        default,
        deserialize_with = "opt_f64"
    )]
    pub congestion_price: Option<f64>,
    /// Status flag (for example `DSO-RD`); absent in pre-dispatch reports.
    #[serde(rename = "Flag", default, deserialize_with = "opt_text")]
    pub flag: Option<String>,
}

/// Day-ahead (`DAHourlyOntarioZonalPrice`) or pre-dispatch
/// (`PredispHourlyOntarioZonalPrice`) Ontario zonal prices for one date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OntarioZonalHourly {
    /// The delivery date.
    #[serde(rename = "DeliveryDate")]
    pub delivery_date: NaiveDate,
    /// One entry per delivery hour.
    #[serde(rename = "HourlyPriceComponents", default)]
    pub hours: Vec<ZonalPriceHour>,
}

impl OntarioZonalHourly {
    /// The entry for hour-ending `hour`, if present.
    pub fn hour(&self, hour: u8) -> Option<&ZonalPriceHour> {
        self.hours.iter().find(|h| h.hour == hour)
    }

    /// UTC start of an entry's delivery hour.
    pub fn start_utc(&self, hour: u8) -> Option<DateTime<Utc>> {
        time::hour_start_utc(self.delivery_date, hour)
    }
}

/// One five-minute interval of the real-time Ontario zonal price.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RealtimeInterval {
    /// Interval within the hour, `1..=12`.
    #[serde(rename = "Interval")]
    pub interval: u8,
    /// Status flag; empty until the interval is published.
    #[serde(rename = "Flag", default, deserialize_with = "opt_text")]
    pub flag: Option<String>,
    /// Capped LMP in CAD/MWh; `None` until published.
    #[serde(rename = "LmpCap", default, deserialize_with = "opt_f64")]
    pub lmp: Option<f64>,
    /// Loss component in CAD/MWh.
    #[serde(rename = "LossPriceCap", default, deserialize_with = "opt_f64")]
    pub loss_price: Option<f64>,
    /// Congestion component in CAD/MWh.
    #[serde(rename = "CongPriceCap", default, deserialize_with = "opt_f64")]
    pub congestion_price: Option<f64>,
}

/// Hour average of the published real-time intervals.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RealtimeAverage {
    /// Average capped LMP in CAD/MWh.
    #[serde(rename = "LmpCap", default, deserialize_with = "opt_f64")]
    pub lmp: Option<f64>,
    /// Average loss component in CAD/MWh.
    #[serde(rename = "LossPriceCap", default, deserialize_with = "opt_f64")]
    pub loss_price: Option<f64>,
    /// Average congestion component in CAD/MWh.
    #[serde(rename = "CongPriceCap", default, deserialize_with = "opt_f64")]
    pub congestion_price: Option<f64>,
}

/// `RealtimeOntarioZonalPrice`: twelve five-minute prices for one delivery hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OntarioZonalRealtime {
    /// The delivery date.
    #[serde(rename = "DeliveryDate")]
    pub delivery_date: NaiveDate,
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "DeliveryHour")]
    pub delivery_hour: u8,
    /// Intervals `1..=12`; unpublished ones have `lmp == None`.
    #[serde(rename = "ZonalPrice", default)]
    pub intervals: Vec<RealtimeInterval>,
    /// Average over the published intervals.
    #[serde(rename = "AveragePrice", default)]
    pub average: Option<RealtimeAverage>,
}

impl OntarioZonalRealtime {
    /// Published intervals with their UTC start time.
    pub fn published(&self) -> impl Iterator<Item = (DateTime<Utc>, &RealtimeInterval)> + '_ {
        self.intervals
            .iter()
            .filter(|i| i.lmp.is_some())
            .filter_map(|i| {
                time::interval_start_utc(self.delivery_date, self.delivery_hour, i.interval)
                    .map(|t| (t, i))
            })
    }
}

/// One five-minute price for a virtual transaction zone.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VirtualZonalInterval {
    /// Interval within the hour, `1..=12`.
    #[serde(rename = "Interval")]
    pub interval: u8,
    /// Zonal energy price in CAD/MWh; `None` until published.
    #[serde(rename = "ZonalPrice", default, deserialize_with = "opt_f64")]
    pub zonal_price: Option<f64>,
    /// Loss component in CAD/MWh.
    #[serde(rename = "EnergyLossPrice", default, deserialize_with = "opt_f64")]
    pub loss_price: Option<f64>,
    /// Congestion component in CAD/MWh.
    #[serde(rename = "EnergyCongPrice", default, deserialize_with = "opt_f64")]
    pub congestion_price: Option<f64>,
    /// Quality flag number.
    #[serde(rename = "FlagNo", default, deserialize_with = "opt_i32")]
    pub flag: Option<i32>,
}

/// A virtual transaction zone (for example `EAST:HUB`) and its intervals.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VirtualZone {
    /// Zone name.
    #[serde(rename = "ZoneName")]
    pub name: String,
    /// Intervals `1..=12`.
    #[serde(rename = "IntervalPrice", default)]
    pub intervals: Vec<VirtualZonalInterval>,
}

/// `RealtimeZonalEnergyPrices`: virtual zonal prices for one delivery hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VirtualZonalRealtime {
    /// The delivery date.
    #[serde(rename = "DELIVERYDATE", alias = "DeliveryDate")]
    pub delivery_date: NaiveDate,
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "DELIVERYHOUR", alias = "DeliveryHour")]
    pub delivery_hour: u8,
    /// All virtual zones.
    #[serde(rename = "ZonalPrices", default, deserialize_with = "children")]
    pub zones: Vec<VirtualZone>,
}

/// Decode `DAHourlyOntarioZonalPrice` or `PredispHourlyOntarioZonalPrice`.
pub fn parse_ontario_zonal_hourly(xml: &str) -> Result<Document<OntarioZonalHourly>> {
    parse_document(xml)
}

/// Decode `RealtimeOntarioZonalPrice`.
pub fn parse_ontario_zonal_realtime(xml: &str) -> Result<Document<OntarioZonalRealtime>> {
    parse_document(xml)
}

/// Decode `RealtimeZonalEnergyPrices`.
pub fn parse_virtual_zonal_realtime(xml: &str) -> Result<Document<VirtualZonalRealtime>> {
    parse_document(xml)
}

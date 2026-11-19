//! Typed decoders for the report payloads.
//!
//! Every `parse_*` function is pure (text in, value out) so it can be used on
//! files you fetched yourself, cached, or got from elsewhere.

pub mod adequacy;
pub mod demand;
pub mod generation;
pub mod intertie;
pub mod lmp;
pub mod xml;
pub mod zonal;

pub use xml::Document;

/// One hour's value in a series: `hour` is hour-ending `1..=24`; `value` is
/// MW (or `MWh` for energy series) and `None` when IESO left the cell empty.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct HourlyValue {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Hour", alias = "DeliveryHour")]
    pub hour: u8,
    /// The value for the hour.
    #[serde(
        rename = "EnergyMW",
        alias = "EnergyMWhr",
        default,
        deserialize_with = "crate::serde_util::opt_f64"
    )]
    pub value: Option<f64>,
}

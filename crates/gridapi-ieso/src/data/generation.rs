//! Generation reports (XML): per-generator output/capability and hourly
//! output by fuel type.

use chrono::NaiveDate;
use serde::Deserialize;

use crate::data::xml::{parse_document, Document};
use crate::data::HourlyValue;
use crate::error::Result;
use crate::serde_util::{children, opt_f64, opt_i32};

/// One generating unit's hourly series for a date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Generator {
    /// Unit name, for example `BRUCEA-G1`.
    #[serde(rename = "GeneratorName")]
    pub name: String,
    /// Fuel type (`NUCLEAR`, `GAS`, `HYDRO`, `WIND`, `SOLAR`, `BIOFUEL`, `OTHER`).
    #[serde(rename = "FuelType", default)]
    pub fuel_type: String,
    /// Metered output in MW per hour.
    #[serde(rename = "Outputs", default, deserialize_with = "children")]
    pub outputs: Vec<HourlyValue>,
    /// Capability in MW per hour (what the unit could produce).
    #[serde(rename = "Capabilities", default, deserialize_with = "children")]
    pub capabilities: Vec<HourlyValue>,
    /// Available capacity in MW per hour.
    #[serde(rename = "Capacities", default, deserialize_with = "children")]
    pub available_capacity: Vec<HourlyValue>,
}

/// `GenOutputCapability` for one delivery date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneratorOutputCapability {
    /// The delivery date.
    #[serde(rename = "Date")]
    pub date: NaiveDate,
    /// Every transmission-connected generator.
    #[serde(rename = "Generators", default, deserialize_with = "children")]
    pub generators: Vec<Generator>,
}

impl GeneratorOutputCapability {
    /// Total metered output per hour in MW, summed over generators matching
    /// `fuel_type` (case-insensitive). Hours with no reported value count as zero.
    pub fn output_by_fuel(&self, fuel_type: &str) -> [f64; 24] {
        self.generators
            .iter()
            .filter(|g| g.fuel_type.eq_ignore_ascii_case(fuel_type))
            .flat_map(|g| g.outputs.iter())
            .fold([0.0; 24], |mut acc, v| {
                if let Some(slot) = usize::from(v.hour)
                    .checked_sub(1)
                    .and_then(|i| acc.get_mut(i))
                {
                    *slot += v.value.unwrap_or(0.0);
                }
                acc
            })
    }
}

/// One fuel's output in one hour.
#[derive(Debug, Clone, PartialEq)]
pub struct FuelOutput {
    /// Fuel type (`NUCLEAR`, `GAS`, `HYDRO`, `WIND`, `SOLAR`, `BIOFUEL`, `OTHER`).
    pub fuel: String,
    /// IESO data-quality code (`0` good, `-1` estimated, ...).
    pub quality: Option<i32>,
    /// Output in MW (average over the hour).
    pub output_mw: Option<f64>,
}

#[derive(Deserialize)]
struct RawFuelTotal {
    #[serde(rename = "Fuel")]
    fuel: String,
    #[serde(rename = "EnergyValue", default)]
    value: RawEnergyValue,
}

#[derive(Deserialize, Default)]
struct RawEnergyValue {
    #[serde(rename = "OutputQuality", default, deserialize_with = "opt_i32")]
    quality: Option<i32>,
    #[serde(rename = "Output", default, deserialize_with = "opt_f64")]
    output: Option<f64>,
}

impl<'de> Deserialize<'de> for FuelOutput {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = RawFuelTotal::deserialize(d)?;
        Ok(Self {
            fuel: raw.fuel,
            quality: raw.value.quality,
            output_mw: raw.value.output,
        })
    }
}

/// All fuels for one delivery hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HourFuelOutput {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Hour")]
    pub hour: u8,
    /// One entry per fuel type.
    #[serde(rename = "FuelTotal", default)]
    pub fuels: Vec<FuelOutput>,
}

/// All hours of one delivery date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DayFuelOutput {
    /// The delivery date.
    #[serde(rename = "Day")]
    pub day: NaiveDate,
    /// One entry per delivery hour published so far.
    #[serde(rename = "HourlyData", default)]
    pub hours: Vec<HourFuelOutput>,
}

/// `GenOutputbyFuelHourly`: every day of one calendar year.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FuelOutputYear {
    /// The delivery year.
    #[serde(rename = "DeliveryYear")]
    pub year: i32,
    /// One entry per delivery date published so far.
    #[serde(rename = "DailyData", default)]
    pub days: Vec<DayFuelOutput>,
}

/// Decode `GenOutputCapability`.
pub fn parse_output_capability(xml: &str) -> Result<Document<GeneratorOutputCapability>> {
    parse_document(xml)
}

/// Decode `GenOutputbyFuelHourly`.
pub fn parse_output_by_fuel_hourly(xml: &str) -> Result<Document<FuelOutputYear>> {
    parse_document(xml)
}

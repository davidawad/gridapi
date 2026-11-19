//! `Adequacy3`: the day's supply/demand outlook by hour.
//!
//! The XML carries far more than most users need (zonal imports and exports,
//! area demand, dispatchable load, reserve holdbacks). This module decodes the
//! headline series and the internal resources by fuel; everything else is left
//! to [`crate::Client::fetch`] plus your own deserializer.

use chrono::NaiveDate;
use serde::Deserialize;

use crate::data::xml::{parse_document, Document};
use crate::data::HourlyValue;
use crate::error::Result;
use crate::serde_util::children;

/// Supply side of the outlook.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ForecastSupply {
    /// Forecast supply capacity in MW per hour.
    #[serde(rename = "Capacities", default, deserialize_with = "children")]
    pub capacities: Vec<HourlyValue>,
    /// Forecast supply energy in `MWh` per hour.
    #[serde(rename = "Energies", default, deserialize_with = "children")]
    pub energies: Vec<HourlyValue>,
    /// Internal (in-province) resources by fuel type.
    #[serde(rename = "InternalResources", default)]
    pub internal_resources: InternalResources,
    /// Bottled (stranded) capacity in MW per hour.
    #[serde(rename = "BottledCapacities", default, deserialize_with = "children")]
    pub bottled_capacities: Vec<HourlyValue>,
    /// Regulation requirement in MW per hour.
    #[serde(rename = "Regulations", default, deserialize_with = "children")]
    pub regulations: Vec<HourlyValue>,
    /// Total supply in MW per hour.
    #[serde(rename = "TotalSupplies", default, deserialize_with = "children")]
    pub total_supplies: Vec<HourlyValue>,
}

/// Wrapper for the per-fuel list.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct InternalResources {
    /// One entry per fuel type.
    #[serde(rename = "InternalResource", default)]
    pub by_fuel: Vec<InternalResource>,
}

/// One fuel type's supply outlook.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InternalResource {
    /// Fuel type (`Nuclear`, `Gas`, `Hydro`, `Wind`, `Solar`, `Biofuel`).
    #[serde(rename = "FuelType")]
    pub fuel_type: String,
    /// Installed capacity in MW per hour.
    #[serde(rename = "Capacities", default, deserialize_with = "children")]
    pub capacities: Vec<HourlyValue>,
    /// Outages in MW per hour.
    #[serde(rename = "Outages", default, deserialize_with = "children")]
    pub outages: Vec<HourlyValue>,
    /// Offered MW per hour.
    #[serde(rename = "Offers", default, deserialize_with = "children")]
    pub offers: Vec<HourlyValue>,
    /// Scheduled MW per hour.
    #[serde(rename = "Schedules", default, deserialize_with = "children")]
    pub schedules: Vec<HourlyValue>,
}

/// Ontario demand forecast series.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct OntarioDemand {
    /// Forecast Ontario demand in MW per hour.
    #[serde(rename = "ForecastOntDemand", default, deserialize_with = "children")]
    pub forecast: Vec<HourlyValue>,
    /// Forecast peak demand in MW per hour.
    #[serde(rename = "PeakDemand", default, deserialize_with = "children")]
    pub peak: Vec<HourlyValue>,
    /// Forecast average demand in MW per hour.
    #[serde(rename = "AverageDemand", default, deserialize_with = "children")]
    pub average: Vec<HourlyValue>,
    /// Embedded wind generation in MW per hour.
    #[serde(rename = "WindEmbedded", default, deserialize_with = "children")]
    pub wind_embedded: Vec<HourlyValue>,
    /// Embedded solar generation in MW per hour.
    #[serde(rename = "SolarEmbedded", default, deserialize_with = "children")]
    pub solar_embedded: Vec<HourlyValue>,
}

/// Demand side of the outlook.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ForecastDemand {
    /// Ontario demand series.
    #[serde(rename = "OntarioDemand", default)]
    pub ontario_demand: OntarioDemand,
    /// Total requirement (demand plus reserves) in MW per hour.
    #[serde(rename = "TotalRequirements", default, deserialize_with = "children")]
    pub total_requirements: Vec<HourlyValue>,
    /// Excess capacity in MW per hour.
    #[serde(rename = "ExcessCapacities", default, deserialize_with = "children")]
    pub excess_capacities: Vec<HourlyValue>,
    /// Excess energy in `MWh` per hour.
    #[serde(rename = "ExcessEnergies", default, deserialize_with = "children")]
    pub excess_energies: Vec<HourlyValue>,
    /// Excess offered capacity in MW per hour.
    #[serde(
        rename = "ExcessOfferedCapacities",
        default,
        deserialize_with = "children"
    )]
    pub excess_offered_capacities: Vec<HourlyValue>,
    /// Unscheduled resources in MW per hour.
    #[serde(
        rename = "UnscheduledResources",
        default,
        deserialize_with = "children"
    )]
    pub unscheduled_resources: Vec<HourlyValue>,
    /// Unscheduled imports in MW per hour.
    #[serde(rename = "UnscheduledImports", default, deserialize_with = "children")]
    pub unscheduled_imports: Vec<HourlyValue>,
}

/// `Adequacy3` for one delivery date.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Adequacy {
    /// The delivery date.
    #[serde(rename = "DeliveryDate")]
    pub delivery_date: NaiveDate,
    /// Supply outlook.
    #[serde(rename = "ForecastSupply")]
    pub supply: ForecastSupply,
    /// Demand outlook.
    #[serde(rename = "ForecastDemand")]
    pub demand: ForecastDemand,
}

/// Decode `Adequacy3`.
pub fn parse_adequacy(xml: &str) -> Result<Document<Adequacy>> {
    parse_document(xml)
}

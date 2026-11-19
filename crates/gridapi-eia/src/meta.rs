//! Route metadata: what `GET /v2/<route>` and `GET /v2/<route>/facet/<id>`
//! return. Use these to discover datasets, their frequencies, facets and data
//! columns without leaving the crate.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::de::opt_u32;

/// One entry in a route's `routes` list: a child route.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ChildRoute {
    /// Path segment, for example `rto`.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Longer description, often with the source survey form.
    #[serde(default)]
    pub description: String,
}

/// A frequency a dataset supports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FrequencyMeta {
    /// Wire value for the `frequency` parameter, for example `hourly`.
    pub id: String,
    /// Display alias, for example `hourly (UTC)`.
    #[serde(default)]
    pub alias: Option<String>,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Short code (`H`, `M`, `A`).
    #[serde(default)]
    pub query: String,
    /// Period format, for example `YYYY-MM`.
    #[serde(default)]
    pub format: String,
}

/// A facet (filter dimension) a dataset supports.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FacetMeta {
    /// Facet id used in `facets[<id>][]`.
    pub id: String,
    /// Description.
    #[serde(default)]
    pub description: String,
}

/// A selectable data column of a dataset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DataColumn {
    /// Display name.
    #[serde(default)]
    pub alias: String,
    /// Units, when the column is numeric.
    #[serde(default)]
    pub units: Option<String>,
    /// Aggregation method across periods.
    #[serde(default, rename = "aggregation-method")]
    pub aggregation_method: Option<String>,
}

/// Metadata for any route node: the root, an intermediate node (has
/// `routes`), or a dataset (has `frequency`, `facets` and `data`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RouteMeta {
    /// Route id (empty for the root).
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Child routes (non-empty for intermediate nodes).
    #[serde(default)]
    pub routes: Vec<ChildRoute>,
    /// Supported frequencies (datasets only).
    #[serde(default)]
    pub frequency: Vec<FrequencyMeta>,
    /// Supported facets (datasets only).
    #[serde(default)]
    pub facets: Vec<FacetMeta>,
    /// Data columns keyed by column id (datasets only).
    #[serde(default)]
    pub data: BTreeMap<String, DataColumn>,
    /// First period available.
    #[serde(default, rename = "startPeriod")]
    pub start_period: Option<String>,
    /// Last period available.
    #[serde(default, rename = "endPeriod")]
    pub end_period: Option<String>,
    /// Format of `period` in data rows.
    #[serde(default, rename = "defaultDateFormat")]
    pub default_date_format: Option<String>,
    /// Frequency used when a query omits it.
    #[serde(default, rename = "defaultFrequency")]
    pub default_frequency: Option<String>,
}

impl RouteMeta {
    /// True when this node serves data (`/data` is queryable).
    #[must_use]
    pub fn is_dataset(&self) -> bool {
        !self.data.is_empty()
    }
}

/// One value of a facet, from `GET /v2/<route>/facet/<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FacetValue {
    /// Value to pass in `facets[<id>][]`.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Display alias, sometimes `(ID) Name`.
    #[serde(default)]
    pub alias: Option<String>,
}

/// Facet value listing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct FacetList {
    #[serde(default, rename = "totalFacets", deserialize_with = "opt_u32")]
    pub(crate) total: Option<u32>,
    #[serde(default)]
    pub(crate) facets: Vec<FacetValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialises_dataset_meta() {
        let json = include_str!("../tests/fixtures/meta_region_data.json");
        let env: serde_json::Value = serde_json::from_str(json).unwrap();
        let m: RouteMeta = serde_json::from_value(env["response"].clone()).unwrap();
        assert_eq!(m.id, "region-data");
        assert!(m.is_dataset());
        assert_eq!(
            m.frequency
                .iter()
                .map(|f| f.query.as_str())
                .collect::<Vec<_>>(),
            ["H", "LH"]
        );
        assert_eq!(
            m.facets.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            ["respondent", "type"]
        );
        assert_eq!(m.data["value"].units.as_deref(), Some("megawatthours"));
        assert_eq!(m.data["value"].aggregation_method.as_deref(), Some("SUM"));
        assert_eq!(m.default_frequency.as_deref(), Some("hourly"));
    }

    #[test]
    fn deserialises_intermediate_node() {
        let json = include_str!("../tests/fixtures/meta_rto.json");
        let env: serde_json::Value = serde_json::from_str(json).unwrap();
        let m: RouteMeta = serde_json::from_value(env["response"].clone()).unwrap();
        assert!(!m.is_dataset());
        assert!(m.routes.iter().any(|r| r.id == "fuel-type-data"));
    }
}

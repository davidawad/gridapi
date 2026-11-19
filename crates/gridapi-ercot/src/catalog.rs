//! Report catalog: the list of EMIL products and their data endpoints.

use serde::Deserialize;

use crate::error::{Error, Result};

/// One data endpoint (artifact) of a product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Numeric report type id.
    pub report_type_id: Option<u64>,
    /// Display title.
    pub display_name: String,
    /// Full endpoint URL, e.g. `.../np6-905-cd/spp_node_zone_hub`.
    pub endpoint: String,
}

impl Artifact {
    /// Artifact name: the last path segment of the endpoint (`spp_node_zone_hub`).
    #[must_use]
    pub fn name(&self) -> &str {
        self.endpoint
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactWire {
    report_type_id: Option<u64>,
    #[serde(default)]
    display_name: String,
    #[serde(rename = "_links", default)]
    links: ArtifactLinks,
}

#[derive(Debug, Default, Deserialize)]
struct ArtifactLinks {
    endpoint: Option<Href>,
}

#[derive(Debug, Deserialize)]
struct Href {
    href: String,
}

/// A report product (EMIL id) from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    /// EMIL product id, e.g. `NP6-905-CD`.
    pub emil_id: String,
    /// Product title.
    pub name: String,
    /// Longer description.
    #[serde(default)]
    pub description: Option<String>,
    /// `Active` or retired.
    #[serde(default)]
    pub status: Option<String>,
    /// Numeric report type id.
    #[serde(default)]
    pub report_type_id: Option<u64>,
    /// Cadence, e.g. `Chron - 15 Minutes`.
    #[serde(default)]
    pub generation_frequency: Option<String>,
    /// `DATA` (queryable rows) or `BINARY` (file-only).
    #[serde(default)]
    pub content_type: Option<String>,
    /// Timestamp of the latest posting (`yyyy-MM-ddTHH:mm:ss`, Central time).
    #[serde(default)]
    pub last_post_datetime: Option<String>,
    /// Days of history the archive holds.
    #[serde(default)]
    pub archive_duration: Option<u32>,
    /// Maximum documents per bulk download.
    #[serde(default)]
    pub download_limit: Option<u32>,
    /// Data endpoints; empty for `BINARY` products.
    #[serde(default, deserialize_with = "artifacts")]
    pub artifacts: Vec<Artifact>,
}

fn artifacts<'de, D>(de: D) -> Result<Vec<Artifact>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let wire = Vec::<ArtifactWire>::deserialize(de)?;
    Ok(wire
        .into_iter()
        .filter_map(|a| {
            Some(Artifact {
                report_type_id: a.report_type_id,
                display_name: a.display_name,
                endpoint: a.links.endpoint?.href,
            })
        })
        .collect())
}

impl Product {
    /// True when the product has queryable data endpoints.
    #[must_use]
    pub fn is_queryable(&self) -> bool {
        !self.artifacts.is_empty()
    }

    /// The artifact called `name` (case-insensitive).
    #[must_use]
    pub fn artifact(&self, name: &str) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|a| a.name().eq_ignore_ascii_case(name))
    }

    /// True when `term` appears in the id, name or description (case-insensitive).
    #[must_use]
    pub fn matches(&self, term: &str) -> bool {
        let term = term.to_ascii_lowercase();
        let hay = |s: &str| s.to_ascii_lowercase().contains(&term);
        hay(&self.emil_id) || hay(&self.name) || self.description.as_deref().is_some_and(hay)
    }
}

#[derive(Debug, Deserialize)]
struct CatalogWire {
    #[serde(rename = "_embedded")]
    embedded: Embedded,
}

#[derive(Debug, Deserialize)]
struct Embedded {
    products: Vec<Product>,
}

/// Parses the catalog listing (`GET /api/public-reports`).
///
/// # Errors
///
/// [`Error::Decode`] when the body is not a catalog.
pub fn parse_catalog(body: &[u8]) -> Result<Vec<Product>> {
    serde_json::from_slice::<CatalogWire>(body)
        .map(|w| w.embedded.products)
        .map_err(|e| Error::decode("product catalog", e))
}

/// Parses a single product (`GET /api/public-reports/{emil}`).
///
/// # Errors
///
/// [`Error::Decode`] when the body is not a product.
pub fn parse_product(body: &[u8]) -> Result<Product> {
    serde_json::from_slice(body).map_err(|e| Error::decode("product", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &[u8] = include_bytes!("../tests/fixtures/catalog_sample.json");

    #[test]
    fn parses_products_and_artifacts() {
        let products = parse_catalog(CATALOG).expect("fixture");
        assert!(products.len() >= 4, "{}", products.len());
        let spp = products
            .iter()
            .find(|p| p.emil_id == "NP6-905-CD")
            .expect("np6-905");
        assert_eq!(spp.content_type.as_deref(), Some("DATA"));
        assert_eq!(spp.artifacts.len(), 1);
        assert_eq!(spp.artifacts[0].name(), "spp_node_zone_hub");
        assert!(spp.is_queryable());
        assert!(spp.artifact("SPP_NODE_ZONE_HUB").is_some());
    }

    #[test]
    fn binary_products_have_no_artifacts() {
        let products = parse_catalog(CATALOG).expect("fixture");
        let binary = products
            .iter()
            .find(|p| p.content_type.as_deref() == Some("BINARY"))
            .expect("fixture keeps one binary product");
        assert!(!binary.is_queryable());
    }

    #[test]
    fn matches_searches_id_name_and_description() {
        let products = parse_catalog(CATALOG).expect("fixture");
        let hits: Vec<_> = products
            .iter()
            .filter(|p| p.matches("settlement point"))
            .collect();
        assert!(hits.iter().any(|p| p.emil_id == "NP6-905-CD"));
        assert!(products.iter().any(|p| p.matches("np4-190")));
        assert!(!products.iter().any(|p| p.matches("zzz-no-such-thing")));
    }

    #[test]
    fn garbage_is_a_decode_error() {
        assert!(matches!(parse_catalog(b"{}"), Err(Error::Decode { .. })));
    }
}

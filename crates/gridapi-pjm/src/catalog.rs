//! Feed catalog and field-definition (metadata) types.

use std::fmt;

use chrono::{DateTime, FixedOffset, NaiveDateTime};
use serde::{de, Deserialize, Deserializer};

use crate::page::Link;

/// One entry of the feed list (`GET /api/v1/`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSummary {
    /// Feed name, the path segment used to query it (`da_hrl_lmps`).
    pub name: String,
    /// Numeric feed id (a number in the list, a string in metadata; always
    /// presented here as text).
    #[serde(default, deserialize_with = "text_or_number")]
    pub feed_id: Option<String>,
    /// Human-readable title.
    #[serde(default)]
    pub display_name: String,
    /// Description from PJM.
    #[serde(default)]
    pub description: String,
    /// Catalog category (`Locational Marginal Prices`, `Load`, ...).
    #[serde(default)]
    pub category: Option<String>,
    /// Earliest data, when PJM states it.
    #[serde(default)]
    pub first_available: Option<NaiveDateTime>,
    /// How often new data is posted (`Hourly`, `Daily`, ...).
    #[serde(default)]
    pub posting_frequency: Option<String>,
    /// Posting schedule detail.
    #[serde(default)]
    pub posting_day: Option<String>,
    /// How long PJM retains data (`Indefinitely`, `30 days`, ...).
    #[serde(default)]
    pub retention_time: Option<String>,
    /// PJM's own default search, as a JSON string.
    #[serde(default)]
    pub default_search_json: Option<String>,
    /// `Overwrite`, `Version`, `append only`, ...
    #[serde(default)]
    pub file_loading_method: Option<String>,
    /// Links to the feed and its metadata.
    #[serde(default)]
    pub links: Vec<Link>,
}

/// A feed's metadata: summary plus field definitions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedMetadata {
    /// The feed's name, description, schedule and links.
    #[serde(flatten)]
    pub summary: FeedSummary,
    /// When PJM last loaded data (UTC).
    #[serde(default)]
    pub last_data_load: Option<NaiveDateTime>,
    /// When PJM last loaded data, with the Eastern offset.
    #[serde(default)]
    pub last_data_load_est: Option<DateTime<FixedOffset>>,
    /// Whether old data lives in a restricted archive.
    #[serde(default)]
    pub enable_archiving: bool,
    /// Age in days beyond which data is archived.
    #[serde(default)]
    pub archive_cutoff_days: Option<u32>,
    /// Field definitions.
    #[serde(default)]
    pub columns: Vec<Column>,
}

impl FeedMetadata {
    /// The definition of one column.
    #[must_use]
    pub fn column(&self, field_name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.field_name == field_name)
    }

    /// Columns usable as query filters.
    pub fn filterable(&self) -> impl Iterator<Item = &Column> {
        self.columns.iter().filter(|c| c.enable_filtering)
    }

    /// Columns usable in `sort`.
    pub fn sortable(&self) -> impl Iterator<Item = &Column> {
        self.columns.iter().filter(|c| c.enable_sorting)
    }
}

/// One field definition of a feed.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    /// Name used in `fields`, filters and `sort`.
    pub field_name: String,
    /// Display label.
    #[serde(default)]
    pub display_name: String,
    /// `Number`, `String`, `DateTime`, `Date`, `Boolean`.
    #[serde(default)]
    pub data_type: String,
    /// How PJM's UI filters it (`Input`, `Lookup`, `Date`, `SelectorPnode`).
    #[serde(default)]
    pub filter_type: Option<String>,
    /// Field description.
    #[serde(default)]
    pub description: Option<String>,
    /// Position in PJM's default column order.
    #[serde(default)]
    pub ordinal_position: Option<u32>,
    /// Whether the column can be filtered on.
    #[serde(default, deserialize_with = "lenient_bool")]
    pub enable_filtering: bool,
    /// Whether the column can be sorted on.
    #[serde(default, deserialize_with = "lenient_bool")]
    pub enable_sorting: bool,
    /// Whether the column can be filtered on in archived data.
    #[serde(default, deserialize_with = "lenient_bool")]
    pub enable_archive_filtering: bool,
}

/// PJM sends booleans as `true`, `"True"` or omits them.
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Text(String),
    }
    Ok(match Option::<Raw>::deserialize(d)? {
        None => false,
        Some(Raw::Bool(b)) => b,
        Some(Raw::Text(t)) => t.eq_ignore_ascii_case("true"),
    })
}

/// PJM sends ids as numbers in one place and strings in another.
fn text_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct Visitor;
    impl de::Visitor<'_> for Visitor {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a string, a number or null")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_owned()))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    d.deserialize_any(Visitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Page;

    #[test]
    fn feed_list_fixture_decodes() {
        let body = include_str!("../tests/fixtures/feeds_list.json");
        let page: Page<FeedSummary> = serde_json::from_str(body).unwrap();
        assert_eq!(page.total_rows, 119);
        let da = page.items.iter().find(|f| f.name == "da_hrl_lmps").unwrap();
        assert_eq!(da.display_name, "Day-Ahead Hourly LMPs");
        assert_eq!(da.feed_id.as_deref(), Some("6"));
        assert_eq!(da.category.as_deref(), Some("Locational Marginal Prices"));
        assert!(da.first_available.is_some());
    }

    #[test]
    fn metadata_fixture_decodes_columns() {
        let body = include_str!("../tests/fixtures/metadata_da_hrl_lmps.json");
        let md: FeedMetadata = serde_json::from_str(body).unwrap();
        assert_eq!(md.summary.name, "da_hrl_lmps");
        assert_eq!(md.summary.feed_id.as_deref(), Some("6"));
        assert!(md.enable_archiving);
        assert_eq!(md.archive_cutoff_days, Some(731));
        assert!(md.last_data_load.is_some() && md.last_data_load_est.is_some());
        let pnode = md.column("pnode_id").unwrap();
        assert_eq!(pnode.data_type, "Number");
        assert!(pnode.enable_filtering && pnode.enable_sorting);
        let lmp = md.column("total_lmp_da").unwrap();
        assert!(!lmp.enable_filtering);
        assert!(md
            .filterable()
            .any(|c| c.field_name == "datetime_beginning_ept"));
        assert!(md.sortable().any(|c| c.field_name == "zone"));
        assert!(md.column("nope").is_none());
    }

    #[test]
    fn lenient_bool_accepts_text_bool_and_null() {
        let c: Column = serde_json::from_str(
            r#"{"fieldName":"a","enableFiltering":"True","enableSorting":true,"enableArchiveFiltering":null}"#,
        )
        .unwrap();
        assert!(c.enable_filtering && c.enable_sorting && !c.enable_archive_filtering);
    }
}

//! The envelope PJM wraps every feed response in.

use serde::Deserialize;

/// A row of an arbitrary feed: column name to JSON value.
pub type Row = serde_json::Map<String, serde_json::Value>;

/// A hypermedia link in a response (`self`, `next`, `metadata`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Link {
    /// Relation name.
    pub rel: String,
    /// Absolute URL.
    pub href: String,
}

/// One page of feed rows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Page<T> {
    /// Hypermedia links; a `next` link is present when more rows remain.
    #[serde(default)]
    pub links: Vec<Link>,
    /// The rows of this page.
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    /// What PJM understood the query to be (echo of filters, fields, ...).
    #[serde(rename = "searchSpecification", default)]
    pub search_specification: Option<serde_json::Value>,
    /// Rows matching the query across all pages.
    #[serde(rename = "totalRows", default)]
    pub total_rows: u64,
}

impl<T> Page<T> {
    /// URL of the next page, when PJM advertises one.
    #[must_use]
    pub fn next_url(&self) -> Option<&str> {
        self.links
            .iter()
            .find(|l| l.rel == "next")
            .map(|l| l.href.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_envelope_with_and_without_next() {
        let with_next = r#"{"links":[{"rel":"self","href":"a"},{"rel":"next","href":"b"}],
            "items":[{"x":1}],"searchSpecification":{"rowCount":1},"totalRows":9}"#;
        let page: Page<Row> = serde_json::from_str(with_next).unwrap();
        assert_eq!(page.total_rows, 9);
        assert_eq!(page.next_url(), Some("b"));
        assert_eq!(page.items.len(), 1);

        let page: Page<Row> = serde_json::from_str(r#"{"items":[],"totalRows":0}"#).unwrap();
        assert_eq!(page.next_url(), None);
        assert_eq!(page.items.len(), 0);
    }
}

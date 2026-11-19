//! Format-neutral rows: one OASIS CSV line or XML `REPORT_ITEM` as a column map.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};

use crate::error::Error;

/// One row of an OASIS response, keyed by upper-cased column name.
///
/// CSV responses map their header columns directly. XML responses flatten each
/// `REPORT_ITEM`: the fields of its `REPORT_HEADER` and `REPORT_DATA` children
/// become columns (data fields win a name clash).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Record {
    fields: BTreeMap<String, String>,
}

impl Record {
    /// Builds a record from `(column, value)` pairs; names are upper-cased.
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        Self {
            fields: pairs
                .into_iter()
                .map(|(k, v)| (k.as_ref().trim().to_ascii_uppercase(), v.into()))
                .collect(),
        }
    }

    /// The raw value of a column, case-insensitively.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .get(&name.to_ascii_uppercase())
            .map(String::as_str)
    }

    /// The first non-empty value among several candidate column names.
    ///
    /// CSV and XML name the same quantity differently (`INTERVALSTARTTIME_GMT`
    /// vs `INTERVAL_START_GMT`), so typed rows look up an alias list.
    #[must_use]
    pub fn first(&self, names: &[&str]) -> Option<&str> {
        names
            .iter()
            .find_map(|name| self.get(name).filter(|v| !v.is_empty()))
    }

    /// Like [`Record::first`] but a miss is an error.
    ///
    /// # Errors
    ///
    /// [`Error::MissingField`] when none of the columns is present and non-empty.
    pub fn require(&self, names: &[&str]) -> Result<&str, Error> {
        self.first(names).ok_or_else(|| Error::MissingField {
            field: names.join("|"),
        })
    }

    /// A required timestamp (`2026-11-10T08:00:00-00:00` style, RFC 3016.8).
    ///
    /// # Errors
    ///
    /// [`Error::MissingField`] or [`Error::InvalidField`].
    pub fn timestamp(&self, names: &[&str]) -> Result<DateTime<Utc>, Error> {
        parse_timestamp(self.require(names)?, names)
    }

    /// An optional timestamp: absent or empty yields `None`.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidField`] when a value is present but malformed.
    pub fn optional_timestamp(&self, names: &[&str]) -> Result<Option<DateTime<Utc>>, Error> {
        self.first(names)
            .map(|raw| parse_timestamp(raw, names))
            .transpose()
    }

    /// A required finite floating point number.
    ///
    /// # Errors
    ///
    /// [`Error::MissingField`] or [`Error::InvalidField`] (including NaN/inf).
    pub fn number(&self, names: &[&str]) -> Result<f64, Error> {
        let raw = self.require(names)?;
        raw.trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| invalid(names, raw))
    }

    /// An optional calendar date (`2026-11-10`).
    ///
    /// # Errors
    ///
    /// [`Error::InvalidField`] when a value is present but malformed.
    pub fn optional_date(&self, names: &[&str]) -> Result<Option<NaiveDate>, Error> {
        self.first(names)
            .map(|raw| {
                NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| invalid(names, raw))
            })
            .transpose()
    }

    /// An optional non-negative integer.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidField`] when a value is present but malformed.
    pub fn optional_u32(&self, names: &[&str]) -> Result<Option<u32>, Error> {
        self.first(names)
            .map(|raw| raw.trim().parse::<u32>().map_err(|_| invalid(names, raw)))
            .transpose()
    }

    /// All columns in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Number of columns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// True when the record has no columns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

fn invalid(names: &[&str], raw: &str) -> Error {
    Error::InvalidField {
        field: names.join("|"),
        value: raw.to_owned(),
    }
}

fn parse_timestamp(raw: &str, names: &[&str]) -> Result<DateTime<Utc>, Error> {
    let text = raw.trim();
    DateTime::parse_from_rfc3339(text)
        .or_else(|_| DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%z"))
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| invalid(names, raw))
}

/// Conversion from a [`Record`] into a typed row.
///
/// Implement it for your own type to use [`Client::fetch`](crate::Client::fetch)
/// with a report this crate has no typed helper for.
pub trait FromRecord: Sized {
    /// Reads one typed row.
    ///
    /// # Errors
    ///
    /// [`Error::MissingField`] or [`Error::InvalidField`] when the record does
    /// not carry the columns the type needs.
    fn from_record(record: &Record) -> Result<Self, Error>;
}

impl FromRecord for Record {
    fn from_record(record: &Record) -> Result<Self, Error> {
        Ok(record.clone())
    }
}

/// Parses a CSV document (header line plus rows) into records.
///
/// A leading UTF-8 byte-order mark is ignored and short rows are tolerated.
///
/// # Errors
///
/// [`Error::Csv`] when the document cannot be tokenised.
pub fn parse_csv(bytes: &[u8]) -> Result<Vec<Record>, Error> {
    parse_csv_with_header(bytes).map(|(_, records)| records)
}

/// Like [`parse_csv`] but also returns the header names in file order.
///
/// # Errors
///
/// [`Error::Csv`] when the document cannot be tokenised.
pub fn parse_csv_with_header(bytes: &[u8]) -> Result<(Vec<String>, Vec<Record>), Error> {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(body);
    let headers = reader
        .headers()
        .map_err(|e| Error::Csv(e.to_string()))?
        .clone();
    let records = reader
        .records()
        .map(|row| {
            row.map(|fields| Record::from_pairs(headers.iter().zip(fields.iter())))
                .map_err(|e| Error::Csv(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((headers.iter().map(str::to_owned).collect(), records))
}

/// Parses an OASIS XML report into one record per `REPORT_ITEM`.
///
/// Element namespaces are ignored (the schema version is part of the
/// namespace and changes between reports). A document with no `REPORT_ITEM`
/// yields an empty list.
///
/// # Errors
///
/// [`Error::Xml`] when the text is not well-formed XML.
pub fn parse_xml(text: &str) -> Result<Vec<Record>, Error> {
    let doc = roxmltree::Document::parse(text).map_err(|e| Error::Xml(e.to_string()))?;
    Ok(doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "REPORT_ITEM")
        .map(|item| {
            let fields = |section: &'static str| {
                item.children()
                    .filter(move |c| c.is_element() && c.tag_name().name() == section)
                    .flat_map(|s| s.children().filter(roxmltree::Node::is_element))
                    .filter_map(|leaf| {
                        leaf.text()
                            .map(|t| (leaf.tag_name().name().to_owned(), t.trim().to_owned()))
                    })
                    .collect::<Vec<_>>()
            };
            Record::from_pairs(
                fields("REPORT_HEADER")
                    .into_iter()
                    .chain(fields("REPORT_DATA")),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_maps_header_to_values_and_strips_bom() -> Result<(), Error> {
        let rows = parse_csv(b"\xEF\xBB\xBFA,b\n1,2\n3,\n")?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get("a"), Some("1"));
        assert_eq!(rows[0].get("B"), Some("2"));
        assert_eq!(rows[1].first(&["B"]), None, "empty values are skipped");
        Ok(())
    }

    #[test]
    fn csv_tolerates_short_rows() -> Result<(), Error> {
        let rows = parse_csv(b"a,b,c\n1,2\n")?;
        assert_eq!(rows[0].get("c"), None);
        Ok(())
    }

    #[test]
    fn typed_getters_validate() -> Result<(), Error> {
        let r = Record::from_pairs([
            ("T", "2026-11-10T08:00:00-00:00"),
            ("N", "1.5"),
            ("BAD", "NaN"),
            ("D", "2026-11-10"),
        ]);
        assert_eq!(
            r.timestamp(&["T"])?.to_rfc3339(),
            "2026-11-10T08:00:00+00:00"
        );
        assert!((r.number(&["N"])? - 1.5).abs() < f64::EPSILON);
        assert!(matches!(
            r.number(&["BAD"]),
            Err(Error::InvalidField { .. })
        ));
        assert!(matches!(
            r.number(&["NOPE"]),
            Err(Error::MissingField { .. })
        ));
        assert_eq!(
            r.optional_date(&["D"])?.map(|d| d.to_string()).as_deref(),
            Some("2026-11-10")
        );
        assert_eq!(r.optional_date(&["NOPE"])?, None);
        Ok(())
    }

    #[test]
    fn xml_flattens_report_items() -> Result<(), Error> {
        let xml = r#"<R xmlns="urn:x"><RTO><REPORT_ITEM>
            <REPORT_HEADER><UOM>MW</UOM></REPORT_HEADER>
            <REPORT_DATA><DATA_ITEM>X</DATA_ITEM><VALUE> 4 </VALUE></REPORT_DATA>
            </REPORT_ITEM></RTO></R>"#;
        let rows = parse_xml(xml)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("uom"), Some("MW"));
        assert_eq!(rows[0].get("value"), Some("4"));
        Ok(())
    }

    #[test]
    fn xml_rejects_garbage() {
        assert!(matches!(parse_xml("<a><b></a>"), Err(Error::Xml(_))));
    }
}

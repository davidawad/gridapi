//! Generic report pages: column metadata, raw rows and typed cell access.

use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;
pub use serde_json::Value;

use crate::error::{Error, Result};

/// Paging block (`_meta`) of every report response.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    /// Rows matching the filters across all pages.
    pub total_records: u64,
    /// Rows per page the server used.
    pub page_size: u32,
    /// Pages available for this query.
    pub total_pages: u32,
    /// 1-based index of this page (0 when there are no rows).
    pub current_page: u32,
}

impl PageMeta {
    /// True when another page follows this one.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.current_page < self.total_pages
    }
}

/// Identity of the report that produced a page.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportInfo {
    /// Artifact name, e.g. `spp_node_zone_hub`.
    pub report_name: String,
    /// Human-readable title.
    pub report_display_name: String,
    /// EMIL product id, e.g. `NP6-905-CD`.
    #[serde(rename = "reportEMIL")]
    pub report_emil: String,
    /// Maximum rows one bulk download may contain.
    pub download_limit: Option<u64>,
}

/// Value type ERCOT declares for a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum DataType {
    /// `yyyy-MM-dd`.
    #[serde(rename = "DATE")]
    Date,
    /// `yyyy-MM-ddTHH:mm:ss`, Central Prevailing Time.
    #[serde(rename = "DATETIME")]
    DateTime,
    /// JSON integer.
    #[serde(rename = "INTEGER")]
    Integer,
    /// JSON number.
    #[serde(rename = "DOUBLE")]
    Double,
    /// JSON string.
    #[serde(rename = "STRING")]
    String,
    /// JSON boolean.
    #[serde(rename = "BOOLEAN")]
    Boolean,
    /// A type this crate version does not know.
    #[serde(other)]
    Other,
}

/// Column description from a report's `fields` list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Machine name; also the query-parameter stem (`{name}From`, `{name}To`).
    pub name: String,
    /// Display label.
    pub label: String,
    /// 1-based column position.
    pub cardinality: u32,
    /// Declared value type.
    pub data_type: DataType,
    /// Whether the column can be filtered.
    pub searchable: bool,
    /// Whether the column can be used in `sort`.
    pub sortable: bool,
    /// Whether `{name}From` / `{name}To` range filters exist.
    pub has_range: bool,
}

/// One page of a report: column metadata plus untyped rows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReportPage {
    /// Paging block.
    #[serde(rename = "_meta")]
    pub meta: PageMeta,
    /// Which report answered.
    pub report: ReportInfo,
    /// Column descriptions, in row order.
    pub fields: Vec<Field>,
    /// Row values, aligned with `fields`.
    pub data: Vec<Vec<Value>>,
}

impl ReportPage {
    /// Parses a response body.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] when the body is not a report page.
    pub fn from_json(body: &[u8]) -> Result<Self> {
        serde_json::from_slice(body).map_err(|e| Error::decode("report page", e))
    }

    /// Iterates rows with by-name cell access.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.data.iter().map(|values| Row {
            fields: &self.fields,
            values,
        })
    }

    /// Converts every row with `T`'s [`FromRow`] implementation.
    ///
    /// # Errors
    ///
    /// The first row that fails to convert.
    pub fn records<T: FromRow>(&self) -> Result<Vec<T>> {
        self.rows().map(|row| T::from_row(&row)).collect()
    }
}

/// Conversion from one report row into a typed record.
pub trait FromRow: Sized {
    /// Builds the record from `row`.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when a column is missing or has the wrong shape.
    fn from_row(row: &Row<'_>) -> Result<Self>;
}

/// A borrowed row: values plus the column names that label them.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    fields: &'a [Field],
    values: &'a [Value],
}

impl<'a> Row<'a> {
    /// The raw cell for column `name`, if the column exists.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&'a Value> {
        let idx = self.fields.iter().position(|f| f.name == name)?;
        self.values.get(idx)
    }

    fn required(&self, name: &str) -> Result<&'a Value> {
        match self.get(name) {
            None => Err(Error::row(name, "column not present in this report")),
            Some(Value::Null) => Err(Error::row(name, "value is null")),
            Some(v) => Ok(v),
        }
    }

    /// String cell.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not a string.
    pub fn str(&self, name: &str) -> Result<&'a str> {
        self.required(name)?
            .as_str()
            .ok_or_else(|| Error::row(name, "expected a string"))
    }

    /// Floating-point cell; numeric strings are accepted.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not numeric.
    pub fn f64(&self, name: &str) -> Result<f64> {
        number(self.required(name)?).ok_or_else(|| Error::row(name, "expected a number"))
    }

    /// Like [`Row::f64`] but null or missing columns yield `None`.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when the cell is present, non-null and not numeric.
    pub fn opt_f64(&self, name: &str) -> Result<Option<f64>> {
        match self.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => number(v)
                .map(Some)
                .ok_or_else(|| Error::row(name, "expected a number")),
        }
    }

    /// Unsigned 8-bit integer cell.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null, not an integer, or out of range.
    pub fn u8(&self, name: &str) -> Result<u8> {
        self.required(name)?
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| Error::row(name, "expected an integer in 0..=255"))
    }

    /// Boolean cell.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not a boolean.
    pub fn bool(&self, name: &str) -> Result<bool> {
        self.required(name)?
            .as_bool()
            .ok_or_else(|| Error::row(name, "expected a boolean"))
    }

    /// `yyyy-MM-dd` date cell.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not a date.
    pub fn date(&self, name: &str) -> Result<NaiveDate> {
        let text = self.str(name)?;
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map_err(|e| Error::row(name, format!("bad date {text:?}: {e}")))
    }

    /// `yyyy-MM-ddTHH:mm:ss[.fff]` timestamp cell (Central Prevailing Time, no offset).
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not a timestamp.
    pub fn datetime(&self, name: &str) -> Result<NaiveDateTime> {
        let text = self.str(name)?;
        parse_datetime(text).map_err(|e| Error::row(name, format!("bad timestamp {text:?}: {e}")))
    }

    /// Hour-ending cell such as `"01:00"`, `"1:00"` or `"24:00"`.
    ///
    /// # Errors
    ///
    /// [`Error::Row`] when absent, null or not an hour ending.
    pub fn hour_ending(&self, name: &str) -> Result<HourEnding> {
        let text = self.str(name)?;
        text.parse()
            .map_err(|e: Error| Error::row(name, e.to_string()))
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Parses ERCOT timestamps with or without fractional seconds.
pub(crate) fn parse_datetime(text: &str) -> std::result::Result<NaiveDateTime, chrono::ParseError> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
}

/// An hour ending in `1..=24`, as used by day-ahead and hourly reports.
///
/// `24` is the last hour of the operating day (the `24:00` label).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HourEnding(u8);

impl HourEnding {
    /// Validates `hour` is within `1..=24`.
    ///
    /// # Errors
    ///
    /// [`Error::Config`] when out of range.
    pub fn try_new(hour: u8) -> Result<Self> {
        if (1..=24).contains(&hour) {
            Ok(Self(hour))
        } else {
            Err(Error::Config(format!(
                "hour ending {hour} is outside 1..=24"
            )))
        }
    }

    /// The hour number, `1..=24`.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }

    /// The label ERCOT filters on, e.g. `"07:00"`.
    #[must_use]
    pub fn label(self) -> String {
        format!("{:02}:00", self.0)
    }
}

impl std::str::FromStr for HourEnding {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        let hour = text
            .split_once(':')
            .map_or(text, |(h, _)| h)
            .trim()
            .parse::<u8>()
            .map_err(|_| Error::Config(format!("not an hour ending: {text:?}")))?;
        Self::try_new(hour)
    }
}

impl std::fmt::Display for HourEnding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> ReportPage {
        ReportPage::from_json(include_bytes!("../tests/fixtures/dam_spp.json"))
            .expect("fixture parses")
    }

    #[test]
    fn parses_meta_report_and_fields_from_a_real_response() {
        let p = page();
        assert_eq!(p.meta.total_records, 24);
        assert_eq!(p.meta.page_size, 3);
        assert!(p.meta.has_next());
        assert_eq!(p.report.report_emil, "NP4-190-CD");
        assert_eq!(p.fields[3].name, "settlementPointPrice");
        assert_eq!(p.fields[3].data_type, DataType::Double);
        assert!(p.fields[3].has_range);
        assert!(!p.fields[1].has_range, "hourEnding is exact-match only");
    }

    #[test]
    fn row_access_by_name() {
        let p = page();
        let row = p.rows().next().expect("one row");
        assert_eq!(row.str("settlementPoint").expect("str"), "HB_HOUSTON");
        assert_eq!(
            row.date("deliveryDate").expect("date"),
            NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid")
        );
        assert!((row.f64("settlementPointPrice").expect("num") - 50.73).abs() < 1e-9);
        assert!(!row.bool("DSTFlag").expect("bool"));
        assert_eq!(row.hour_ending("hourEnding").expect("he").get(), 1);
    }

    #[test]
    fn missing_column_and_wrong_type_are_row_errors_naming_the_field() {
        let p = page();
        let row = p.rows().next().expect("row");
        let missing = row.f64("nope").expect_err("absent");
        assert!(
            matches!(&missing, Error::Row { field, .. } if field == "nope"),
            "{missing}"
        );
        let wrong = row
            .f64("settlementPoint")
            .expect_err("string is not a number");
        assert!(
            matches!(&wrong, Error::Row { field, .. } if field == "settlementPoint"),
            "{wrong}"
        );
        assert_eq!(row.opt_f64("nope").expect("optional"), None);
    }

    #[test]
    fn datetime_with_and_without_fraction() {
        assert!(parse_datetime("2026-11-17T23:05:18").is_ok());
        assert!(parse_datetime("2026-11-17T23:47:01.000").is_ok());
        assert!(parse_datetime("2026-11-17 23:05:18").is_err());
    }

    #[test]
    fn hour_ending_formats_round_trip() {
        for (text, want) in [("01:00", 1), ("1:00", 1), ("24:00", 24), ("7", 7)] {
            assert_eq!(
                text.parse::<HourEnding>().expect(text).get(),
                want,
                "{text}"
            );
        }
        assert!("00:00".parse::<HourEnding>().is_err());
        assert!("25:00".parse::<HourEnding>().is_err());
        assert!("ab".parse::<HourEnding>().is_err());
        assert_eq!(HourEnding::try_new(7).expect("valid").label(), "07:00");
    }

    #[test]
    fn numeric_strings_parse_as_numbers() {
        assert_eq!(number(&Value::String(" 9490.83 ".into())), Some(9490.83));
        assert_eq!(number(&Value::Bool(true)), None);
    }

    #[test]
    fn unknown_data_type_does_not_fail_the_page() {
        let body = br#"{"_meta":{"totalRecords":0,"pageSize":1,"totalPages":0,"currentPage":0},
          "report":{"reportName":"r","reportDisplayName":"R","reportEMIL":"X","downloadLimit":1},
          "fields":[{"name":"a","label":"A","cardinality":1,"dataType":"GEOMETRY","searchable":true,"sortable":true,"hasRange":false}],
          "data":[]}"#;
        let p = ReportPage::from_json(body).expect("tolerates new types");
        assert_eq!(p.fields[0].data_type, DataType::Other);
    }
}

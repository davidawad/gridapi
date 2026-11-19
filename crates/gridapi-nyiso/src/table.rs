//! A tolerant CSV table reader.
//!
//! NYISO's files drift over time: old headers are cut off mid-parenthesis
//! (`Marginal Cost Congestion ($/MWH`), units change (`Gen MWh` vs `Gen MW`) and
//! some files quote every cell. Columns are therefore matched on a normalised
//! header: the text before the first ` (`, trimmed and lower-cased.

use csv::StringRecord;

use crate::{dataset::Dataset, error::Error};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

fn normalise(header: &str) -> String {
    header
        .split(" (")
        .next()
        .unwrap_or(header)
        .trim()
        .to_ascii_lowercase()
}

/// A parsed CSV file: its header and all of its records.
#[derive(Debug)]
pub(crate) struct Table {
    dataset: Dataset,
    raw_header: Vec<String>,
    header: Vec<String>,
    records: Vec<StringRecord>,
}

impl Table {
    pub(crate) fn read(dataset: Dataset, bytes: &[u8]) -> Result<Self, Error> {
        let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
        let csv_error = |row: usize, source: csv::Error| Error::Csv {
            dataset: dataset.to_string(),
            row,
            source: Box::new(source),
        };
        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(bytes);
        let raw_header: Vec<String> = reader
            .headers()
            .map_err(|e| csv_error(1, e))?
            .iter()
            .map(str::to_owned)
            .collect();
        let header = raw_header.iter().map(|h| normalise(h)).collect();
        let records = reader
            .records()
            .enumerate()
            .map(|(i, r)| r.map_err(|e| csv_error(i + 2, e)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            dataset,
            raw_header,
            header,
            records,
        })
    }

    /// The header as published, joined for error messages.
    pub(crate) fn header_text(&self) -> String {
        self.raw_header.join(",")
    }

    /// The published (un-normalised) name of column `index`.
    pub(crate) fn raw_name(&self, index: usize) -> &str {
        self.raw_header.get(index).map_or("", String::as_str)
    }

    /// Index of the first column whose normalised name is one of `aliases`.
    pub(crate) fn find(&self, aliases: &[&str]) -> Option<usize> {
        self.header
            .iter()
            .position(|h| aliases.iter().any(|a| h == a))
    }

    /// Like [`Table::find`], but a missing column is an error.
    pub(crate) fn column(&self, aliases: &[&str]) -> Result<usize, Error> {
        self.find(aliases).ok_or_else(|| Error::MissingColumn {
            dataset: self.dataset.to_string(),
            column: aliases.first().copied().unwrap_or_default().to_owned(),
            header: self.header_text(),
        })
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.records.iter().enumerate().map(|(i, record)| Row {
            table: self,
            record,
            number: i + 2,
        })
    }

    /// Number of header columns.
    pub(crate) fn width(&self) -> usize {
        self.raw_header.len()
    }

    /// Number of data records.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }
}

/// One data row with cell accessors that report where a bad cell is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row<'t> {
    table: &'t Table,
    record: &'t StringRecord,
    number: usize,
}

impl<'t> Row<'t> {
    pub(crate) fn text(&self, column: usize) -> &'t str {
        self.record.get(column).unwrap_or("")
    }

    fn bad_cell(&self, column: usize) -> Error {
        Error::BadCell {
            dataset: self.table.dataset.to_string(),
            row: self.number,
            column: self.table.raw_name(column).to_owned(),
            value: self.text(column).to_owned(),
        }
    }

    /// A required floating point cell.
    pub(crate) fn float(&self, column: usize) -> Result<f64, Error> {
        self.text(column)
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| self.bad_cell(column))
    }

    /// A floating point cell that may be blank.
    pub(crate) fn opt_float(&self, column: usize) -> Result<Option<f64>, Error> {
        match self.text(column) {
            "" => Ok(None),
            _ => self.float(column).map(Some),
        }
    }

    /// A required unsigned integer cell.
    pub(crate) fn uint(&self, column: usize) -> Result<u32, Error> {
        self.text(column)
            .parse::<u32>()
            .map_err(|_| self.bad_cell(column))
    }

    pub(crate) fn dataset(&self) -> Dataset {
        self.table.dataset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::Level;

    const DATASET: Dataset = Dataset::DayAheadLbmp(Level::Zone);

    #[test]
    fn normalises_truncated_and_unit_suffixed_headers() {
        assert_eq!(
            normalise("Marginal Cost Congestion ($/MWH"),
            "marginal cost congestion"
        );
        assert_eq!(normalise("LBMP ($/MWHr)"), "lbmp");
        assert_eq!(normalise(" Time Stamp "), "time stamp");
        assert_eq!(normalise("Gen MWh"), "gen mwh");
    }

    #[test]
    fn reads_quoted_and_unquoted_cells_and_a_bom() {
        let csv = "\u{feff}\"Time Stamp\",Name,PTID\n\"11/17/2026 00:00\", CAPITL ,61757\n";
        let table = Table::read(DATASET, csv.as_bytes()).expect("parses");
        assert_eq!(table.len(), 1);
        let name = table.column(&["name"]).expect("name column");
        let row = table.rows().next().expect("one row");
        assert_eq!(row.text(name), "CAPITL", "cells are trimmed");
    }

    #[test]
    fn missing_column_names_the_header() {
        let table = Table::read(DATASET, b"a,b\n1,2\n").expect("parses");
        let err = table.column(&["lbmp"]).expect_err("no lbmp column");
        assert!(
            matches!(&err, Error::MissingColumn { column, header, .. } if column == "lbmp" && header == "a,b"),
            "got {err:?}"
        );
    }

    #[test]
    fn bad_cells_report_row_and_column() {
        let table = Table::read(DATASET, b"Name,LBMP ($/MWHr)\nA,1.5\nB,oops\n").expect("parses");
        let lbmp = table.column(&["lbmp"]).expect("lbmp");
        let results: Vec<_> = table.rows().map(|r| r.float(lbmp)).collect();
        assert_eq!(results[0].as_ref().ok(), Some(&1.5));
        let err = results[1].as_ref().expect_err("oops is not a number");
        assert!(
            matches!(err, Error::BadCell { row: 3, column, value, .. } if column == "LBMP ($/MWHr)" && value == "oops"),
            "got {err:?}"
        );
    }

    #[test]
    fn optional_floats_allow_blank_but_not_nan() {
        let table = Table::read(DATASET, b"x\n\nnan\n7\n").expect("parses");
        let rows: Vec<_> = table.rows().map(|r| r.opt_float(0)).collect();
        // csv skips fully empty lines, so the first record is "nan".
        assert!(rows[0].is_err(), "NaN is rejected");
        assert_eq!(rows[1].as_ref().ok(), Some(&Some(7.0)));
    }
}

//! Parser for ISO Express CSV reports.
//!
//! Every report is a CSV whose first cell tags the line: `C` comment, `H` header
//! (a names line, then a units line), `D` data, `T` trailer (`"N lines"`, the
//! number of data rows). Cells may or may not be quoted, and the tag itself is
//! quoted in some reports and not in others.

use csv::StringRecord;

use crate::error::Error;

/// The body ISO Express returns, with HTTP 200, for a day it has no file for.
const NO_DATA_BODY: &str = "No data exists for this period.";

/// Whether `body` is the "no such file" answer rather than a report.
pub(crate) fn is_no_data(body: &[u8]) -> bool {
    body.len() < 200 && std::str::from_utf8(body).is_ok_and(|text| text.trim() == NO_DATA_BODY)
}

fn normalise(header: &str) -> String {
    header.trim().to_ascii_lowercase()
}

/// A parsed report: its column names and data rows.
#[derive(Debug)]
pub(crate) struct Report {
    name: String,
    raw_header: Vec<String>,
    header: Vec<String>,
    rows: Vec<(usize, StringRecord)>,
}

/// The line tag of a raw line, without quotes.
fn tag(line: &str) -> &str {
    let first = line.split(',').next().unwrap_or("");
    first.trim().trim_matches('"')
}

/// The leading integer of a trailer such as `28968 lines`.
fn trailer_count(record: &StringRecord) -> Option<usize> {
    record.get(1)?.split_whitespace().next()?.parse().ok()
}

impl Report {
    /// Parse `bytes`, check the trailer against the rows received, and index the
    /// header. `name` labels errors.
    pub(crate) fn parse(name: &str, bytes: &[u8]) -> Result<Self, Error> {
        let text = String::from_utf8_lossy(bytes);
        let mut body = String::with_capacity(text.len());
        let mut line_numbers = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if !matches!(tag(line), "C" | "") {
                body.push_str(line);
                body.push('\n');
                line_numbers.push(index + 1);
            }
        }
        let csv_error = |row: usize, source: csv::Error| Error::Csv {
            report: name.to_owned(),
            row,
            source: Box::new(source),
        };
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(body.as_bytes());

        let mut names: Vec<Vec<String>> = Vec::new();
        let mut rows = Vec::new();
        let mut trailer = None;
        for (i, record) in reader.records().enumerate() {
            let line = line_numbers.get(i).copied().unwrap_or(i + 1);
            let record = record.map_err(|e| csv_error(line, e))?;
            match record.get(0).unwrap_or("") {
                "H" if names.len() < 2 => {
                    names.push(record.iter().skip(1).map(str::to_owned).collect());
                }
                "D" => rows.push((
                    line,
                    StringRecord::from(record.iter().skip(1).collect::<Vec<_>>()),
                )),
                "T" => trailer = trailer_count(&record),
                _ => {}
            }
        }
        if let Some(expected) = trailer {
            if expected != rows.len() {
                return Err(Error::Truncated {
                    report: name.to_owned(),
                    expected,
                    found: rows.len(),
                });
            }
        }
        // Some reports leave a name blank on the first header line and give it on the second.
        let first = names.first().cloned().unwrap_or_default();
        let second = names.get(1).cloned().unwrap_or_default();
        let raw_header: Vec<String> = first
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if n.is_empty() {
                    second.get(i).cloned().unwrap_or_default()
                } else {
                    n.clone()
                }
            })
            .collect();
        let header = raw_header.iter().map(|h| normalise(h)).collect();
        Ok(Self {
            name: name.to_owned(),
            raw_header,
            header,
            rows,
        })
    }

    /// The header as published, joined for error messages.
    pub(crate) fn header_text(&self) -> String {
        self.raw_header.join(",")
    }

    /// Number of data rows in the file.
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// Index of the first column whose normalised name is one of `aliases`.
    pub(crate) fn find(&self, aliases: &[&str]) -> Option<usize> {
        self.header
            .iter()
            .position(|h| aliases.iter().any(|a| h == a))
    }

    /// Like [`Report::find`], but a missing column is an error.
    pub(crate) fn column(&self, aliases: &[&str]) -> Result<usize, Error> {
        self.find(aliases).ok_or_else(|| Error::MissingColumn {
            report: self.name.clone(),
            column: aliases.first().copied().unwrap_or_default().to_owned(),
            header: self.header_text(),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.rows.iter().map(|(line, record)| Row {
            report: self,
            record,
            line: *line,
        })
    }
}

/// One data row with cell accessors that report where a bad cell is.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row<'r> {
    report: &'r Report,
    record: &'r StringRecord,
    line: usize,
}

impl<'r> Row<'r> {
    pub(crate) fn text(&self, column: usize) -> &'r str {
        self.record.get(column).unwrap_or("")
    }

    pub(crate) fn report_name(&self) -> &'r str {
        self.report.name()
    }

    fn bad_cell(&self, column: usize) -> Error {
        Error::BadCell {
            report: self.report.name.clone(),
            row: self.line,
            column: self
                .report
                .raw_header
                .get(column)
                .cloned()
                .unwrap_or_default(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTED: &str = "\"C\",\"Day-Ahead Energy Market Hourly LMP Report\"\n\
\"C\",\"Report for: 11/17/2026 - 11/17/2026\"\n\
\"H\",\"Date\",\"Hour Ending\",\"Location ID\",\"Locational Marginal Price\"\n\
\"H\",\"Date\",\"HE\",\"String\",\"Number\"\n\
\"D\",\"11/17/2026\",\"01\",\"321\",45.94\n\
\"D\",\"11/17/2026\",\"02\",\"321\",49.5\n\
\"T\",\"2 lines\"\n";

    const UNQUOTED: &str = "C,Five-Minute prelim Locational Marginal Prices\n\
C,Report generated 2026-11-18 01:19:52 EST\n\
H,Location ID,Local Time,LMP\n\
H,Label,Time,$/MWh\n\
D,321,2026-11-18 01:15:00,49.74\n\
T,1 lines\n";

    #[test]
    fn parses_quoted_reports() {
        let r = Report::parse("da-lmp", QUOTED.as_bytes()).expect("parses");
        assert_eq!(r.len(), 2);
        let id = r.column(&["location id"]).expect("id");
        let lmp = r.column(&["locational marginal price"]).expect("lmp");
        let row = r.rows().next().expect("row");
        assert_eq!(row.uint(id).expect("uint"), 321);
        assert!((row.float(lmp).expect("float") - 45.94).abs() < 1e-9);
    }

    #[test]
    fn parses_unquoted_reports_and_reports_line_numbers() {
        let r = Report::parse("5min", UNQUOTED.as_bytes()).expect("parses");
        assert_eq!(r.len(), 1);
        let lmp = r.column(&["lmp"]).expect("lmp");
        assert!((r.rows().next().expect("row").float(lmp).expect("float") - 49.74).abs() < 1e-9);
    }

    #[test]
    fn a_trailer_that_disagrees_means_a_truncated_download() {
        let cut = QUOTED.replace("\"D\",\"11/17/2026\",\"02\",\"321\",49.5\n", "");
        let err = Report::parse("da-lmp", cut.as_bytes()).expect_err("1 row, trailer says 2");
        assert!(
            matches!(
                err,
                Error::Truncated {
                    expected: 2,
                    found: 1,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn reports_without_a_trailer_are_accepted() {
        let no_trailer = UNQUOTED.replace("T,1 lines\n", "");
        assert_eq!(
            Report::parse("5min", no_trailer.as_bytes())
                .expect("parses")
                .len(),
            1
        );
    }

    #[test]
    fn blank_first_header_names_are_filled_from_the_second_header_line() {
        let text = "C,Hourly Reserve Requirements\nH,,,Location ID,Ten-Minute Spinning\nH,Date,Hour Ending,Label,MW\nD,2026-11-17,01,7000,343\nT,1 lines\n";
        let r = Report::parse("req", text.as_bytes()).expect("parses");
        assert_eq!(r.column(&["date"]).expect("date"), 0);
        assert_eq!(r.column(&["hour ending"]).expect("he"), 1);
        assert_eq!(r.column(&["location id"]).expect("id"), 2);
        assert_eq!(r.column(&["ten-minute spinning"]).expect("spin"), 3);
    }

    #[test]
    fn a_zero_row_report_parses_to_empty() {
        let text = "\"C\",\"Dispatch Fuel Mix Report\"\n\"H\",\"Date\",\"Time\"\n\"H\",\"Local Date\",\"Local Time\"\n\"T\",\"0 lines\"\n";
        let r = Report::parse("fuel", text.as_bytes()).expect("parses");
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn crlf_line_endings_are_fine() {
        let text = UNQUOTED.replace('\n', "\r\n");
        assert_eq!(
            Report::parse("5min", text.as_bytes())
                .expect("parses")
                .len(),
            1
        );
    }

    #[test]
    fn missing_column_names_the_header() {
        let r = Report::parse("5min", UNQUOTED.as_bytes()).expect("parses");
        let err = r.column(&["nope"]).expect_err("absent");
        assert!(
            matches!(&err, Error::MissingColumn { column, header, .. } if column == "nope" && header.starts_with("Location ID")),
            "got {err:?}"
        );
    }

    #[test]
    fn bad_cells_report_the_original_line_and_column() {
        let text = UNQUOTED.replace("49.74", "oops");
        let r = Report::parse("5min", text.as_bytes()).expect("parses");
        let lmp = r.column(&["lmp"]).expect("lmp");
        let err = r.rows().next().expect("row").float(lmp).expect_err("oops");
        assert!(
            matches!(&err, Error::BadCell { row: 5, column, value, .. } if column == "LMP" && value == "oops"),
            "got {err:?}"
        );
    }

    #[test]
    fn nan_and_infinity_are_rejected() {
        let r =
            Report::parse("x", b"H,V\nH,u\nD,NaN\nD,inf\nD,\nD,7\nT,4 lines\n").expect("parses");
        let results: Vec<_> = r.rows().map(|row| row.opt_float(0)).collect();
        assert!(results[0].is_err(), "NaN rejected");
        assert!(results[1].is_err(), "inf rejected");
        assert_eq!(results[2].as_ref().ok(), Some(&None), "blank is None");
        assert_eq!(results[3].as_ref().ok(), Some(&Some(7.0)));
    }

    #[test]
    fn recognises_the_no_data_body() {
        assert!(is_no_data(b"No data exists for this period."));
        assert!(is_no_data(b"No data exists for this period.\r\n"));
        assert!(!is_no_data(QUOTED.as_bytes()));
        assert!(!is_no_data(b""));
    }
}

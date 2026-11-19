//! CAISO "Today's Outlook" CSV feeds (`caiso.com/outlook/current/*.csv`).
//!
//! Each file is one Pacific-time day at 5-minute resolution with a `Time`
//! column (`HH:MM`, no date) and one column per series. Future slots are
//! present with empty cells until they are measured.

use chrono::{DateTime, Days, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::America::Los_Angeles;

use crate::error::Error;
use crate::record::{parse_csv_with_header, Record};

/// One of the Outlook CSV feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OutlookDataset {
    /// Generation by fuel source in MW (`fuelsource.csv`).
    FuelSource,
    /// Demand with day-ahead and hour-ahead forecasts (`demand.csv`).
    Demand,
    /// Net demand (demand minus wind and solar) and forecast (`netdemand.csv`).
    NetDemand,
    /// Battery charge/discharge in MW (`storage.csv`).
    Storage,
    /// Estimated CO2 emissions by source (`co2.csv`).
    Co2,
}

impl OutlookDataset {
    /// The file stem on caiso.com.
    #[must_use]
    pub fn file_stem(self) -> &'static str {
        match self {
            Self::FuelSource => "fuelsource",
            Self::Demand => "demand",
            Self::NetDemand => "netdemand",
            Self::Storage => "storage",
            Self::Co2 => "co2",
        }
    }
}

/// One 5-minute row of an Outlook feed.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlookPoint {
    /// The Pacific wall-clock date of the row.
    pub local_date: NaiveDate,
    /// The Pacific wall-clock time of the row.
    pub local_time: NaiveTime,
    /// The instant in UTC, or `None` for a skipped or repeated DST hour.
    pub time_utc: Option<DateTime<Utc>>,
    /// One cell per column of [`OutlookSeries::columns`]; `None` when empty.
    pub values: Vec<Option<f64>>,
}

/// A parsed Outlook feed for one Pacific day.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlookSeries {
    /// Which feed this is.
    pub dataset: OutlookDataset,
    /// The Pacific day the file covers.
    pub date: NaiveDate,
    /// Series names in file order (the `Time` column excluded).
    pub columns: Vec<String>,
    /// The rows in file order.
    pub points: Vec<OutlookPoint>,
}

impl OutlookSeries {
    /// Index of a series, matched case-insensitively.
    #[must_use]
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns
            .iter()
            .position(|c| c.eq_ignore_ascii_case(name))
    }

    /// The value of a named series in a given row.
    #[must_use]
    pub fn value(&self, point: &OutlookPoint, column: &str) -> Option<f64> {
        self.column_index(column)
            .and_then(|i| point.values.get(i).copied().flatten())
    }

    /// The last row with any value in any column.
    ///
    /// Forecast columns (`demand`, `netdemand`) are filled through the trailing
    /// midnight row, so for those feeds use [`OutlookSeries::latest_in`] with the
    /// measured column (for example `Current demand`).
    #[must_use]
    pub fn latest(&self) -> Option<&OutlookPoint> {
        self.points
            .iter()
            .rev()
            .find(|p| p.values.iter().any(Option::is_some))
    }

    /// The last row where the named column has a value.
    #[must_use]
    pub fn latest_in(&self, column: &str) -> Option<&OutlookPoint> {
        let index = self.column_index(column)?;
        self.points
            .iter()
            .rev()
            .find(|p| p.values.get(index).copied().flatten().is_some())
    }

    /// One series as `(time, value)` pairs, skipping unmeasured and DST-ambiguous rows.
    #[must_use]
    pub fn series(&self, column: &str) -> Vec<(DateTime<Utc>, f64)> {
        self.column_index(column)
            .map(|i| {
                self.points
                    .iter()
                    .filter_map(|p| Some((p.time_utc?, p.values.get(i).copied().flatten()?)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Resolves a Pacific wall-clock time to UTC; `None` when it does not exist or is ambiguous.
fn pacific_to_utc(local: NaiveDateTime) -> Option<DateTime<Utc>> {
    Los_Angeles
        .from_local_datetime(&local)
        .single()
        .map(|t| t.with_timezone(&Utc))
}

fn parse_clock(raw: &str) -> Result<NaiveTime, Error> {
    NaiveTime::parse_from_str(raw.trim(), "%H:%M").map_err(|_| Error::InvalidField {
        field: "Time".into(),
        value: raw.to_owned(),
    })
}

fn parse_cell(raw: &str, column: &str) -> Result<Option<f64>, Error> {
    let text = raw.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or_else(|| Error::InvalidField {
            field: column.to_owned(),
            value: raw.to_owned(),
        })
}

/// Parses an Outlook CSV for the Pacific day `date`.
///
/// The files end with a `00:00` row that belongs to the following midnight; the
/// date rolls forward whenever the clock time goes backwards.
///
/// # Errors
///
/// [`Error::Csv`] for malformed CSV, [`Error::MissingField`] when there is no
/// `Time` column, [`Error::InvalidField`] for an unparseable time or cell.
pub fn parse_outlook_csv(
    dataset: OutlookDataset,
    date: NaiveDate,
    bytes: &[u8],
) -> Result<OutlookSeries, Error> {
    // Column order comes from the header, which `Record` (a map) does not keep.
    let (header, records) = parse_csv_with_header(bytes)?;
    let columns: Vec<String> = header
        .into_iter()
        .filter(|c| !c.eq_ignore_ascii_case("time"))
        .collect();

    let rows = records
        .iter()
        .map(|record| decode_row(record, &columns))
        .collect::<Result<Vec<_>, _>>()?;

    let (points, _, _) = rows.into_iter().fold(
        (Vec::new(), date, None::<NaiveTime>),
        |(mut points, day, prev), (time, values)| {
            let day = match prev {
                Some(p) if time < p => day.checked_add_days(Days::new(1)).unwrap_or(day),
                _ => day,
            };
            points.push(OutlookPoint {
                local_date: day,
                local_time: time,
                time_utc: pacific_to_utc(day.and_time(time)),
                values,
            });
            (points, day, Some(time))
        },
    );
    Ok(OutlookSeries {
        dataset,
        date,
        columns,
        points,
    })
}

fn decode_row(record: &Record, columns: &[String]) -> Result<(NaiveTime, Vec<Option<f64>>), Error> {
    let time = parse_clock(record.require(&["TIME"])?)?;
    let values = columns
        .iter()
        .map(|column| parse_cell(record.get(column).unwrap_or_default(), column))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((time, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 17).unwrap_or_default()
    }

    #[test]
    fn parses_columns_values_and_blanks() -> Result<(), Error> {
        let csv = b"Time,Day ahead forecast,Current demand,Demand response\n00:00,13703,14705,\n00:05,27083,,\n";
        let s = parse_outlook_csv(OutlookDataset::Demand, day(), csv)?;
        assert_eq!(
            s.columns,
            ["Day ahead forecast", "Current demand", "Demand response"]
        );
        assert_eq!(s.points.len(), 2);
        assert_eq!(s.value(&s.points[0], "current demand"), Some(14705.0));
        assert_eq!(s.value(&s.points[1], "Current demand"), None);
        assert_eq!(
            s.latest().map(|p| p.local_time.to_string()).as_deref(),
            Some("00:05:00")
        );
        Ok(())
    }

    #[test]
    fn latest_in_skips_rows_where_the_column_is_blank() -> Result<(), Error> {
        let csv = b"Time,Forecast,Actual\n00:00,5,1\n00:05,6,2\n00:10,7,\n";
        let s = parse_outlook_csv(OutlookDataset::Demand, day(), csv)?;
        assert_eq!(
            s.latest().map(|p| p.local_time.to_string()).as_deref(),
            Some("00:10:00")
        );
        assert_eq!(
            s.latest_in("actual")
                .map(|p| p.local_time.to_string())
                .as_deref(),
            Some("00:05:00")
        );
        assert!(s.latest_in("missing").is_none());
        Ok(())
    }

    #[test]
    fn pacific_time_converts_to_utc() -> Result<(), Error> {
        // 2026-11-17 is Pacific standard time (UTC-8).
        let s = parse_outlook_csv(
            OutlookDataset::Storage,
            day(),
            b"Time,Total batteries\n13:05,10\n",
        )?;
        let t = s.points[0].time_utc.map(|t| t.to_rfc3339());
        assert_eq!(t.as_deref(), Some("2026-11-17T21:05:00+00:00"));
        Ok(())
    }

    #[test]
    fn trailing_midnight_row_rolls_the_date() -> Result<(), Error> {
        let s = parse_outlook_csv(OutlookDataset::Demand, day(), b"Time,A\n23:55,1\n00:00,2\n")?;
        assert_eq!(s.points[0].local_date, day());
        assert_eq!(s.points[1].local_date, day().succ_opt().unwrap_or(day()));
        Ok(())
    }

    #[test]
    fn dst_gap_has_no_utc_time() -> Result<(), Error> {
        // 2026-03-08 02:30 does not exist in America/Los_Angeles.
        let d = NaiveDate::from_ymd_opt(2026, 3, 8).unwrap_or_default();
        let s = parse_outlook_csv(OutlookDataset::Demand, d, b"Time,A\n02:30,1\n")?;
        assert_eq!(s.points[0].time_utc, None);
        assert!(s.series("A").is_empty());
        Ok(())
    }

    #[test]
    fn bad_cell_and_bad_time_are_errors() {
        let bad_cell = parse_outlook_csv(OutlookDataset::Demand, day(), b"Time,A\n00:00,abc\n");
        assert!(matches!(bad_cell, Err(Error::InvalidField { .. })));
        let bad_time = parse_outlook_csv(OutlookDataset::Demand, day(), b"Time,A\nnoon,1\n");
        assert!(matches!(bad_time, Err(Error::InvalidField { .. })));
        let no_time = parse_outlook_csv(OutlookDataset::Demand, day(), b"A\n1\n");
        assert!(matches!(no_time, Err(Error::MissingField { .. })));
    }
}

//! ISO New England publishes wall-clock Eastern time with no zone column.
//! Hourly reports label hours "hour ending" (`01`..`24`, plus `02X` for the
//! repeated hour on the fall-back day); finer reports give local clock times
//! that repeat on the fall-back day. Every timestamp is resolved to an
//! unambiguous instant with its UTC offset.

use std::{collections::HashMap, hash::Hash};

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::America::New_York;

use crate::error::Error;

/// Formats ISO-NE uses for date-times, most specific first.
const DATE_TIME_FORMATS: [&str; 3] = ["%m/%d/%Y %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%m/%d/%Y %H:%M"];
/// Formats ISO-NE uses for a day.
const DATE_FORMATS: [&str; 2] = ["%m/%d/%Y", "%Y-%m-%d"];
/// Formats ISO-NE uses for a time of day.
const TIME_FORMATS: [&str; 2] = ["%H:%M:%S", "%H:%M"];

/// Parse a published day (`MM/DD/YYYY` or `YYYY-MM-DD`).
pub(crate) fn parse_date(report: &str, cell: &str) -> Result<NaiveDate, Error> {
    let cell = cell.trim();
    DATE_FORMATS
        .iter()
        .find_map(|fmt| NaiveDate::parse_from_str(cell, fmt).ok())
        .ok_or_else(|| timestamp_error(report, cell))
}

/// Parse a published date-time cell.
pub(crate) fn parse_date_time(report: &str, cell: &str) -> Result<NaiveDateTime, Error> {
    let cell = cell.trim();
    DATE_TIME_FORMATS
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(cell, fmt).ok())
        .ok_or_else(|| timestamp_error(report, cell))
}

/// Combine a day with a published time-of-day cell.
pub(crate) fn on_day(report: &str, day: NaiveDate, cell: &str) -> Result<NaiveDateTime, Error> {
    let cell = cell.trim();
    TIME_FORMATS
        .iter()
        .find_map(|fmt| chrono::NaiveTime::parse_from_str(cell, fmt).ok())
        .map(|t| day.and_time(t))
        .ok_or_else(|| timestamp_error(report, cell))
}

fn timestamp_error(report: &str, cell: &str) -> Error {
    Error::Timestamp {
        report: report.to_owned(),
        value: cell.to_owned(),
    }
}

/// The instant an "hour ending" interval begins.
///
/// `label` is `01`..`24`, optionally suffixed `X` for the second occurrence of
/// the repeated hour on the fall-back day. The interval begins at local clock
/// time `HE - 1`. On the fall-back day `02` is the first 01:00 (EDT) and `02X`
/// the second (EST); on the spring-forward day there is no `03`.
pub(crate) fn hour_ending_start(
    day: NaiveDate,
    label: &str,
) -> Result<DateTime<FixedOffset>, Error> {
    let bad = || Error::HourEnding {
        value: label.to_owned(),
        day,
    };
    let label = label.trim();
    let (digits, second_pass) = label
        .strip_suffix(['X', 'x'])
        .map_or((label, false), |d| (d, true));
    let hour_ending: u32 = digits.parse().map_err(|_| bad())?;
    let local = hour_ending
        .checked_sub(1)
        .filter(|h| *h < 24)
        .and_then(|h| day.and_hms_opt(h, 0, 0))
        .ok_or_else(bad)?;
    match New_York.from_local_datetime(&local) {
        LocalResult::Single(t) if !second_pass => Ok(t.fixed_offset()),
        LocalResult::Ambiguous(first, second) => {
            Ok(if second_pass { second } else { first }.fixed_offset())
        }
        LocalResult::Single(_) => Err(bad()),
        LocalResult::None => Err(Error::NonexistentLocalTime(local)),
    }
}

/// Resolves wall-clock times from reports with no zone column.
///
/// On the fall-back day the repeated hour appears twice, first as EDT and then
/// as EST. Rows of one series are in time order, so a repeated local time that
/// is not later than the series' previous instant must be the second occurrence.
/// The series key (location id, fuel category, ...) keeps interleaved series apart.
#[derive(Debug)]
pub(crate) struct EasternClock<K> {
    last: HashMap<K, DateTime<FixedOffset>>,
}

impl<K> Default for EasternClock<K> {
    fn default() -> Self {
        Self {
            last: HashMap::new(),
        }
    }
}

impl<K: Hash + Eq> EasternClock<K> {
    pub(crate) fn resolve(
        &mut self,
        series: K,
        local: NaiveDateTime,
    ) -> Result<DateTime<FixedOffset>, Error> {
        let previous = self.last.get(&series).copied();
        let instant = match New_York.from_local_datetime(&local) {
            LocalResult::Single(t) => t,
            LocalResult::Ambiguous(first, second) => match previous {
                Some(prev) if prev >= first => second,
                _ => first,
            },
            LocalResult::None => return Err(Error::NonexistentLocalTime(local)),
        }
        .fixed_offset();
        self.last.insert(series, instant);
        Ok(instant)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Timelike, Utc};

    use super::*;

    fn day(year: i32, month: u32, dom: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, dom).expect("valid test date")
    }

    fn at(year: i32, month: u32, dom: u32, hour: u32, minute: u32, second: u32) -> NaiveDateTime {
        day(year, month, dom)
            .and_hms_opt(hour, minute, second)
            .expect("valid test time")
    }

    #[test]
    fn parses_published_dates_and_times() {
        assert_eq!(
            parse_date("t", "11/17/2026").expect("us"),
            day(2026, 11, 17)
        );
        assert_eq!(
            parse_date("t", "2026-11-17").expect("iso"),
            day(2026, 11, 17)
        );
        assert_eq!(
            parse_date_time("t", "11/17/2026 00:05:00").expect("us"),
            at(2026, 11, 17, 0, 5, 0)
        );
        assert_eq!(
            parse_date_time("t", "2026-11-18 01:15:00").expect("iso"),
            at(2026, 11, 18, 1, 15, 0)
        );
        assert_eq!(
            on_day("t", day(2026, 11, 17), "20:00:00").expect("time"),
            at(2026, 11, 17, 20, 0, 0)
        );
        assert!(parse_date("t", "17 Nov 2026").is_err());
    }

    #[test]
    fn hour_ending_begins_one_hour_earlier() {
        let first = hour_ending_start(day(2026, 11, 17), "01").expect("HE01");
        assert_eq!(first.to_rfc3339(), "2026-11-17T00:00:00-05:00");
        let last = hour_ending_start(day(2026, 11, 17), "24").expect("HE24");
        assert_eq!(last.to_rfc3339(), "2026-11-17T23:00:00-05:00");
        let summer = hour_ending_start(day(2026, 7, 1), "12").expect("HE12");
        assert_eq!(summer.to_rfc3339(), "2026-07-01T11:00:00-04:00");
    }

    #[test]
    fn fall_back_day_has_25_distinct_hours_with_the_x_suffix() {
        let d = day(2026, 11, 1);
        let labels: Vec<String> = (1..=24)
            .flat_map(|h| {
                let base = format!("{h:02}");
                if h == 2 {
                    vec![base, "02X".to_owned()]
                } else {
                    vec![base]
                }
            })
            .collect();
        let starts: Vec<_> = labels
            .iter()
            .map(|l| {
                hour_ending_start(d, l)
                    .expect("resolves")
                    .with_timezone(&Utc)
            })
            .collect();
        assert_eq!(starts.len(), 25);
        assert!(
            starts
                .windows(2)
                .all(|w| w[1] - w[0] == chrono::Duration::hours(1)),
            "consecutive hours, none skipped or repeated: {starts:?}"
        );
        let he02 = hour_ending_start(d, "02").expect("02");
        let he02x = hour_ending_start(d, "02X").expect("02X");
        assert_eq!(he02.offset().local_minus_utc(), -4 * 3600, "first pass EDT");
        assert_eq!(
            he02x.offset().local_minus_utc(),
            -5 * 3600,
            "second pass EST"
        );
    }

    #[test]
    fn spring_forward_day_skips_he03() {
        let d = day(2026, 3, 8);
        let he02 = hour_ending_start(d, "02").expect("02");
        let he04 = hour_ending_start(d, "04").expect("04");
        assert_eq!(
            he04.with_timezone(&Utc) - he02.with_timezone(&Utc),
            chrono::Duration::hours(1)
        );
        let err = hour_ending_start(d, "03").expect_err("03:00-04:00 hour 02:00 never existed");
        assert!(matches!(err, Error::NonexistentLocalTime(_)), "got {err:?}");
    }

    #[test]
    fn rejects_bad_hour_labels() {
        let d = day(2026, 11, 17);
        for bad in ["", "00", "25", "1X", "ab", "05X", "-1"] {
            assert!(
                hour_ending_start(d, bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn x_suffix_outside_a_repeated_hour_is_rejected() {
        let err = hour_ending_start(day(2026, 11, 17), "02X").expect_err("not a fall-back day");
        assert!(matches!(err, Error::HourEnding { .. }), "got {err:?}");
    }

    #[test]
    fn five_minute_series_stay_monotonic_across_the_repeated_hour() {
        let mut clock = EasternClock::default();
        let d = day(2026, 11, 1);
        let hour = (0..12).map(|i| d.and_hms_opt(1, i * 5, 0).expect("time"));
        let wall: Vec<_> = hour
            .clone()
            .chain(hour)
            .chain(std::iter::once(d.and_hms_opt(2, 0, 0).expect("time")))
            .collect();
        let resolved: Vec<_> = wall
            .into_iter()
            .map(|t| clock.resolve(4000_u32, t).expect("resolves"))
            .collect();
        assert_eq!(resolved.len(), 25);
        assert!(
            resolved.windows(2).all(|w| w[0] < w[1]),
            "strictly increasing: {resolved:?}"
        );
        assert_eq!(resolved[0].offset().local_minus_utc(), -4 * 3600);
        assert_eq!(resolved[12].offset().local_minus_utc(), -5 * 3600);
        assert_eq!(resolved[0].with_timezone(&Utc).hour(), 5);
    }

    #[test]
    fn irregular_samples_resolve_the_repeat_by_going_backwards() {
        // Fuel-mix samples have irregular seconds, so there are no exact duplicates;
        // the repeat shows up as the clock stepping back.
        let mut clock = EasternClock::default();
        let d = day(2026, 11, 1);
        let samples = [(1, 58, 59), (1, 3, 4), (1, 58, 40)];
        let got: Vec<i32> = samples
            .iter()
            .map(|&(h, m, s)| {
                clock
                    .resolve("Nuclear", d.and_hms_opt(h, m, s).expect("time"))
                    .expect("resolves")
                    .offset()
                    .local_minus_utc()
                    / 3600
            })
            .collect();
        assert_eq!(got, [-4, -5, -5]);
    }

    #[test]
    fn interleaved_series_do_not_share_state() {
        let mut clock = EasternClock::default();
        let t = at(2026, 11, 1, 1, 0, 0);
        let got: Vec<i32> = [321_u32, 4000, 321, 4000]
            .into_iter()
            .map(|id| {
                clock
                    .resolve(id, t)
                    .expect("resolves")
                    .offset()
                    .local_minus_utc()
                    / 3600
            })
            .collect();
        assert_eq!(got, [-4, -4, -5, -5]);
    }

    #[test]
    fn spring_forward_gap_is_an_error() {
        let mut clock: EasternClock<u32> = EasternClock::default();
        let err = clock
            .resolve(1, at(2026, 3, 8, 2, 30, 0))
            .expect_err("02:30 never happened");
        assert!(matches!(err, Error::NonexistentLocalTime(_)));
    }
}

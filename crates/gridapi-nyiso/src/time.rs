//! NYISO publishes wall-clock Eastern Prevailing Time. Some files carry a
//! `Time Zone` column (`EST`/`EDT`); most do not. Either way every timestamp is
//! resolved to an unambiguous instant with its UTC offset.

use std::{collections::HashMap, hash::Hash};

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::America::New_York;

use crate::error::Error;

const EST_SECONDS_WEST: i32 = 5 * 3600;
const EDT_SECONDS_WEST: i32 = 4 * 3600;

/// Formats NYISO uses for a timestamp cell, most specific first.
const FORMATS: [&str; 2] = ["%m/%d/%Y %H:%M:%S", "%m/%d/%Y %H:%M"];

/// Parse a published `MM/DD/YYYY HH:MM[:SS]` cell into a naive wall-clock time.
pub(crate) fn parse_local(dataset: &str, cell: &str) -> Result<NaiveDateTime, Error> {
    let cell = cell.trim();
    FORMATS
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(cell, fmt).ok())
        .ok_or_else(|| Error::Timestamp {
            dataset: dataset.to_owned(),
            value: cell.to_owned(),
        })
}

/// Resolve a wall-clock time using the file's own `Time Zone` cell.
pub(crate) fn with_zone_cell(
    local: NaiveDateTime,
    zone: &str,
) -> Result<DateTime<FixedOffset>, Error> {
    let seconds_west = match zone.trim().to_ascii_uppercase().as_str() {
        "EST" => EST_SECONDS_WEST,
        "EDT" => EDT_SECONDS_WEST,
        other => return Err(Error::UnknownZone(other.to_owned())),
    };
    FixedOffset::west_opt(seconds_west)
        .and_then(|offset| offset.from_local_datetime(&local).single())
        .ok_or(Error::NonexistentLocalTime(local))
}

/// Resolves wall-clock times from files that have no `Time Zone` column.
///
/// On the fall-back day the repeated hour appears twice, first as EDT and then
/// as EST. Rows of one series are in time order, so a repeated local time that
/// is not later than the series' previous instant must be the second occurrence.
/// The series key (zone name, fuel category, ...) keeps interleaved series apart.
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
    use chrono::{NaiveDate, Timelike, Utc};

    use super::*;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .and_then(|date| date.and_hms_opt(h, min, 0))
            .expect("valid test date")
    }

    #[test]
    fn parses_both_published_formats() {
        let a = parse_local("t", "11/17/2026 00:05:00").expect("with seconds");
        let b = parse_local("t", "11/17/2026 00:05").expect("without seconds");
        assert_eq!(a, b);
        assert_eq!(a, at(2026, 11, 17, 0, 5));
    }

    #[test]
    fn rejects_garbage() {
        let err = parse_local("pal", "2026-11-17T00:05").expect_err("iso format is not NYISO's");
        assert!(matches!(err, Error::Timestamp { .. }), "got {err:?}");
    }

    #[test]
    fn zone_cell_sets_the_offset() {
        let edt = with_zone_cell(at(2026, 11, 1, 1, 30), "EDT").expect("edt");
        let est = with_zone_cell(at(2026, 11, 1, 1, 30), "est").expect("est, any case");
        assert_eq!(edt.offset().local_minus_utc(), -4 * 3600);
        assert_eq!(est.offset().local_minus_utc(), -5 * 3600);
        assert_eq!(
            est.with_timezone(&Utc) - edt.with_timezone(&Utc),
            chrono::Duration::hours(1)
        );
    }

    #[test]
    fn zone_cell_rejects_unknown() {
        let err = with_zone_cell(at(2026, 11, 1, 1, 0), "CST").expect_err("not eastern");
        assert!(matches!(err, Error::UnknownZone(z) if z == "CST"));
    }

    #[test]
    fn ordinary_day_is_edt_or_est_by_calendar() {
        let mut clock = EasternClock::default();
        let summer = clock.resolve("a", at(2026, 7, 1, 12, 0)).expect("summer");
        let winter = clock.resolve("a", at(2026, 12, 1, 12, 0)).expect("winter");
        assert_eq!(summer.offset().local_minus_utc(), -4 * 3600);
        assert_eq!(winter.offset().local_minus_utc(), -5 * 3600);
    }

    #[test]
    fn fall_back_hour_resolves_first_edt_then_est() {
        let mut clock = EasternClock::default();
        let day = |h, m| at(2026, 11, 1, h, m);
        let seq: Vec<_> = [day(0, 0), day(1, 0), day(1, 0), day(2, 0)]
            .into_iter()
            .map(|t| clock.resolve("capitl", t).expect("resolves"))
            .collect();
        let offsets: Vec<i32> = seq
            .iter()
            .map(|t| t.offset().local_minus_utc() / 3600)
            .collect();
        assert_eq!(offsets, [-4, -4, -5, -5]);
        let utc_hours: Vec<u32> = seq.iter().map(|t| t.with_timezone(&Utc).hour()).collect();
        assert_eq!(
            utc_hours,
            [4, 5, 6, 7],
            "four consecutive hours, none skipped"
        );
    }

    #[test]
    fn fall_back_five_minute_series_stay_monotonic() {
        let mut clock = EasternClock::default();
        // 01:00..01:55 as EDT, the same wall clock again as EST, then 02:00.
        let hour = (0..12).map(|i| at(2026, 11, 1, 1, i * 5));
        let wall_clock: Vec<_> = hour
            .clone()
            .chain(hour)
            .chain(std::iter::once(at(2026, 11, 1, 2, 0)))
            .collect();
        let times: Vec<_> = wall_clock
            .into_iter()
            .map(|t| clock.resolve("z", t).expect("resolves"))
            .collect();
        assert_eq!(times.len(), 25);
        assert!(
            times.windows(2).all(|w| w[0] < w[1]),
            "instants must be strictly increasing: {times:?}"
        );
    }

    #[test]
    fn interleaved_series_do_not_share_state() {
        // Rows arrive A@01:00, B@01:00 (both EDT), then A@01:00, B@01:00 again (both EST).
        let mut clock = EasternClock::default();
        let t = at(2026, 11, 1, 1, 0);
        let got: Vec<i32> = ["a", "b", "a", "b"]
            .into_iter()
            .map(|s| {
                clock
                    .resolve(s, t)
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
        let mut clock: EasternClock<&str> = EasternClock::default();
        let err = clock
            .resolve("a", at(2026, 3, 8, 2, 30))
            .expect_err("02:30 never happened on 2026-03-08");
        assert!(matches!(err, Error::NonexistentLocalTime(_)));
    }
}

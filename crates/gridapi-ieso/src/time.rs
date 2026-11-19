//! IESO time conventions.
//!
//! Every IESO report is stamped in Eastern *Standard* Time all year round
//! (UTC-5, no daylight saving). Hours are "hour ending": hour 1 covers
//! 00:00-01:00 EST of the delivery date, hour 24 covers 23:00-24:00. Real-time
//! reports further split an hour into twelve five-minute intervals.

use chrono::{DateTime, Duration, NaiveDate, Utc};

/// Hours IESO local time sits behind UTC, all year (no DST).
const EST_BEHIND_UTC_HOURS: i64 = 5;

/// Start (UTC) of delivery hour `hour` (1..=24, hour ending) on `date`.
/// Returns `None` for an hour outside `1..=24`.
pub fn hour_start_utc(date: NaiveDate, hour: u8) -> Option<DateTime<Utc>> {
    if !(1..=24).contains(&hour) {
        return None;
    }
    let midnight_est_as_utc = date.and_hms_opt(0, 0, 0)?.and_utc();
    Some(midnight_est_as_utc + Duration::hours(EST_BEHIND_UTC_HOURS + i64::from(hour) - 1))
}

/// Start (UTC) of five-minute `interval` (1..=12) within delivery hour `hour`.
/// Returns `None` when either index is out of range.
pub fn interval_start_utc(date: NaiveDate, hour: u8, interval: u8) -> Option<DateTime<Utc>> {
    if !(1..=12).contains(&interval) {
        return None;
    }
    hour_start_utc(date, hour).map(|h| h + Duration::minutes(5 * (i64::from(interval) - 1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[test]
    fn hour_one_starts_at_midnight_est_which_is_five_utc() {
        let t = hour_start_utc(d(2026, 11, 17), 1).expect("hour 1");
        assert_eq!(t.to_rfc3339(), "2026-11-17T05:00:00+00:00");
    }

    #[test]
    fn hour_twenty_four_rolls_into_the_next_utc_day() {
        let t = hour_start_utc(d(2026, 11, 17), 24).expect("hour 24");
        assert_eq!(t.to_rfc3339(), "2026-11-18T04:00:00+00:00");
    }

    #[test]
    fn est_is_used_even_in_summer() {
        let t = hour_start_utc(d(2026, 7, 1), 1).expect("hour 1");
        assert_eq!(t.to_rfc3339(), "2026-07-01T05:00:00+00:00");
    }

    #[test]
    fn five_minute_intervals_step_within_the_hour() {
        let t = interval_start_utc(d(2026, 11, 18), 2, 3).expect("interval");
        assert_eq!(t.to_rfc3339(), "2026-11-18T06:10:00+00:00");
    }

    #[test]
    fn out_of_range_indices_are_rejected() {
        assert!(hour_start_utc(d(2026, 11, 17), 0).is_none());
        assert!(hour_start_utc(d(2026, 11, 17), 25).is_none());
        assert!(interval_start_utc(d(2026, 11, 17), 1, 0).is_none());
        assert!(interval_start_utc(d(2026, 11, 17), 1, 13).is_none());
    }
}

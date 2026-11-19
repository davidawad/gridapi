//! OASIS timestamp formatting and range chunking.

use chrono::{DateTime, TimeDelta, Utc};

/// The OASIS request timestamp layout, always UTC: `20261110T08:00-0000`.
pub const OASIS_TIME_FORMAT: &str = "%Y%m%dT%H:%M-0000";

/// The longest window OASIS accepts in one request (it rejects more than 31 days).
pub const OASIS_MAX_SPAN_DAYS: i64 = 31;

/// Formats an instant the way `startdatetime` / `enddatetime` expect.
///
/// # Examples
///
/// ```
/// use chrono::{TimeZone, Utc};
/// let t = Utc.with_ymd_and_hms(2026, 11, 10, 8, 0, 0).single();
/// assert_eq!(gridapi_caiso::format_oasis_time(t.unwrap_or_default()), "20261110T08:00-0000");
/// ```
#[must_use]
pub fn format_oasis_time(instant: DateTime<Utc>) -> String {
    instant.format(OASIS_TIME_FORMAT).to_string()
}

/// Splits `[start, end)` into contiguous half-open windows no longer than `max_span`.
///
/// OASIS treats `enddatetime` as exclusive, so adjacent windows never overlap
/// and never leave a gap. Returns an empty list when `end <= start` or
/// `max_span` is not positive.
///
/// # Examples
///
/// ```
/// use chrono::{TimeDelta, TimeZone, Utc};
/// let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap_or_default();
/// let end = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).single().unwrap_or_default();
/// let windows = gridapi_caiso::chunk_range(start, end, TimeDelta::days(30));
/// assert_eq!(windows.len(), 2);
/// assert_eq!(windows[0].1, windows[1].0);
/// assert_eq!(windows[1].1, end);
/// ```
#[must_use]
pub fn chunk_range(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    max_span: TimeDelta,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    if end <= start || max_span <= TimeDelta::zero() {
        return Vec::new();
    }
    std::iter::successors(Some(start), |&s| s.checked_add_signed(max_span))
        .take_while(|&s| s < end)
        .map(|s| {
            (
                s,
                s.checked_add_signed(max_span).map_or(end, |e| e.min(end)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .unwrap_or_default()
    }

    #[test]
    fn formats_in_utc_layout() {
        assert_eq!(
            format_oasis_time(at(2026, 11, 10, 8)),
            "20261110T08:00-0000"
        );
    }

    #[test]
    fn windows_are_contiguous_and_bounded() {
        let (start, end) = (at(2026, 1, 1, 0), at(2026, 6, 15, 7));
        let span = TimeDelta::days(30);
        let windows = chunk_range(start, end, span);
        assert_eq!(windows.first().map(|w| w.0), Some(start));
        assert_eq!(windows.last().map(|w| w.1), Some(end));
        assert!(windows.iter().all(|(s, e)| e > s && *e - *s <= span));
        assert!(windows.windows(2).all(|pair| pair[0].1 == pair[1].0));
    }

    #[test]
    fn short_range_is_one_window() {
        let (s, e) = (at(2026, 1, 1, 0), at(2026, 1, 2, 0));
        assert_eq!(chunk_range(s, e, TimeDelta::days(31)), vec![(s, e)]);
    }

    #[test]
    fn empty_or_inverted_range_yields_nothing() {
        let t = at(2026, 1, 1, 0);
        assert!(chunk_range(t, t, TimeDelta::days(1)).is_empty());
        assert!(chunk_range(at(2026, 1, 2, 0), t, TimeDelta::days(1)).is_empty());
        assert!(chunk_range(t, at(2026, 1, 3, 0), TimeDelta::zero()).is_empty());
    }
}

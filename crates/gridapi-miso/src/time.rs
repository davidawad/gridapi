//! MISO timestamps. MISO publishes market data in Eastern Standard Time (UTC-5) all year, with no DST.

use chrono::{DateTime, FixedOffset, NaiveDateTime, Offset, TimeZone, Utc};

/// Seconds east of UTC for MISO's market clock (EST, UTC-5, never DST).
const EST_OFFSET_SECS: i32 = -5 * 3600;

/// MISO's market-clock offset.
pub fn est() -> FixedOffset {
    // The constant is well inside chrono's range; the fallback (UTC) is unreachable.
    FixedOffset::east_opt(EST_OFFSET_SECS).unwrap_or_else(|| Utc.fix())
}

/// Convert a naive MISO market-clock timestamp (EST) to UTC.
pub fn est_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    est()
        .from_local_datetime(&naive)
        .single()
        .map_or_else(|| naive.and_utc(), |t| t.with_timezone(&Utc))
}

/// Formats tried, in order, for the timestamps the public API emits.
const FORMATS: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %I:%M:%S %p",
    "%m/%d/%Y %I:%M:%S %p",
    "%m/%d/%Y %H:%M:%S",
];

/// Parse a timestamp in any format the MISO public API uses.
pub fn parse_timestamp(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    FORMATS
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(text, fmt).ok())
}

/// Parse the `RefId` MISO stamps on every response, e.g. `18-Nov-2026 - Interval 01:10 EST`.
///
/// Returns the interval's EST market-clock timestamp.
pub fn parse_ref_id(ref_id: &str) -> Option<NaiveDateTime> {
    let cleaned = ref_id.trim().trim_end_matches("EST").trim();
    NaiveDateTime::parse_from_str(cleaned, "%d-%b-%Y - Interval %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(cleaned, "%d-%b-%Y - Interval %H:%M"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn parses_every_public_api_format() {
        for text in [
            "2026-11-18T01:05:00",
            "2026-11-18 01:05:00",
            "2026-11-18 1:05:00 AM",
            "11/18/2026 01:05:00 AM",
        ] {
            let t = parse_timestamp(text).unwrap_or_else(|| panic!("no parse for {text}"));
            assert_eq!((t.hour(), t.minute()), (1, 5), "{text}");
        }
        assert_eq!(parse_timestamp("not a time"), None);
    }

    #[test]
    fn ref_id_round_trips() {
        let t = parse_ref_id("18-Nov-2026 - Interval 01:10 EST").expect("ref id");
        assert_eq!(t.to_string(), "2026-11-18 01:10:00");
        let t2 = parse_ref_id("18-Nov-2026 - Interval 01:10:00 EST").expect("ref id with seconds");
        assert_eq!(t, t2);
        assert_eq!(parse_ref_id("garbage"), None);
    }

    #[test]
    fn est_is_five_hours_behind_utc() {
        let naive = parse_timestamp("2026-11-18T01:05:00").expect("ts");
        assert_eq!(est_to_utc(naive).to_rfc3339(), "2026-11-18T06:05:00+00:00");
    }
}

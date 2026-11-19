//! serde helpers for AESO's JSON quirks: numbers arrive as strings in some
//! APIs and numbers in others, "not yet known" is the empty string, and
//! timestamps are `yyyy-MM-dd HH:mm[:ss]` without a zone designator.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum NumOrStr {
    Num(f64),
    Str(String),
}

/// A number given as a JSON number, a numeric string, `""` or `null`.
pub(crate) fn opt_f64<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    match Option::<NumOrStr>::deserialize(d)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => Ok(Some(n)),
        Some(NumOrStr::Str(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<f64>()
                    .map(Some)
                    .map_err(|e| serde::de::Error::custom(format!("bad number {s:?}: {e}")))
            }
        }
    }
}

/// A small non-negative integer given as a number or numeric string.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is checked to be an integer in 0..=255 before the cast"
)]
pub(crate) fn u8_lenient<'de, D>(d: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let n = opt_f64(d)?.ok_or_else(|| serde::de::Error::custom("missing integer"))?;
    if n.fract() == 0.0 && (0.0..=255.0).contains(&n) {
        Ok(n as u8)
    } else {
        Err(serde::de::Error::custom(format!(
            "integer out of range: {n}"
        )))
    }
}

/// `"Y"`/`"N"` flags.
pub(crate) fn yes_no<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    match s.trim() {
        "Y" | "y" => Ok(true),
        "N" | "n" | "" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "expected Y or N, got {other:?}"
        ))),
    }
}

/// Parse `yyyy-MM-dd HH:mm` or `yyyy-MM-dd HH:mm:ss`.
pub(crate) fn parse_naive(s: &str) -> Result<NaiveDateTime, chrono::ParseError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
}

/// A UTC timestamp string.
pub(crate) fn utc<'de, D>(d: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    parse_naive(s.trim())
        .map(|n| n.and_utc())
        .map_err(|e| serde::de::Error::custom(format!("bad UTC timestamp {s:?}: {e}")))
}

/// A local (Mountain prevailing time) timestamp string, kept zone-less.
pub(crate) fn local<'de, D>(d: D) -> Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    parse_naive(s.trim())
        .map_err(|e| serde::de::Error::custom(format!("bad local timestamp {s:?}: {e}")))
}

/// An optional text field where `""` means absent.
pub(crate) fn opt_text<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty()))
}

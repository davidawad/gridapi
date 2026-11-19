//! Serde helpers for MISO's habit of sending every number as a JSON string.

use chrono::NaiveDateTime;
use serde::de::{self, Deserializer};
use serde::Deserialize;

use crate::time::parse_timestamp;

/// Either a JSON string or a JSON number.
#[derive(Deserialize)]
#[serde(untagged)]
enum Loose {
    Num(f64),
    Text(String),
}

/// `"13.9"` or `13.9` becomes `13.9`; anything else is a decode error.
pub(crate) fn f64_loose<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    match Loose::deserialize(d)? {
        Loose::Num(n) => Ok(n),
        Loose::Text(s) => s
            .trim()
            .replace(',', "")
            .parse::<f64>()
            .map_err(|e| de::Error::custom(format!("bad number {s:?}: {e}"))),
    }
}

/// Like [`f64_loose`], but null, empty and `"None"` become `None`.
pub(crate) fn opt_f64_loose<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(match Option::<Loose>::deserialize(d)? {
        None => None,
        Some(Loose::Num(n)) => Some(n),
        Some(Loose::Text(s)) => s.trim().replace(',', "").parse::<f64>().ok(),
    })
}

/// Integer-valued strings (`"1"`, `"24"`) as `u32`.
pub(crate) fn u32_loose<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let n = f64_loose(d)?;
    if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "range and integrality checked above"
        )]
        Ok(n as u32)
    } else {
        Err(de::Error::custom(format!(
            "not a small unsigned integer: {n}"
        )))
    }
}

/// A timestamp in any format the public API emits.
pub(crate) fn timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDateTime, D::Error> {
    let s = String::deserialize(d)?;
    parse_timestamp(&s).ok_or_else(|| de::Error::custom(format!("bad timestamp {s:?}")))
}

/// Like [`timestamp`], but null and empty become `None`.
pub(crate) fn opt_timestamp<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<NaiveDateTime>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.and_then(|s| parse_timestamp(&s)))
}

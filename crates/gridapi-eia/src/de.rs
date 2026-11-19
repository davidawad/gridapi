//! Lenient serde helpers. EIA returns almost every number as a JSON string
//! (`"value":"49812"`) and uses `null` or `""` for missing values, so typed
//! rows decode through these instead of plain `f64`.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Number-ish JSON scalar: string, number or null -> `Option<f64>`.
pub(crate) fn opt_f64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

/// Unsigned count that may arrive as a string or a number.
pub(crate) fn u64_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("count is not an unsigned integer")),
        Value::String(s) => s
            .trim()
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("count {s:?} is not an integer"))),
        other => Err(serde::de::Error::custom(format!(
            "expected a count, got {other}"
        ))),
    }
}

/// Like [`opt_f64`] but for integers (`totalFacets`, years).
pub(crate) fn opt_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Ok(match Option::<Value>::deserialize(d)? {
        Some(Value::Number(n)) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct T {
        #[serde(default, deserialize_with = "opt_f64")]
        v: Option<f64>,
    }

    fn v(json: &str) -> Option<f64> {
        serde_json::from_str::<T>(json).ok().and_then(|t| t.v)
    }

    #[test]
    fn f64_from_string_number_null_empty_and_absent() {
        assert_eq!(v(r#"{"v":"49812"}"#), Some(49812.0));
        assert_eq!(v(r#"{"v":-3.5}"#), Some(-3.5));
        assert_eq!(v(r#"{"v":null}"#), None);
        assert_eq!(v(r#"{"v":""}"#), None);
        assert_eq!(v(r#"{"v":"n/a"}"#), None);
        assert_eq!(v("{}"), None);
    }

    #[derive(Deserialize)]
    struct C {
        #[serde(deserialize_with = "u64_lenient")]
        total: u64,
    }

    #[test]
    fn count_from_string_or_number() {
        let a: C = serde_json::from_str(r#"{"total":"69077"}"#).unwrap_or(C { total: 0 });
        let b: C = serde_json::from_str(r#"{"total":12}"#).unwrap_or(C { total: 0 });
        assert_eq!((a.total, b.total), (69077, 12));
        assert!(serde_json::from_str::<C>(r#"{"total":"x"}"#).is_err());
        assert!(serde_json::from_str::<C>(r#"{"total":-1}"#).is_err());
    }
}

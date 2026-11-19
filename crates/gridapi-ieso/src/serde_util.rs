//! Small serde helpers for the quirks of IESO XML: empty elements stand for
//! "not published yet", and list wrappers use a different child tag per report.

use serde::{Deserialize, Deserializer};

/// An element whose text may be absent or empty. `<X/>`, `<X></X>` and a
/// missing `X` all become `None`; anything else must parse as `f64`.
pub(crate) fn opt_f64<'de, D>(d: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    match raw.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => s
            .parse::<f64>()
            .map(Some)
            .map_err(|e| serde::de::Error::custom(format!("bad number {s:?}: {e}"))),
    }
}

/// Like [`opt_f64`] for small signed integers (quality flags).
pub(crate) fn opt_i32<'de, D>(d: D) -> Result<Option<i32>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    match raw.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) => s
            .parse::<i32>()
            .map(Some)
            .map_err(|e| serde::de::Error::custom(format!("bad integer {s:?}: {e}"))),
    }
}

/// An optional text element: empty becomes `None`.
pub(crate) fn opt_text<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()))
}

/// Collect every child element of a wrapper (`<Outputs><Output>..</Output>..</Outputs>`)
/// regardless of the child tag name.
pub(crate) fn children<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    struct Children<T> {
        #[serde(rename = "$value", default = "Vec::new")]
        items: Vec<T>,
    }
    Children::<T>::deserialize(d).map(|c| c.items)
}

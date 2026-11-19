//! Typed response models, one module per API family.
//!
//! Timestamps come in pairs: `*_utc` is a UTC instant and `*_mpt` is the same
//! instant in Mountain prevailing time as AESO wrote it (no zone attached).
//! Numbers that AESO has not published yet (a forecast hour, a price that has
//! not settled) are `None`.

pub mod assets;
pub mod csd;
pub mod interchange;
pub mod market;
pub mod price;
pub mod supply;

use serde::Deserialize;

/// The envelope most AESO endpoints wrap their payload in.
#[derive(Debug, Deserialize)]
pub(crate) struct Reply<T> {
    #[serde(rename = "return")]
    pub(crate) payload: T,
}

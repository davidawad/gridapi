//! Typed helpers for the datasets most used for grid and power-market work.
//! Each module offers query constructors (route, frequency and data columns
//! preset; add facets, period bounds and sort on the returned
//! [`crate::DataQuery`]) and a row struct to decode into.
//!
//! Any other route works through [`crate::DataQuery`] with [`crate::Row`].

pub mod generators;
pub mod natural_gas;
pub mod retail;
pub mod rto;

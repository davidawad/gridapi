//! Nodal LMP reports (CSV).
//!
//! The first line is `CREATED AT 2026/11/16 13:35:36 FOR 2026/11/17`, followed
//! by a header row and one row per node and hour (or hour and interval).
//! The Ontario zonal price appears as the pricing location `ONTARIO_ZONAL_PRICE`.

use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;

use crate::error::{Error, Result};

/// A decoded LMP file.
#[derive(Debug, Clone, PartialEq)]
pub struct LmpFile<R> {
    /// When IESO produced the file (Eastern Standard Time).
    pub created_at: NaiveDateTime,
    /// The delivery date the file covers.
    pub delivery_date: NaiveDate,
    /// One row per node and period.
    pub rows: Vec<R>,
}

/// A day-ahead or pre-dispatch row: one node, one delivery hour.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HourlyLmp {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Delivery Hour")]
    pub hour: u8,
    /// Pricing node, for example `ABITIBISF-LT.AG_T1`.
    #[serde(rename = "Pricing Location")]
    pub location: String,
    /// Locational marginal price in CAD/MWh.
    #[serde(rename = "LMP")]
    pub lmp: Option<f64>,
    /// Loss component in CAD/MWh.
    #[serde(rename = "Energy Loss Price")]
    pub loss_price: Option<f64>,
    /// Congestion component in CAD/MWh.
    #[serde(rename = "Energy Congestion Price")]
    pub congestion_price: Option<f64>,
}

/// A real-time row: one node, one five-minute interval.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IntervalLmp {
    /// Delivery hour, hour-ending `1..=24`.
    #[serde(rename = "Delivery Hour")]
    pub hour: u8,
    /// Interval within the hour, `1..=12`.
    #[serde(rename = "Interval")]
    pub interval: u8,
    /// Pricing node.
    #[serde(rename = "Pricing Location")]
    pub location: String,
    /// Locational marginal price in CAD/MWh.
    #[serde(rename = "LMP")]
    pub lmp: Option<f64>,
    /// Loss component in CAD/MWh.
    #[serde(rename = "Energy Loss Price")]
    pub loss_price: Option<f64>,
    /// Congestion component in CAD/MWh.
    #[serde(rename = "Energy Congestion Price")]
    pub congestion_price: Option<f64>,
}

/// Decode a day-ahead or pre-dispatch hourly LMP file.
pub fn parse_hourly_lmp(csv_text: &str) -> Result<LmpFile<HourlyLmp>> {
    parse(csv_text)
}

/// Decode a real-time five-minute LMP file.
pub fn parse_interval_lmp(csv_text: &str) -> Result<LmpFile<IntervalLmp>> {
    parse(csv_text)
}

fn parse<R: serde::de::DeserializeOwned>(text: &str) -> Result<LmpFile<R>> {
    let text = text.trim_start_matches('\u{feff}');
    let (first, rest) = text
        .split_once('\n')
        .ok_or_else(|| Error::Malformed("LMP file has no header line".into()))?;
    let (created_at, delivery_date) = parse_created_line(first)?;
    let rows = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(rest.as_bytes())
        .deserialize::<R>()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(LmpFile {
        created_at,
        delivery_date,
        rows,
    })
}

/// `CREATED AT 2026/11/16 13:35:36 FOR 2026/11/17`
fn parse_created_line(line: &str) -> Result<(NaiveDateTime, NaiveDate)> {
    let bad = || Error::Malformed(format!("unrecognised LMP header line: {line:?}"));
    let body = line.trim().strip_prefix("CREATED AT ").ok_or_else(bad)?;
    let (created, delivery) = body.split_once(" FOR ").ok_or_else(bad)?;
    let created_at =
        NaiveDateTime::parse_from_str(created.trim(), "%Y/%m/%d %H:%M:%S").map_err(|_| bad())?;
    let delivery_date =
        NaiveDate::parse_from_str(delivery.trim(), "%Y/%m/%d").map_err(|_| bad())?;
    Ok((created_at, delivery_date))
}

/// The pricing location that carries the Ontario zonal price in LMP files.
pub const ONTARIO_ZONAL_PRICE: &str = "ONTARIO_ZONAL_PRICE";

impl LmpFile<HourlyLmp> {
    /// Rows for one pricing location, in hour order as published.
    pub fn location<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a HourlyLmp> + 'a {
        self.rows.iter().filter(move |r| r.location == name)
    }
}

impl LmpFile<IntervalLmp> {
    /// Rows for one pricing location, in interval order as published.
    pub fn location<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a IntervalLmp> + 'a {
        self.rows.iter().filter(move |r| r.location == name)
    }
}

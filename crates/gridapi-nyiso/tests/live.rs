//! Live tests against mis.nyiso.com. They need network access and are skipped by
//! default: `cargo test -p gridapi-nyiso --test live -- --ignored --nocapture`.

// Live tests fail loudly with `expect`, and `assert!(x.is_empty())` reads best.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::collections::BTreeSet;

use gridapi_nyiso::{
    chrono::{Duration, NaiveDate, Utc},
    Client, Level,
};

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

fn client() -> Client {
    Client::builder()
        .user_agent("gridapi-nyiso-live-tests/0.1")
        .build()
        .expect("client builds")
}

fn yesterday() -> NaiveDate {
    Utc::now().date_naive() - Duration::days(1)
}

/// The value the task brief verified by hand: CAPITL day-ahead, 2026-11-17 00:00.
#[tokio::test]
#[ignore = "live network"]
async fn day_ahead_capitl_2026_11_17() {
    let rows = client()
        .day_ahead_lbmp(Level::Zone, day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("fetches");
    let capitl: Vec<_> = rows.iter().filter(|r| r.name == "CAPITL").collect();
    assert_eq!(capitl.len(), 24, "24 hourly rows for CAPITL");
    let midnight = capitl[0];
    println!(
        "damlbmp zone 2026-11-17 CAPITL 00:00: lbmp={} losses={} congestion={} ts={}",
        midnight.lbmp,
        midnight.marginal_cost_losses,
        midnight.marginal_cost_congestion,
        midnight.timestamp
    );
    assert!(midnight.lbmp.is_finite());
}

/// An old day that only exists in the monthly archive.
#[tokio::test]
#[ignore = "live network"]
async fn archive_only_day_2025_01_15() {
    let rows = client()
        .day_ahead_lbmp(Level::Zone, day(2025, 1, 15))
        .fetch_strict()
        .await
        .expect("served from the monthly archive");
    let capitl = rows
        .iter()
        .find(|r| r.name == "CAPITL")
        .expect("CAPITL row");
    println!("damlbmp zone 2025-01-15 (archive) first CAPITL: {capitl:?}");
    assert!(
        rows.len() >= 15 * 24,
        "15 zones x 24 hours, got {}",
        rows.len()
    );
}

/// A day three years back through the same entry point, plus one from 2001.
#[tokio::test]
#[ignore = "live network"]
async fn archive_reaches_back_to_2001() {
    let rows = client()
        .day_ahead_lbmp(Level::Zone, day(2001, 1, 1))
        .fetch_strict()
        .await
        .expect("2001 archive");
    let first = &rows[0];
    println!("damlbmp zone 2001-01-01 first row: {first:?}");
    assert_eq!(first.name, "CAPITL");
    assert!(first.lbmp.is_finite());
}

/// The fall-back day has 25 distinct hourly instants even though the file has no
/// time zone column.
#[tokio::test]
#[ignore = "live network"]
async fn fall_back_day_has_25_distinct_hours() {
    let rows = client()
        .day_ahead_lbmp(Level::Zone, day(2025, 11, 2))
        .fetch_strict()
        .await
        .expect("fetches");
    let capitl: BTreeSet<_> = rows
        .iter()
        .filter(|r| r.name == "CAPITL")
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    println!("2025-11-02 CAPITL distinct instants: {}", capitl.len());
    assert_eq!(capitl.len(), 25);
}

#[tokio::test]
#[ignore = "live network"]
async fn spring_forward_day_has_23_distinct_hours() {
    let rows = client()
        .day_ahead_lbmp(Level::Zone, day(2026, 3, 8))
        .fetch_strict()
        .await
        .expect("fetches");
    let capitl: BTreeSet<_> = rows
        .iter()
        .filter(|r| r.name == "CAPITL")
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    println!("2026-03-08 CAPITL distinct instants: {}", capitl.len());
    assert_eq!(capitl.len(), 23);
}

#[tokio::test]
#[ignore = "live network"]
async fn every_dataset_decodes_for_yesterday() {
    let c = client();
    let d = yesterday();

    let rt = c
        .real_time_lbmp(Level::Zone, d)
        .fetch_strict()
        .await
        .expect("realtime zone");
    let rt_hourly = c
        .integrated_real_time_lbmp(Level::Zone, d)
        .fetch_strict()
        .await
        .expect("rtlbmp zone");
    let pal = c.actual_load(d).fetch_strict().await.expect("pal");
    let integrated = c
        .integrated_load(d)
        .fetch_strict()
        .await
        .expect("palIntegrated");
    let isolf = c.load_forecast(d).fetch_strict().await.expect("isolf");
    let mix = c.fuel_mix(d).fetch_strict().await.expect("rtfuelmix");
    let da_asp = c
        .day_ahead_ancillary(d)
        .fetch_strict()
        .await
        .expect("damasp");
    let rt_asp = c
        .real_time_ancillary(d)
        .fetch_strict()
        .await
        .expect("rtasp");
    let flows = c
        .interface_flows(d)
        .fetch_strict()
        .await
        .expect("ExternalLimitsFlows");

    println!("day {d}");
    println!("realtime zone rows {} first {:?}", rt.len(), rt.first());
    println!(
        "rtlbmp zone rows {} first {:?}",
        rt_hourly.len(),
        rt_hourly.first()
    );
    println!("pal rows {} first {:?}", pal.len(), pal.first());
    println!(
        "palIntegrated rows {} first {:?}",
        integrated.len(),
        integrated.first()
    );
    println!("isolf rows {} first {:?}", isolf.len(), isolf.first());
    println!("rtfuelmix rows {} first {:?}", mix.len(), mix.first());
    println!("damasp rows {} first {:?}", da_asp.len(), da_asp.first());
    println!("rtasp rows {} first {:?}", rt_asp.len(), rt_asp.first());
    println!("flows rows {} first {:?}", flows.len(), flows.first());

    assert!(rt.len() >= 280 * 15, "288 five-minute intervals x 15 zones");
    assert!(rt_hourly.len() >= 23 * 15);
    assert!(
        pal.len() >= 280 * 11,
        "288 five-minute intervals x 11 load zones"
    );
    assert!(integrated.len() >= 23 * 11);
    assert!(isolf.len() >= 23);
    assert!(mix.len() >= 280 * 5);
    assert!(da_asp.len() >= 23 * 11);
    assert!(rt_asp.len() >= 280 * 11);
    assert!(flows.len() >= 280);
}

#[tokio::test]
#[ignore = "live network"]
async fn generator_level_decodes_for_yesterday() {
    let rows = client()
        .day_ahead_lbmp(Level::Generator, yesterday())
        .fetch_strict()
        .await
        .expect("damlbmp gen");
    println!("damlbmp gen rows {} first {:?}", rows.len(), rows.first());
    assert!(rows.len() > 24 * 100, "hundreds of generators x 24 hours");
}

/// Range fetch across a month boundary uses one archive request per month.
#[tokio::test]
#[ignore = "live network"]
async fn range_across_month_boundary() {
    let series = client()
        .fuel_mix(day(2025, 6, 29))
        .through(day(2025, 7, 2))
        .fetch()
        .await
        .expect("fetches");
    println!(
        "fuel mix 2025-06-29..2025-07-02: {} rows, missing {:?}",
        series.rows.len(),
        series.missing
    );
    assert!(series.missing.is_empty());
    assert!(series.rows.len() >= 4 * 280 * 5);
}

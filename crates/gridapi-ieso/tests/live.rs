//! Live checks against <https://reports-public.ieso.ca> (no key needed).
//!
//! Run with: `cargo test -p gridapi-ieso --test live -- --ignored --nocapture`
//! The assertions use dates that IESO has already settled, so they stay true.
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

use chrono::NaiveDate;
use gridapi_ieso::data::lmp::ONTARIO_ZONAL_PRICE;
use gridapi_ieso::{Client, FileSpec, Report, Stamp, Version};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
}

fn client() -> Client {
    Client::builder()
        .user_agent("gridapi-ieso-live-tests/0.1 (https://example.invalid)")
        .build()
        .expect("client")
}

#[tokio::test]
#[ignore = "network"]
async fn live_day_ahead_zonal_price_2026_11_17_hour_1() {
    let doc = client()
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect("fetch");
    let h1 = doc.body.hour(1).expect("hour 1");
    println!(
        "DA Ontario zonal 2026-11-17 HE1 = {:?} CAD/MWh (flag {:?})",
        h1.zonal_price, h1.flag
    );
    assert!(h1.zonal_price.is_some());
    assert_eq!(doc.body.hours.len(), 24);
}

#[tokio::test]
#[ignore = "network"]
async fn live_day_ahead_lmp_has_the_ontario_zonal_row_and_many_nodes() {
    let file = client().da_lmp(d(2026, 11, 17)).await.expect("fetch");
    let zonal = file
        .location(ONTARIO_ZONAL_PRICE)
        .next()
        .expect("zonal row");
    let nodes: std::collections::BTreeSet<_> =
        file.rows.iter().map(|r| r.location.as_str()).collect();
    println!(
        "DA LMP 2026-11-17: {} rows, {} locations, ONTARIO_ZONAL_PRICE HE1 = {:?}",
        file.rows.len(),
        nodes.len(),
        zonal.lmp
    );
    assert!(zonal.lmp.is_some());
    assert!(nodes.len() > 500);
}

#[tokio::test]
#[ignore = "network"]
async fn live_realtime_current_zonal_and_lmp() {
    let cl = client();
    let zonal = cl
        .realtime_ontario_zonal_prices_current()
        .await
        .expect("zonal");
    let last = zonal
        .body
        .published()
        .last()
        .map(|(t, i)| (t.to_rfc3339(), i.lmp));
    println!(
        "RT Ontario zonal latest published interval: {last:?} (date {} HE{})",
        zonal.body.delivery_date, zonal.body.delivery_hour
    );
    assert!(last.is_some());
    let virt = cl
        .realtime_virtual_zonal_prices_current()
        .await
        .expect("virtual");
    println!("RT virtual zones: {}", virt.body.zones.len());
    assert!(!virt.body.zones.is_empty());
    let lmp = cl.realtime_lmp_current().await.expect("lmp");
    println!("RT LMP rows: {}", lmp.rows.len());
    assert!(!lmp.rows.is_empty());
}

#[tokio::test]
#[ignore = "network"]
async fn live_demand_2026_has_first_hour_of_the_year() {
    let report = client().demand(2026).await.expect("fetch");
    let first = &report.rows[0];
    println!(
        "Demand 2026 first row: {:?}; total rows {}",
        first,
        report.rows.len()
    );
    assert_eq!((first.date, first.hour), (d(2026, 1, 1), 1));
    assert!(first.ontario_demand.is_some());
}

#[tokio::test]
#[ignore = "network"]
async fn live_generation_intertie_and_adequacy() {
    let cl = client();
    let goc = cl
        .generator_output_capability(d(2026, 11, 17))
        .await
        .expect("goc");
    let gas: f64 = goc.body.output_by_fuel("GAS").iter().sum();
    println!(
        "GenOutputCapability 2026-11-17: {} generators, gas MWh = {gas:.0}",
        goc.body.generators.len()
    );
    assert!(goc.body.generators.len() > 100);

    let fuel = cl.generation_by_fuel(2026).await.expect("fuel");
    let jan1 = &fuel.body.days[0];
    let nuclear = jan1.hours[0]
        .fuels
        .iter()
        .find(|f| f.fuel == "NUCLEAR")
        .and_then(|f| f.output_mw);
    println!(
        "GenOutputbyFuelHourly 2026-01-01 HE1 nuclear = {nuclear:?} MW; days {}",
        fuel.body.days.len()
    );
    assert!(nuclear.is_some());

    let isf = cl
        .intertie_schedule_flow(d(2026, 11, 17))
        .await
        .expect("isf");
    println!(
        "IntertieScheduleFlow zones: {:?}",
        isf.body
            .zones
            .iter()
            .map(|z| z.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(isf.body.zone("MICHIGAN").is_some());

    let adq = cl.adequacy(d(2026, 11, 17)).await.expect("adequacy");
    println!(
        "Adequacy3 2026-11-17 forecast Ontario demand HE1 = {:?} MW",
        adq.body
            .demand
            .ontario_demand
            .forecast
            .first()
            .and_then(|v| v.value)
    );
    assert_eq!(adq.body.supply.capacities.len(), 24);
}

#[tokio::test]
#[ignore = "network"]
async fn live_listing_and_latest_version_selection() {
    let cl = client();
    let reports = cl.list_reports().await.expect("root");
    println!("report directories: {}", reports.len());
    assert!(reports
        .iter()
        .any(|r| r.name == "DAHourlyOntarioZonalPrice"));
    let spec = FileSpec::of(
        Report::DaHourlyOntarioZonalPrice,
        Stamp::Day(d(2026, 11, 17)),
    )
    .version(Version::HighestListed);
    let got = cl.fetch(&spec).await.expect("highest listed");
    println!(
        "highest listed revision of DA zonal 2026-11-17: v{:?} ({})",
        got.revision, got.url
    );
    assert!(got.revision.is_some());
}

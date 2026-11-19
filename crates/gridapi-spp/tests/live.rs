//! Live checks against the SPP portal. Run with `cargo test -p gridapi-spp -- --ignored --nocapture`.
//!
//! Each test prints sample values; those are what the README's "Verified" section records.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

use chrono::{Datelike, NaiveDate};
use gridapi_spp::{
    at_location, BalancingArea, Chart, Client, EntryKind, GenMixWindow, DA_LMP_BY_LOCATION,
    HOURLY_LOAD, SPP_HUBS,
};

fn client() -> Client {
    Client::builder()
        .user_agent("gridapi-spp-live-tests (+https://example.invalid)")
        .build()
        .expect("client")
}

#[tokio::test]
#[ignore = "network"]
async fn live_anonymous_listing_and_discovery() {
    let c = client();
    let years = c.list(&DA_LMP_BY_LOCATION, "/").await.expect("list years");
    println!(
        "DA years: {:?}",
        years.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()
    );
    assert!(years
        .iter()
        .any(|e| e.name == "2026" && e.kind == EntryKind::Folder));
    let pages = c.file_browser_pages().await.expect("menu");
    println!("file-browser pages: {}", pages.len());
    assert!(pages.len() > 50);
    assert!(pages
        .iter()
        .any(|p| p.source.as_str() == "rtbm-lmp-by-location"));
}

#[tokio::test]
#[ignore = "network"]
async fn live_day_ahead_lmp() {
    let c = client();
    let date = NaiveDate::from_ymd_opt(2026, 11, 17).expect("date");
    let rows = c.da_lmp_by_location(date).await.expect("da 2026-11-17");
    let crsp = at_location(&rows, "CRSP_HUB")
        .find(|p| p.interval_end.format("%H:%M").to_string() == "01:00")
        .expect("CRSP_HUB 01:00");
    println!(
        "DA 2026-11-17 rows={} CRSP_HUB 01:00 LMP={} MLC={:?} MCC={:?} (utc {})",
        rows.len(),
        crsp.lmp,
        crsp.mlc,
        crsp.mcc,
        crsp.interval_end_utc
    );
    assert!(
        crsp.lmp.is_finite(),
        "CRSP_HUB LMP must be a number, got {}",
        crsp.lmp
    );
    for hub in SPP_HUBS {
        let hrs = at_location(&rows, hub).count();
        let first = at_location(&rows, hub).next().map(|p| p.lmp);
        println!("  {hub}: {hrs} hours, HE1 LMP={first:?}");
        assert_eq!(hrs, 24);
    }
    let (entry, latest) = c.latest_da_lmp_by_location().await.expect("latest da");
    println!(
        "latest DA file {} ({} rows, modified {:?})",
        entry.name,
        latest.len(),
        entry.modified
    );
    assert!(latest.len() > 10_000);
}

#[tokio::test]
#[ignore = "network"]
async fn live_real_time_lmp() {
    let c = client();
    let (entry, rows) = c.latest_rt_lmp_by_location().await.expect("latest rt");
    println!("latest RT file {} rows={}", entry.name, rows.len());
    let north = at_location(&rows, "SPPNORTH_HUB")
        .next()
        .expect("SPPNORTH_HUB");
    println!(
        "RT {} SPPNORTH_HUB LMP={} MCC={:?} MLC={:?}",
        north.interval_end, north.lmp, north.mcc, north.mlc
    );
    assert!(rows.len() > 1000);
    let again = c
        .rt_lmp_by_location(north.interval_end)
        .await
        .expect("by timestamp");
    assert_eq!(again.len(), rows.len());
}

#[tokio::test]
#[ignore = "network"]
async fn live_load_generation_mix_and_charts() {
    let c = client();
    let load = c
        .hourly_load_monthly(2026, 10)
        .await
        .expect("monthly load Oct 2026");
    let spp: f64 = load
        .iter()
        .filter(|z| z.balancing_area.as_deref() == Some("SPP") && z.hour_end_utc.month() == 10)
        .filter(|z| z.hour_end_utc.day() == 15 && z.hour_end_utc.format("%H").to_string() == "18")
        .map(|z| z.mw)
        .sum();
    println!(
        "load Oct 2026 rows={} SPP total at 2026-10-15 18:00Z = {spp:.1} MW",
        load.len()
    );
    assert!(spp > 10_000.0);
    let daily = c
        .hourly_load_daily(NaiveDate::from_ymd_opt(2026, 11, 16).expect("date"))
        .await
        .expect("daily load");
    println!("daily load 2026-11-16 rows={}", daily.len());
    assert_ne!(daily.len(), 0, "no daily load rows");

    for area in [BalancingArea::Spp, BalancingArea::Swpw] {
        let mix = c
            .generation_mix(area, GenMixWindow::TwoHour)
            .await
            .expect("mix");
        let last = mix.last().expect("a sample");
        println!(
            "{area:?} gen mix last={} fuels={} total={:.1} MW load={:?}",
            last.interval_utc,
            last.fuels.len(),
            last.total_mw(),
            last.load_mw
        );
        assert!(mix.len() >= 20);
    }
    let chart = c.chart(Chart::LoadForecast).await.expect("chart");
    println!(
        "chart load-forecast times={} latest actual={:?}",
        chart.times.len(),
        chart.latest("Actual Load")
    );
    let ace = c.chart(Chart::Ace).await.expect("ace chart");
    println!(
        "chart ace series={:?}",
        ace.series
            .iter()
            .map(|s| s.label.as_str())
            .collect::<Vec<_>>()
    );
    let listing = c.list(&HOURLY_LOAD, "/2026").await.expect("list load");
    assert!(listing.iter().any(|e| e.name.starts_with("HOURLY_LOAD-")));
}

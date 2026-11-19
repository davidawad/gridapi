//! Live checks against MISO's public hosts. Run with `cargo test -p gridapi-miso -- --ignored --nocapture`.
//!
//! Each test prints sample values; those are what the README's "Verified" section records.
//!
//! NEVER run these in public CI: they hit MISO's website hosts, whose terms bar automated access. Use your own credentials on your own machine.

#![cfg(feature = "miso-public-web")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

use chrono::{Duration, NaiveDate, Utc};
use gridapi_miso::{
    Client, Component, Error, FiveMinuteWindow, FuelMixView, LmpReportKind, NodeType, Renewable,
};

fn client() -> Client {
    Client::builder()
        .user_agent("gridapi-miso-live-tests (+https://example.invalid)")
        .build()
        .expect("client")
}

/// MISO's market day is on EST (UTC-5).
fn market_today() -> NaiveDate {
    (Utc::now() - Duration::hours(5)).date_naive()
}

#[tokio::test]
#[ignore = "network"]
async fn live_ex_ante_lmp() {
    let p = client().ex_ante_lmp().await.expect("ex-ante");
    let hub = p.hub("ARKANSAS.HUB").expect("ARKANSAS.HUB");
    println!(
        "ex-ante {} {} LMP={} loss={} cong={}",
        p.ref_id, hub.name, hub.lmp, hub.loss, hub.congestion
    );
    assert!(p.hubs.len() >= 8);
    assert!(hub.lmp.abs() < 5000.0);
}

#[tokio::test]
#[ignore = "network"]
async fn live_consolidated_table_and_five_minute() {
    let c = client();
    let t = c.lmp_consolidated_table().await.expect("table");
    println!(
        "consolidated {} five-min nodes={} da-expost block={} nodes={}",
        t.ref_id,
        t.five_minute.nodes.len(),
        t.day_ahead_ex_post.label,
        t.day_ahead_ex_post.nodes.len()
    );
    assert!(t.five_minute.nodes.len() > 100);
    let current = c
        .five_minute_ex_post_lmp(FiveMinuteWindow::Current)
        .await
        .expect("current");
    println!(
        "five-min current rows={} first={:?}",
        current.len(),
        current.first()
    );
    assert!(current.len() > 1000);
    let prev = c
        .five_minute_ex_post_lmp(FiveMinuteWindow::Previous)
        .await
        .expect("previous");
    assert!(prev.len() > 1000);
    assert!(prev[0].interval < current[0].interval);
}

#[tokio::test]
#[ignore = "network"]
async fn live_system_conditions() {
    let c = client();
    let mix = c.fuel_mix(FuelMixView::Latest).await.expect("fuel mix");
    let sum: f64 = mix.entries.iter().map(|e| e.mw).sum();
    println!(
        "fuel mix {} total={} sum-of-parts={sum}",
        mix.ref_id, mix.total_mw
    );
    assert!(mix.entries.len() >= 6);
    let load = c.total_load().await.expect("load");
    println!(
        "load {} cleared[0]={:?} actual.last={:?}",
        load.ref_id,
        load.cleared.first(),
        load.actual.last()
    );
    assert_eq!(load.cleared.len(), 24);
    let wind = c.forecast_vs_actual(Renewable::Wind).await.expect("wind");
    println!("wind hours={} first={:?}", wind.len(), wind.first());
    assert!(wind.len() >= 24);
    let mcp = c.ancillary_services_mcp().await.expect("mcp");
    println!("mcp {} zones={}", mcp.ref_id, mcp.zones.len());
    println!("NAI {:?}", c.net_actual_interchange().await.expect("nai"));
    println!(
        "NSI {:?}",
        c.net_scheduled_interchange().await.expect("nsi").flows_mw
    );
    println!("snapshot {:?}", c.snapshot().await.expect("snapshot"));
    println!(
        "constraints {}",
        c.binding_constraints().await.expect("bc").len()
    );
    println!("ace samples {}", c.ace().await.expect("ace").len());
}

#[tokio::test]
#[ignore = "network"]
async fn live_market_reports() {
    let c = client();
    let day = market_today() - Duration::days(2);
    for kind in [
        LmpReportKind::DayAheadExPost,
        LmpReportKind::DayAheadExAnte,
        LmpReportKind::RealTimePrelim,
    ] {
        let r = c.lmp_report(kind, day).await.expect("report");
        println!(
            "{kind:?} {day}: rows={} ARKANSAS.HUB HE1={:?} hubs={}",
            r.rows.len(),
            r.lmp("ARKANSAS.HUB", 1),
            r.lmp_rows(&NodeType::Hub).count()
        );
        assert_eq!(r.market_date, day);
        assert!(r.rows.len() > 1000);
        assert!(r.series("ARKANSAS.HUB", Component::Lmp).is_some());
    }
    let old = NaiveDate::from_ymd_opt(2026, 6, 1).expect("date");
    let fin = c
        .lmp_report(LmpReportKind::RealTimeFinal, old)
        .await
        .expect("final");
    println!(
        "RealTimeFinal {old}: ARKANSAS.HUB HE1={:?}",
        fin.lmp("ARKANSAS.HUB", 1)
    );
}

#[tokio::test]
#[ignore = "network"]
async fn live_retired_broker() {
    let c = client();
    let err = c.data_broker("getfuelmix").await.expect_err("retired");
    println!("broker: {err}");
    assert!(matches!(err, Error::NoData { .. }));
}

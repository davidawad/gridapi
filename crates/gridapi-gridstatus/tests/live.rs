//! Live tests against api.gridstatus.io. All `#[ignore]`: they spend real quota
//! (free tier: 250 requests and 500k rows per month). Run one at a time:
//!
//! ```text
//! GRIDSTATUS_API_KEY=... cargo test -p gridapi-gridstatus --test live -- --ignored <name>
//! ```
//!
//! Each test builds a client with a hard request budget equal to the requests
//! it expects to need, so a bug cannot burn the quota.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gridapi_gridstatus::{Client, Iso, Query};

fn client(budget: u64) -> Client {
    Client::builder()
        .api_key_from_env()
        .request_budget(budget)
        .build()
        .expect("set GRIDSTATUS_API_KEY to run live tests")
}

/// 1 request. ERCOT day-ahead `HB_HOUSTON` at 2026-11-17T00:00Z returns a price;
/// historical day-ahead prices do not change.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn ercot_day_ahead_hb_houston() {
    let c = client(1);
    let page = c
        .ercot_spp_day_ahead_hourly(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T01:00:00Z")
                .location("HB_HOUSTON"),
        )
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    let row = &page.rows[0];
    println!("{row:?}\nresponse: {:?}", page.response);
    assert_eq!(row.location, "HB_HOUSTON");
    assert!(row.spp.is_some());
}

/// 1 request. CAISO day-ahead LMP rows carry price components.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn caiso_day_ahead_lmp_components() {
    let c = client(1);
    let page = c
        .caiso_lmp_day_ahead_hourly(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T01:00:00Z")
                .limit(3),
        )
        .await
        .unwrap();
    println!("{:#?}\nresponse: {:?}", page.rows, page.response);
    assert_ne!(page.rows.len(), 0, "expected CAISO rows");
    for r in &page.rows {
        let lmp = r.lmp.expect("lmp present");
        let parts = r.energy.unwrap_or(0.0)
            + r.congestion.unwrap_or(0.0)
            + r.loss.unwrap_or(0.0)
            + r.ghg.unwrap_or(0.0);
        assert!((lmp - parts).abs() < 0.05, "{r:?}");
    }
}

/// 1 request.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn usage_reports_free_plan() {
    let u = client(1).usage().await.unwrap();
    println!("{u:?} remaining_requests={}", u.remaining_requests());
    assert!(u.limits.api_requests_limit >= 250);
}

/// 1 request, returns the whole catalog (~1 MB).
#[tokio::test]
#[ignore = "live: 1 request, ~1 MB"]
async fn catalog_lists_standard_datasets() {
    let list = client(1).list_datasets().await.unwrap();
    println!("{} datasets", list.datasets.len());
    for iso in Iso::ALL {
        assert!(
            list.get(&iso.fuel_mix_dataset()).is_some(),
            "{iso:?} fuel mix"
        );
        assert!(list.get(&iso.load_dataset()).is_some(), "{iso:?} load");
    }
}

/// 1 request.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn dataset_metadata() {
    let ds = client(1)
        .dataset("ercot_spp_day_ahead_hourly")
        .await
        .unwrap();
    assert_eq!(ds.time_index_column.as_deref(), Some("interval_start_utc"));
}

/// 1 request.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn ercot_real_time_15_minute_hb_houston() {
    let page = client(1)
        .ercot_spp_real_time_15_minute(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T01:00:00Z")
                .location("HB_HOUSTON"),
        )
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 4);
    assert!(page.rows[0].spp.is_some());
}

/// 1 request.
#[tokio::test]
#[ignore = "live: 1 request"]
async fn ercot_fuel_mix_and_load() {
    let c = client(2);
    let q = Query::new()
        .start_time("2026-11-17T00:00:00Z")
        .end_time("2026-11-17T00:15:00Z");
    let mix = c.fuel_mix(Iso::Ercot, &q).await.unwrap();
    let load = c.load(Iso::Ercot, &q).await.unwrap();
    assert_eq!(mix.rows.len(), 3);
    assert!(load.rows[0].load.is_some());
}

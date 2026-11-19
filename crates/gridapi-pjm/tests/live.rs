//! Live tests against api.pjm.com. Ignored by default (network, and PJM's
//! non-member limit of 6 requests per minute makes them slow):
//!
//! ```text
//! cargo test -p gridapi-pjm --test live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! They use the public web-app key unless `PJM_API_KEY` is set (then
//! `live_public_key_end_to_end` still forces the public key; set the variable
//! to exercise `live_user_key_mode` with your own key).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use chrono::{Duration, NaiveDate, Utc};
use gridapi_pjm::{
    fetch_public_key, ApiKey, Client, Error, KeyMode, LmpRequest, LoadRequest, PnodeRequest, Query,
    Row, DEFAULT_SETTINGS_URL,
};

/// Western Hub, the benchmark node.
const WESTERN_HUB: u64 = 51288;

fn days_ago(n: i64) -> NaiveDate {
    (Utc::now() - Duration::days(n)).date_naive()
}

fn public_client() -> Client {
    Client::builder()
        .key_mode(KeyMode::PublicWebApp)
        .build()
        .expect("client builds")
}

#[tokio::test]
#[ignore = "network: api.pjm.com"]
async fn live_public_key_end_to_end() {
    let client = public_client();

    // Catalog and metadata.
    let feeds = client.feeds().await.unwrap();
    println!("feeds: {}", feeds.len());
    assert!(feeds.len() > 100);
    for name in [
        "da_hrl_lmps",
        "rt_hrl_lmps",
        "rt_fivemin_hrl_lmps",
        "hrl_load_metered",
        "load_frcstd_7_day",
        "gen_by_fuel",
        "pnode",
    ] {
        assert!(feeds.iter().any(|f| f.name == name), "feed {name} listed");
    }
    let md = client.feed_metadata("da_hrl_lmps").await.unwrap();
    println!(
        "da_hrl_lmps: {} columns, archive after {:?} days, last load {:?}",
        md.columns.len(),
        md.archive_cutoff_days,
        md.last_data_load
    );
    assert!(md.filterable().any(|c| c.field_name == "pnode_id"));
    assert!(md
        .sortable()
        .any(|c| c.field_name == "datetime_beginning_ept"));

    // Day-ahead LMP: PJM published Western Hub 2026-11-17 hour 00 EPT (05Z).
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
    let da = client
        .da_hrl_lmps(LmpRequest::new(day).pnode_id(WESTERN_HUB))
        .await
        .unwrap();
    println!(
        "DA {} {} utc {} = ${:.6}",
        da[0].pnode_name,
        da[0].datetime_beginning_ept,
        da[0].datetime_beginning_utc,
        da[0].total_lmp_da
    );
    assert_eq!(da.len(), 24);
    assert_eq!(
        da[0].datetime_beginning_utc.to_string(),
        "2026-11-17 05:00:00"
    );
    assert!(
        (da[0].total_lmp_da - 101.50).abs() < 0.01,
        "got {}",
        da[0].total_lmp_da
    );

    // Verified real-time hourly and five-minute LMPs, a few days back.
    let rt_day = days_ago(4);
    let rt = client
        .rt_hrl_lmps(LmpRequest::new(rt_day).pnode_id(WESTERN_HUB))
        .await
        .unwrap();
    println!(
        "RT hourly {rt_day}: {} rows, first ${:.2}",
        rt.len(),
        rt[0].total_lmp_rt
    );
    assert_eq!(rt.len(), 24);

    // Five-minute rows: 288 on a normal day, 276/300 on a DST change.
    let five = client
        .rt_fivemin_hrl_lmps(LmpRequest::new(rt_day).pnode_id(WESTERN_HUB))
        .await
        .unwrap();
    println!("RT 5-min {rt_day}: {} rows", five.len());
    assert!(
        five.len() == 288 || five.len() == 276 || five.len() == 300,
        "DST-aware day length"
    );

    // Metered load, forecasts, generation mix.
    let load = client
        .hrl_load_metered(LoadRequest::new(days_ago(5)).zone("AE"))
        .await
        .unwrap();
    println!(
        "metered load AE {}: {} rows, first {} MW",
        days_ago(5),
        load.len(),
        load[0].mw
    );
    assert!(!load.is_empty() && load.iter().all(|r| r.mw > 0.0));

    let fc = client
        .load_frcstd_7_day(gridapi_pjm::ForecastRequest::new().area("RTO_COMBINED"))
        .await
        .unwrap();
    println!(
        "7-day forecast RTO_COMBINED: {} rows, first {} MW at {}",
        fc.len(),
        fc[0].forecast_load_mw,
        fc[0].forecast_datetime_beginning_ept
    );
    assert!(fc.len() >= 160);
    assert!(fc.iter().all(|r| r.forecast_load_mw > 30_000.0));

    let gen = client.gen_by_fuel(days_ago(1)).await.unwrap();
    let total: f64 = gen
        .iter()
        .filter(|g| g.datetime_beginning_utc == gen[0].datetime_beginning_utc)
        .map(|g| g.mw)
        .sum();
    println!(
        "gen_by_fuel {}: {} rows, first-hour total {total:.0} MW",
        days_ago(1),
        gen.len()
    );
    assert!(gen.iter().any(|g| g.fuel_type == "Nuclear"));

    // Pnode lookup.
    let hub = client
        .pnode(WESTERN_HUB)
        .await
        .unwrap()
        .expect("western hub exists");
    println!(
        "pnode {}: {} {:?}/{:?}",
        hub.pnode_id, hub.pnode_name, hub.pnode_type, hub.pnode_subtype
    );
    assert_eq!(hub.pnode_name, "WESTERN HUB");
    assert!(hub.is_active());
    let by_name = client
        .pnodes(PnodeRequest::new().name("WESTERN HUB"))
        .await
        .unwrap();
    assert_eq!(by_name[0].pnode_id, WESTERN_HUB);

    // A query PJM rejects comes back as a typed error naming the problem.
    let err = client
        .da_hrl_lmps(Query::new().pnode_ids([WESTERN_HUB]))
        .await
        .unwrap_err();
    println!("missing datetime -> {err}");
    assert!(
        matches!(&err, Error::Rejected { errors, .. } if errors[0].message.contains("datetime"))
    );
}

#[tokio::test]
#[ignore = "network: api.pjm.com"]
async fn live_five_minute_load_forecast_and_paging() {
    let client = public_client();
    let rows = client
        .very_short_load_frcst(
            Query::new()
                .filter("evaluated_at_ept", "CurrentHour")
                .filter("forecast_area", "RTO_COMBINED"),
        )
        .await
        .unwrap();
    println!("5-min load forecast, current hour: {} rows", rows.len());
    assert_ne!(rows.len(), 0, "current-hour forecast has rows");

    // Explicit paging on an untyped query: 7-day forecast, 50 rows per page.
    let all: Vec<Row> = client
        .query_all(
            "load_frcstd_7_day",
            &Query::new()
                .row_count(50)
                .filter("forecast_area", "RTO_COMBINED"),
        )
        .await
        .unwrap();
    println!("paged 7-day forecast: {} rows over 50-row pages", all.len());
    assert!(all.len() >= 160);
}

#[tokio::test]
#[ignore = "network: api.pjm.com and dataminer2.pjm.com"]
async fn live_user_key_mode() {
    // Use the key you provisioned when PJM_API_KEY is set; otherwise borrow the
    // public key so the user-key code path (no settings.json refetch) still runs.
    let key = match std::env::var("PJM_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
    {
        Some(k) => ApiKey::new(k).unwrap(),
        None => fetch_public_key(DEFAULT_SETTINGS_URL).await.unwrap(),
    };
    let client = Client::builder().api_key(key).build().unwrap();
    let hub = client.pnode(WESTERN_HUB).await.unwrap().unwrap();
    println!("user-key mode: pnode {} = {}", hub.pnode_id, hub.pnode_name);
    assert_eq!(hub.pnode_name, "WESTERN HUB");

    // A wrong user key is reported, not silently swapped for the public key.
    let bad = Client::builder()
        .api_key(ApiKey::new("not-a-real-key").unwrap())
        .build()
        .unwrap();
    let err = bad.pnode(WESTERN_HUB).await.unwrap_err();
    println!("bad key -> {err}");
    assert!(err.is_unauthorized());
}

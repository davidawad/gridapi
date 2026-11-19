//! Live tests against api.pjm.com. Ignored by default (network, and PJM's
//! non-member limit of 6 requests per minute makes them slow):
//!
//! ```text
//! PJM_API_KEY=<your key> cargo test -p gridapi-pjm --test live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! NEVER run these in public CI: they need your own PJM key (register at
//! <https://apiportal.pjm.com/>; keys must not be shared or leaked into logs)
//! and PJM limits non-members to 6 requests per minute.
//!
//! They need `PJM_API_KEY`. Only when the crate is built with the opt-in
//! `pjm-public-web-key` feature and `PJM_API_KEY` is unset do they fall back
//! to the public key PJM's own web app ships, behind the explicit
//! `i_accept_pjm_public_key_risk()` acknowledgement; that key is not issued
//! to you (see the PJM terms quoted on `KeyMode::PublicWebApp`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use chrono::{Duration, NaiveDate, Utc};
use gridapi_pjm::{ApiKey, Client, Error, LmpRequest, LoadRequest, PnodeRequest, Query, Row};

/// Western Hub, the benchmark node.
const WESTERN_HUB: u64 = 51288;

fn days_ago(n: i64) -> NaiveDate {
    (Utc::now() - Duration::days(n)).date_naive()
}

/// A client on `PJM_API_KEY`; with the opt-in `pjm-public-web-key` feature and
/// no `PJM_API_KEY`, on the public web-app key (explicitly acknowledged).
fn live_client() -> Client {
    let builder = Client::builder();
    #[cfg(feature = "pjm-public-web-key")]
    let builder = if std::env::var(gridapi_pjm::API_KEY_ENV)
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_none()
    {
        builder
            .key_mode(gridapi_pjm::KeyMode::PublicWebApp)
            .i_accept_pjm_public_key_risk()
    } else {
        builder
    };
    builder
        .build()
        .expect("set PJM_API_KEY (register at https://apiportal.pjm.com/)")
}

#[tokio::test]
#[ignore = "network: api.pjm.com"]
async fn live_end_to_end() {
    let client = live_client();

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
    let client = live_client();
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
#[ignore = "network: api.pjm.com (and dataminer2.pjm.com with pjm-public-web-key)"]
async fn live_user_key_mode() {
    // Use the key you provisioned. Only with the opt-in `pjm-public-web-key`
    // feature and no PJM_API_KEY, borrow the public key so the user-key code
    // path (no settings.json refetch) still runs.
    let provisioned = std::env::var("PJM_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty());
    #[cfg(feature = "pjm-public-web-key")]
    let key = match provisioned {
        Some(k) => ApiKey::new(k).unwrap(),
        None => gridapi_pjm::fetch_public_key(gridapi_pjm::DEFAULT_SETTINGS_URL)
            .await
            .unwrap(),
    };
    #[cfg(not(feature = "pjm-public-web-key"))]
    let key = ApiKey::new(provisioned.expect("set PJM_API_KEY")).unwrap();
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

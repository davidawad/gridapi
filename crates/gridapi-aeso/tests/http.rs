//! Client behaviour against a scripted local server: key header, query
//! strings, retry with backoff, error mapping. The key used here is a dummy.
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

mod support;

use std::time::Duration;

use chrono::NaiveDate;
use gridapi_aeso::{
    AssetListQuery, Client, DataType, DateRange, Error, InterchangeQuery, Intertie, OutageQuery,
    RetryPolicy, TransferType,
};
use support::{serve, Canned};

const KEY: &str = "dummy-key-for-tests";
const POOL: &str = include_str!("fixtures/pool_price_20261116.json");
const POOL_PATH: &str = "/public/poolprice-api/v1.1/price/poolPrice";

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
}

fn fast_retry(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn client(base: &str, retry: RetryPolicy) -> Client {
    Client::builder()
        .api_key(KEY)
        .base_url(base)
        .retry(retry)
        .build()
        .expect("client builds")
}

#[tokio::test]
async fn pool_price_sends_the_api_key_header_and_date_params() {
    let server = serve(vec![(POOL_PATH, vec![Canned::ok(POOL)])]).await;
    let rows = client(&server.base, fast_retry(1))
        .pool_price(DateRange::new(d(2026, 11, 16), d(2026, 11, 16)).expect("range"))
        .await
        .expect("fetches");
    assert_eq!(rows[0].pool_price, Some(1.08));
    let seen = server.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].header("api-key"), Some(KEY));
    assert_eq!(
        seen[0].path,
        format!("{POOL_PATH}?startDate=2026-11-16&endDate=2026-11-16")
    );
}

#[tokio::test]
async fn a_single_day_range_omits_end_date() {
    let server = serve(vec![(POOL_PATH, vec![Canned::ok(POOL)])]).await;
    client(&server.base, fast_retry(1))
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("fetches");
    assert_eq!(
        server.requests()[0].path,
        format!("{POOL_PATH}?startDate=2026-11-16")
    );
}

#[tokio::test]
async fn user_agent_defaults_to_the_crate_and_is_overridable() {
    let server = serve(vec![(POOL_PATH, vec![Canned::ok(POOL)])]).await;
    client(&server.base, fast_retry(1))
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("ok");
    Client::builder()
        .api_key(KEY)
        .base_url(format!("{}/", server.base))
        .user_agent("my-bot/2")
        .retry(fast_retry(1))
        .build()
        .expect("builds")
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("ok");
    let seen = server.requests();
    assert!(seen[0]
        .header("user-agent")
        .is_some_and(|u| u.starts_with("gridapi-aeso/")));
    assert_eq!(seen[1].header("user-agent"), Some("my-bot/2"));
}

#[test]
fn building_without_a_key_fails_with_a_clear_error() {
    assert!(matches!(
        Client::builder().build(),
        Err(Error::MissingApiKey)
    ));
    assert!(matches!(
        Client::builder().api_key("  ").build(),
        Err(Error::MissingApiKey)
    ));
    assert!(matches!(
        Client::builder().api_key("bad\nkey").build(),
        Err(Error::InvalidApiKey)
    ));
}

#[test]
fn debug_output_never_contains_the_key() {
    let c = client("http://localhost:1/public", RetryPolicy::none());
    assert!(!format!("{c:?}").contains(KEY));
    let b = Client::builder().api_key(KEY);
    assert!(!format!("{b:?}").contains(KEY));
}

#[tokio::test]
async fn api_400s_become_typed_errors_with_the_message() {
    let body = include_str!("fixtures/error_no_data.json");
    let server = serve(vec![(
        "/public/energymeritorder-api/v1/meritOrder/energy",
        vec![Canned {
            status: 400,
            headers: vec![],
            body: body.to_owned(),
        }],
    )])
    .await;
    let err = client(&server.base, fast_retry(3))
        .energy_merit_order(d(2026, 10, 15))
        .await
        .expect_err("no data");
    assert!(err.is_no_data(), "{err}");
    assert!(
        matches!(&err, Error::Api { status: 400, message, .. } if message.starts_with("No Data"))
    );
    assert_eq!(server.requests().len(), 1, "400 is not retried");
}

#[tokio::test]
async fn the_gateways_404_mentions_the_key() {
    let server = serve(vec![(
        POOL_PATH,
        vec![Canned {
            status: 404,
            headers: vec![],
            body: include_str!("fixtures/error_gateway_404.json").to_owned(),
        }],
    )])
    .await;
    let err = client(&server.base, fast_retry(3))
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect_err("404");
    assert!(matches!(err, Error::NotFound { .. }));
    assert!(err.to_string().contains("API-KEY"));
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn throttling_is_retried_with_backoff_then_succeeds() {
    let server = serve(vec![(
        POOL_PATH,
        vec![
            Canned::status(429).header("Retry-After", "0"),
            Canned::status(503),
            Canned::ok(POOL),
        ],
    )])
    .await;
    let rows = client(&server.base, fast_retry(4))
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("third attempt wins");
    assert_eq!(rows.len(), 24);
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn persistent_5xx_gives_up_at_max_attempts_reporting_the_status() {
    let server = serve(vec![(POOL_PATH, vec![Canned::status(502)])]).await;
    let err = client(&server.base, fast_retry(3))
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect_err("gives up");
    assert!(matches!(err, Error::Status { status: 502, .. }), "{err}");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn interchange_query_reaches_the_itc_path_with_compact_dates() {
    let server = serve(vec![(
        "/public/itc/v1/interchange",
        vec![Canned::ok(include_str!("fixtures/itc_interchange.json"))],
    )])
    .await;
    let q = InterchangeQuery::new(DateRange::day(d(2026, 11, 16)))
        .hours(1, 2)
        .expect("hours")
        .data_type(DataType::Atc)
        .intertie(Intertie::Bc)
        .transfer(TransferType::Import);
    let report = client(&server.base, fast_retry(1))
        .interchange_capability(&q)
        .await
        .expect("ok");
    assert!(report.bc.is_some());
    let path = &server.requests()[0].path;
    for want in [
        "startDate=20261116",
        "endDate=20261116",
        "startHE=1",
        "endHE=2",
        "version=false",
        "dataType=ATC",
        "intertieOrFlowgate=BC",
        "transferType=IMPORT",
    ] {
        assert!(path.contains(want), "{want} in {path}");
    }
}

#[tokio::test]
async fn outages_and_asset_list_use_their_own_paths_and_filters() {
    let server = serve(vec![
        (
            "/public/itc/v1/outage",
            vec![Canned::ok(include_str!("fixtures/itc_outages.json"))],
        ),
        (
            "/public/assetlist-api/v1/assetlist",
            vec![Canned::ok(include_str!("fixtures/asset_list.json"))],
        ),
    ])
    .await;
    let cl = client(&server.base, fast_retry(1));
    let outages = cl
        .intertie_outages(
            &OutageQuery::new(DateRange::new(d(2026, 9, 1), d(2026, 11, 17)).expect("range"))
                .intertie(Intertie::Bc)
                .expect("BC is allowed"),
        )
        .await
        .expect("ok");
    assert_eq!(outages.len(), 3);
    let assets = cl
        .asset_list(&AssetListQuery::all().asset_id("AST1"))
        .await
        .expect("ok");
    assert_eq!(assets[0].asset_id, "AST1");
    let paths = server.paths();
    assert!(
        paths[0].contains("affectedIntertieOrFlowgate=BC")
            && paths[0].contains("startDate=20260901")
    );
    assert!(paths[1].ends_with("assetlist?asset_ID=AST1"));
}

#[tokio::test]
async fn parameterless_endpoints_send_no_query_string() {
    let server = serve(vec![
        (
            "/public/currentsupplydemand-api/v2/csd/summary/current",
            vec![Canned::ok(include_str!("fixtures/csd_summary_v2.json"))],
        ),
        (
            "/public/currentsupplydemand-api/v1/csd/generation/assets/current",
            vec![Canned::ok(include_str!(
                "fixtures/csd_generation_assets.json"
            ))],
        ),
        (
            "/public/systemmarginalprice-api/v1.1/price/systemMarginalPrice/current",
            vec![Canned::ok(include_str!(
                "fixtures/system_marginal_price_current.json"
            ))],
        ),
    ])
    .await;
    let cl = client(&server.base, fast_retry(1));
    assert_eq!(
        cl.current_supply_demand()
            .await
            .expect("csd")
            .alberta_internal_load,
        Some(8054.0)
    );
    assert_eq!(
        cl.generation_assets_current()
            .await
            .expect("assets")
            .asset_list
            .len(),
        9
    );
    assert_eq!(
        cl.system_marginal_price_current().await.expect("smp")[0].system_marginal_price,
        Some(19.74)
    );
    assert!(server.paths().iter().all(|p| !p.contains('?')));
}

#[tokio::test]
async fn raw_json_access_reaches_endpoints_without_a_typed_method() {
    let server = serve(vec![(
        "/public/some-api/v9/thing",
        vec![Canned::ok(r#"{"return":[1,2]}"#)],
    )])
    .await;
    let v = client(&server.base, fast_retry(1))
        .get_json("/some-api/v9/thing", &[("x", "1".to_owned())])
        .await
        .expect("ok");
    assert_eq!(v["return"][1], 2);
    assert_eq!(server.requests()[0].path, "/public/some-api/v9/thing?x=1");
}

#[tokio::test]
async fn an_unreachable_server_is_a_transport_error_without_the_key_in_it() {
    let cl = Client::builder()
        .api_key(KEY)
        .base_url("http://127.0.0.1:1/public")
        .connect_timeout(Duration::from_millis(200))
        .retry(fast_retry(2))
        .build()
        .expect("builds");
    let err = cl
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect_err("refused");
    assert!(matches!(err, Error::Transport { .. }), "{err}");
    assert!(!err.to_string().contains(KEY));
}

#[test]
fn merit_order_latest_date_is_sixty_days_back() {
    assert_eq!(
        gridapi_aeso::merit_order_latest_date(d(2026, 11, 18)),
        d(2026, 9, 19)
    );
}

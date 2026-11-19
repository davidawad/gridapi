//! Client behaviour against a scripted local server: URL construction,
//! version selection, retry with backoff, error mapping.
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

mod support;

use std::time::Duration;

use chrono::NaiveDate;
use gridapi_ieso::{Client, Error, FileSpec, Report, RetryPolicy, Stamp, Version};
use support::{serve, Canned};

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

const DA: &str = include_str!("fixtures/da_ontario_zonal_20261117.xml");
const DA_PATH: &str =
    "/public/DAHourlyOntarioZonalPrice/PUB_DAHourlyOntarioZonalPrice_20261117.xml";

fn client(base: &str, retry: RetryPolicy) -> Client {
    Client::builder()
        .base_url(base)
        .retry(retry)
        .build()
        .expect("client builds")
}

#[tokio::test]
async fn typed_fetch_requests_the_alias_file_and_decodes_hour_one() {
    let server = serve(vec![(DA_PATH, vec![Canned::ok(DA)])]).await;
    let doc = client(&server.base, fast_retry(1))
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect("fetches");
    assert_eq!(doc.body.hour(1).and_then(|h| h.zonal_price), Some(28.47));
    assert_eq!(server.paths(), vec![DA_PATH.to_owned()]);
}

#[tokio::test]
async fn user_agent_is_configurable_and_defaults_to_the_crate() {
    let server = serve(vec![(DA_PATH, vec![Canned::ok(DA)])]).await;
    let custom = Client::builder()
        .base_url(format!("{}/", server.base))
        .user_agent("my-app/9 (ops@example.test)")
        .retry(fast_retry(1))
        .build()
        .expect("builds");
    custom
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect("ok");
    client(&server.base, fast_retry(1))
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect("ok");
    let seen = server.requests();
    assert_eq!(
        seen[0].header("user-agent"),
        Some("my-app/9 (ops@example.test)")
    );
    assert!(seen[1]
        .header("user-agent")
        .is_some_and(|ua| ua.starts_with("gridapi-ieso/")));
}

#[tokio::test]
async fn exact_version_requests_the_suffixed_file() {
    let path = "/public/DAHourlyOntarioZonalPrice/PUB_DAHourlyOntarioZonalPrice_20261117_v1.xml";
    let server = serve(vec![(path, vec![Canned::ok(DA)])]).await;
    let spec = FileSpec::of(
        Report::DaHourlyOntarioZonalPrice,
        Stamp::Day(d(2026, 11, 17)),
    )
    .version(Version::Exact(1));
    let got = client(&server.base, fast_retry(1))
        .fetch(&spec)
        .await
        .expect("ok");
    assert_eq!(got.revision, Some(1));
    assert_eq!(server.paths(), vec![path.to_owned()]);
}

#[tokio::test]
async fn highest_listed_resolves_the_latest_vn_from_the_directory_listing() {
    let listing = include_str!("fixtures/index_rt_ontario_zonal.html");
    let v7 = "/public/RealtimeOntarioZonalPrice/PUB_RealtimeOntarioZonalPrice_2026111802_v7.xml";
    let server = serve(vec![
        (
            "/public/RealtimeOntarioZonalPrice/",
            vec![Canned::ok(listing)],
        ),
        (
            v7,
            vec![Canned::ok(include_str!("fixtures/rt_ontario_zonal.xml"))],
        ),
    ])
    .await;
    let cl = Client::builder()
        .base_url(&server.base)
        .retry(fast_retry(1))
        .version(Version::HighestListed)
        .build()
        .expect("builds");
    let doc = cl
        .realtime_ontario_zonal_prices(d(2026, 11, 18), 2)
        .await
        .expect("ok");
    assert_eq!(doc.body.intervals[0].lmp, Some(42.95));
    assert_eq!(
        server.paths(),
        vec![
            "/public/RealtimeOntarioZonalPrice/".to_owned(),
            v7.to_owned()
        ]
    );
}

#[tokio::test]
async fn highest_listed_with_no_matching_revision_is_a_typed_error() {
    let listing = include_str!("fixtures/index_rt_ontario_zonal.html");
    let server = serve(vec![(
        "/public/RealtimeOntarioZonalPrice/",
        vec![Canned::ok(listing)],
    )])
    .await;
    let spec = FileSpec::of(
        Report::RealtimeOntarioZonalPrice,
        Stamp::Hour(d(1999, 1, 1), 1),
    )
    .version(Version::HighestListed);
    let err = client(&server.base, fast_retry(1))
        .fetch(&spec)
        .await
        .expect_err("none");
    assert!(matches!(err, Error::NoVersions { .. }), "{err}");
}

#[tokio::test]
async fn throttling_and_server_errors_are_retried_until_success() {
    let server = serve(vec![(
        DA_PATH,
        vec![
            Canned::status(503),
            Canned::status(429).header("Retry-After", "0"),
            Canned::ok(DA),
        ],
    )])
    .await;
    let doc = client(&server.base, fast_retry(4))
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect("third attempt succeeds");
    assert_eq!(doc.body.hours.len(), 24);
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn persistent_server_errors_stop_at_max_attempts_with_the_status() {
    let server = serve(vec![(DA_PATH, vec![Canned::status(500)])]).await;
    let err = client(&server.base, fast_retry(3))
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect_err("gives up");
    assert!(matches!(err, Error::Status { status: 500, .. }), "{err}");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn a_missing_file_is_not_found_and_is_not_retried() {
    let server = serve(vec![]).await;
    let err = client(&server.base, fast_retry(5))
        .da_ontario_zonal_prices(d(2030, 1, 1))
        .await
        .expect_err("404");
    assert!(
        matches!(&err, Error::NotFound { url } if url.ends_with("PUB_DAHourlyOntarioZonalPrice_20300101.xml")),
        "{err}"
    );
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn client_errors_other_than_404_are_not_retried() {
    let server = serve(vec![(DA_PATH, vec![Canned::status(403)])]).await;
    let err = client(&server.base, fast_retry(5))
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect_err("403");
    assert!(matches!(err, Error::Status { status: 403, .. }), "{err}");
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn a_byte_order_mark_in_the_body_is_stripped() {
    let server = serve(vec![(DA_PATH, vec![Canned::ok(format!("\u{feff}{DA}"))])]).await;
    let spec = FileSpec::of(
        Report::DaHourlyOntarioZonalPrice,
        Stamp::Day(d(2026, 11, 17)),
    );
    let got = client(&server.base, fast_retry(1))
        .fetch(&spec)
        .await
        .expect("ok");
    assert!(got.body.starts_with("<?xml"));
}

#[tokio::test]
async fn csv_reports_decode_through_the_typed_methods() {
    let lmp = include_str!("fixtures/da_lmp_20261117.csv");
    let server = serve(vec![(
        "/public/DAHourlyEnergyLMP/PUB_DAHourlyEnergyLMP_20261117.csv",
        vec![Canned::ok(lmp)],
    )])
    .await;
    let file = client(&server.base, fast_retry(1))
        .da_lmp(d(2026, 11, 17))
        .await
        .expect("ok");
    let zonal = file
        .location(gridapi_ieso::data::lmp::ONTARIO_ZONAL_PRICE)
        .next()
        .expect("zonal row");
    assert_eq!(zonal.lmp, Some(28.47));
}

#[tokio::test]
async fn directory_listings_parse_through_the_client() {
    let server = serve(vec![
        (
            "/public/",
            vec![Canned::ok(include_str!("fixtures/index_root.html"))],
        ),
        (
            "/public/Demand/",
            vec![Canned::ok(include_str!("fixtures/index_demand.html"))],
        ),
    ])
    .await;
    let cl = client(&server.base, fast_retry(1));
    let reports = cl.list_reports().await.expect("root");
    assert!(reports.iter().any(|r| r.name == "Adequacy3"));
    let files = cl.list_files("Demand").await.expect("demand");
    assert!(files.iter().any(|f| f.name == "PUB_Demand.csv"));
}

#[tokio::test]
async fn an_unreachable_server_is_a_transport_error() {
    let cl = Client::builder()
        .base_url("http://127.0.0.1:1/public")
        .connect_timeout(Duration::from_millis(200))
        .retry(fast_retry(2))
        .build()
        .expect("builds");
    let err = cl
        .da_ontario_zonal_prices(d(2026, 11, 17))
        .await
        .expect_err("refused");
    assert!(matches!(err, Error::Transport { .. }), "{err}");
}

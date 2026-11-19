//! Behavioural tests against a local mock of api.pjm.com, using synthetic response
//! bodies from `tests/fixtures`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use chrono::NaiveDate;
use gridapi_pjm::{
    ApiKey, Client, DaHourlyLmp, Error, KeyMode, LmpRequest, Pnode, PnodeRequest, Query, RateLimit,
    RetryPolicy, Row,
};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DA: &str = include_str!("fixtures/da_hrl_lmps.json");
const FEEDS: &str = include_str!("fixtures/feeds_list.json");
const METADATA: &str = include_str!("fixtures/metadata_da_hrl_lmps.json");
const DATETIME_MISSING: &str = include_str!("fixtures/error_datetime_missing.json");
const PNODE: &str = include_str!("fixtures/pnode.json");

const KEY_HEADER: &str = "Ocp-Apim-Subscription-Key";

fn json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/json")
        .set_body_string(body.to_owned())
}

fn fast_retry(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn user_client(server: &MockServer, key: &str) -> Client {
    Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .key_mode(KeyMode::User(ApiKey::new(key).unwrap()))
        .unthrottled()
        .retry(fast_retry(3))
        .build()
        .unwrap()
}

#[cfg(feature = "pjm-public-web-key")]
fn public_client(server: &MockServer) -> Client {
    Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .settings_url(format!("{}/config/settings.json", server.uri()))
        .key_mode(KeyMode::PublicWebApp)
        .i_accept_pjm_public_key_risk()
        .unthrottled()
        .retry(fast_retry(3))
        .build()
        .unwrap()
}

#[cfg(feature = "pjm-public-web-key")]
fn settings(key: &str) -> String {
    format!(r#"{{"baseUrl":"x","subscriptionKey":"{key}","tool":"Data Miner 2"}}"#)
}

fn nov17() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()
}

#[tokio::test]
async fn typed_lmp_query_sends_key_user_agent_and_documented_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/da_hrl_lmps"))
        .and(header(KEY_HEADER, "user-key"))
        .and(header("user-agent", "gridapi-test/9"))
        .and(query_param("rowCount", "50000"))
        .and(query_param("startRow", "1"))
        .and(query_param("pnode_id", "51288"))
        .and(query_param("row_is_current", "true"))
        .and(query_param(
            "datetime_beginning_ept",
            "11/17/2026 00:00to11/17/2026 23:59",
        ))
        .respond_with(json(DA))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .key_mode(KeyMode::User(ApiKey::new("user-key").unwrap()))
        .user_agent("gridapi-test/9")
        .unthrottled()
        .build()
        .unwrap();
    let rows: Vec<DaHourlyLmp> = client
        .da_hrl_lmps(LmpRequest::new(nov17()).pnode_id(51288))
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].pnode_name, "WESTERN HUB");
    assert!((rows[0].total_lmp_da - 116.425_657).abs() < 1e-9);
}

#[tokio::test]
async fn generic_query_returns_untyped_rows_with_envelope() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/da_hrl_lmps"))
        .and(query_param("fields", "pnode_id,total_lmp_da"))
        .and(query_param("sort", "datetime_beginning_ept"))
        .and(query_param("order", "Desc"))
        .respond_with(json(DA))
        .mount(&server)
        .await;
    let client = user_client(&server, "k");
    let q = Query::new()
        .fields(["pnode_id", "total_lmp_da"])
        .sort("datetime_beginning_ept", gridapi_pjm::Order::Desc);
    let page = client.query::<Row>("da_hrl_lmps", &q).await.unwrap();
    assert_eq!(page.total_rows, 24);
    assert_eq!(page.items[0]["pnode_id"], 51288);
    assert_eq!(page.items[0]["pnode_name"], "WESTERN HUB");
}

#[tokio::test]
async fn catalog_and_metadata() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/"))
        .respond_with(json(FEEDS))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/da_hrl_lmps/metadata"))
        .respond_with(json(METADATA))
        .expect(1)
        .mount(&server)
        .await;
    let client = user_client(&server, "k");
    let feeds = client.feeds().await.unwrap();
    assert!(feeds.iter().any(|f| f.name == "gen_by_fuel"));
    let md = client.feed_metadata("da_hrl_lmps").await.unwrap();
    assert!(md.filterable().any(|c| c.field_name == "pnode_id"));
}

#[cfg(feature = "pjm-public-web-key")]
#[tokio::test]
async fn public_key_is_fetched_once_and_reused() {
    let server = MockServer::start().await;
    Mock::given(path("/config/settings.json"))
        .respond_with(json(&settings("public-1")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/pnode"))
        .and(header(KEY_HEADER, "public-1"))
        .respond_with(json(PNODE))
        .expect(2)
        .mount(&server)
        .await;
    let client = public_client(&server);
    let a = client.pnode(51288).await.unwrap();
    let b = client.pnode(51288).await.unwrap();
    assert_eq!(a, b);
    assert_eq!(a.map(|p| p.pnode_name), Some("WESTERN HUB".to_owned()));
}

#[cfg(feature = "pjm-public-web-key")]
#[tokio::test]
async fn rotated_public_key_is_refetched_on_401_and_the_call_succeeds() {
    let server = MockServer::start().await;
    // First settings fetch serves the stale key, every later one the new key.
    Mock::given(path("/config/settings.json"))
        .respond_with(json(&settings("stale")))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/config/settings.json"))
        .respond_with(json(&settings("fresh")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/da_hrl_lmps"))
        .and(header(KEY_HEADER, "stale"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/da_hrl_lmps"))
        .and(header(KEY_HEADER, "fresh"))
        .respond_with(json(DA))
        .expect(1)
        .mount(&server)
        .await;
    let rows = public_client(&server)
        .da_hrl_lmps(LmpRequest::new(nov17()))
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
}

#[cfg(feature = "pjm-public-web-key")]
#[tokio::test]
async fn public_key_still_rejected_after_refetch_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(path("/config/settings.json"))
        .respond_with(json(&settings("nope")))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(401))
        .expect(2)
        .mount(&server)
        .await;
    let err = public_client(&server).pnode(1).await.unwrap_err();
    assert!(err.is_unauthorized(), "{err}");
}

#[cfg(feature = "pjm-public-web-key")]
#[tokio::test]
async fn user_key_401_is_returned_without_touching_settings() {
    let server = MockServer::start().await;
    Mock::given(path("/config/settings.json"))
        .respond_with(json(&settings("x")))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .settings_url(format!("{}/config/settings.json", server.uri()))
        .api_key(ApiKey::new("mine").unwrap())
        .unthrottled()
        .build()
        .unwrap();
    assert!(client.pnode(1).await.unwrap_err().is_unauthorized());
}

#[cfg(feature = "pjm-public-web-key")]
#[tokio::test]
async fn unreachable_settings_is_a_public_key_error() {
    let server = MockServer::start().await;
    Mock::given(path("/config/settings.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let err = public_client(&server).pnode(1).await.unwrap_err();
    assert!(matches!(err, Error::PublicKey(_)), "{err}");
}

#[tokio::test]
async fn rate_limited_then_ok_is_retried() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(json(PNODE))
        .expect(1)
        .mount(&server)
        .await;
    let rows = user_client(&server, "k")
        .pnodes(PnodeRequest::new().ids([51288]))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    let _: &Pnode = &rows[0];
}

#[tokio::test]
async fn persistent_429_surfaces_rate_limited_with_retry_after() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .expect(3) // first attempt + 2 retries
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .api_key(ApiKey::new("k").unwrap())
        .unthrottled()
        .retry(fast_retry(2))
        .build()
        .unwrap();
    let started = Instant::now();
    let err = client.pnode(1).await.unwrap_err();
    assert!(
        matches!(err, Error::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(1)),
        "{err}"
    );
    // Two honoured Retry-After sleeps of 1 s each.
    assert!(started.elapsed() >= Duration::from_secs(2));
}

#[tokio::test]
async fn server_errors_exhaust_retries_then_fail() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(503))
        .expect(4) // first attempt + 3 retries
        .mount(&server)
        .await;
    let err = user_client(&server, "k").pnode(1).await.unwrap_err();
    assert!(matches!(err, Error::Server { status: 503 }), "{err}");
}

#[tokio::test]
async fn zero_retries_means_one_attempt() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .api_key(ApiKey::new("k").unwrap())
        .unthrottled()
        .retry(fast_retry(0))
        .build()
        .unwrap();
    assert!(matches!(
        client.pnode(1).await.unwrap_err(),
        Error::Server { status: 500 }
    ));
}

#[tokio::test]
async fn validation_errors_are_not_retried_and_carry_pjm_messages() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/da_hrl_lmps"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("content-type", "application/json; charset=utf-8")
                .set_body_string(DATETIME_MISSING),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = user_client(&server, "k")
        .da_hrl_lmps(Query::new().pnode_ids([51288]))
        .await
        .unwrap_err();
    match err {
        Error::Rejected { status, errors, .. } => {
            assert_eq!(status, 400);
            assert!(errors[0].message.starts_with("A datetime is missing"));
            assert_eq!(errors[0].detail, vec!["datetime_filter".to_owned()]);
        }
        other => panic!("expected Rejected, got {other}"),
    }
}

#[tokio::test]
async fn invalid_queries_never_reach_the_network() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(json(DA))
        .expect(0)
        .mount(&server)
        .await;
    let client = user_client(&server, "k");
    let too_big = Query::new().row_count(50_001);
    assert!(matches!(
        client.query::<Row>("da_hrl_lmps", &too_big).await,
        Err(Error::InvalidQuery(_))
    ));
    assert!(matches!(
        client.query::<Row>("../etc/passwd", &Query::new()).await,
        Err(Error::InvalidQuery(_))
    ));
    assert!(matches!(
        client.feed_metadata("a b").await,
        Err(Error::InvalidQuery(_))
    ));
}

#[tokio::test]
async fn non_json_success_body_is_a_decode_error() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;
    let err = user_client(&server, "k").pnode(1).await.unwrap_err();
    assert!(
        matches!(&err, Error::Decode { snippet, .. } if snippet.contains("maintenance")),
        "{err}"
    );
}

fn page_of(start: u32, rows: u32, total: u32) -> String {
    let items = (start..start + rows)
        .map(|i| format!(r#"{{"n":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"links":[],"items":[{items}],"totalRows":{total}}}"#)
}

#[tokio::test]
async fn query_all_follows_start_row_until_total_rows() {
    let server = MockServer::start().await;
    for (start, rows) in [(1u32, 2u32), (3, 2), (5, 1)] {
        Mock::given(path("/api/v1/gen_by_fuel"))
            .and(query_param("startRow", start.to_string()))
            .and(query_param("rowCount", "2"))
            .respond_with(json(&page_of(start, rows, 5)))
            .expect(1)
            .mount(&server)
            .await;
    }
    let rows: Vec<Row> = user_client(&server, "k")
        .query_all("gen_by_fuel", &Query::new().row_count(2))
        .await
        .unwrap();
    let ns: Vec<i64> = rows.iter().map(|r| r["n"].as_i64().unwrap()).collect();
    assert_eq!(ns, vec![1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn query_all_stops_after_an_exactly_full_last_page() {
    let server = MockServer::start().await;
    for start in [1u32, 3] {
        Mock::given(path("/api/v1/gen_by_fuel"))
            .and(query_param("startRow", start.to_string()))
            .respond_with(json(&page_of(start, 2, 4)))
            .expect(1)
            .mount(&server)
            .await;
    }
    let rows: Vec<Row> = user_client(&server, "k")
        .query_all("gen_by_fuel", &Query::new().row_count(2))
        .await
        .unwrap();
    assert_eq!(rows.len(), 4);
}

#[tokio::test]
async fn query_all_on_an_empty_result_makes_one_request() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/gen_by_fuel"))
        .respond_with(json(r#"{"links":[],"items":[],"totalRows":0}"#))
        .expect(1)
        .mount(&server)
        .await;
    let rows: Vec<Row> = user_client(&server, "k")
        .query_all("gen_by_fuel", &Query::new())
        .await
        .unwrap();
    assert_eq!(rows.len(), 0);
}

#[tokio::test]
async fn client_throttle_spaces_requests_within_one_instance() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(json(PNODE))
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .api_key(ApiKey::new("k").unwrap())
        .rate_limit(RateLimit::new(2, Duration::from_millis(600)).unwrap())
        .build()
        .unwrap();
    let started = Instant::now();
    for _ in 0..3 {
        client.pnode(51288).await.unwrap();
    }
    // Two fit the window; the third waits for the first to age out.
    assert!(
        started.elapsed() >= Duration::from_millis(550),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn separate_clients_do_not_throttle_each_other() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(json(PNODE))
        .mount(&server)
        .await;
    let limit = RateLimit::new(1, Duration::from_secs(30)).unwrap();
    let make = || {
        Client::builder()
            .base_url(format!("{}/api/v1", server.uri()))
            .api_key(ApiKey::new("k").unwrap())
            .rate_limit(limit)
            .build()
            .unwrap()
    };
    let started = Instant::now();
    make().pnode(1).await.unwrap();
    make().pnode(1).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn clones_share_one_rate_limit_window() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v1/pnode"))
        .respond_with(json(PNODE))
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(format!("{}/api/v1", server.uri()))
        .api_key(ApiKey::new("k").unwrap())
        .rate_limit(RateLimit::new(1, Duration::from_millis(400)).unwrap())
        .build()
        .unwrap();
    let started = Instant::now();
    client.pnode(1).await.unwrap();
    client.clone().pnode(1).await.unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(350),
        "{:?}",
        started.elapsed()
    );
}

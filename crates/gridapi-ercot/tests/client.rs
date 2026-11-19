//! Client behaviour against a mock server, using synthetic fixtures (real shapes, invented values) as bodies.

use std::time::Duration;

use gridapi_ercot::{
    ArchiveQuery, Client, Credentials, DayAheadSpp, Error, FuelMix, Query, RealTimeLmp,
    RealTimeSpp, ReportRequest, RetryPolicy, SortDir,
};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, header_regex, method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const API: &str = "/api/public-reports";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn json_body(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "application/json")
        .set_body_bytes(fixture(name))
}

fn fast_retry(attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts: attempts,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn client(server: &MockServer, retry: RetryPolicy) -> Client {
    Client::builder(Credentials::new(
        "user@example.com",
        "pw&secret=1",
        "sub-key-123",
    ))
    .base_url(format!("{}{API}", server.uri()))
    .token_url(format!("{}/token", server.uri()))
    .fuel_mix_url(format!("{}/fuel-mix.json", server.uri()))
    .retry(retry)
    .build()
    .expect("client builds")
}

fn token_response(id: &str, refresh: &str, expires_in: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id_token": id, "access_token": "unused", "token_type": "Bearer",
        "expires_in": expires_in, "refresh_token": refresh,
    }))
}

async fn mount_token(server: &MockServer, id: &str) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_response(id, "RT-1", "3600"))
        .mount(server)
        .await;
}

const DAM_PATH: &str = "/api/public-reports/np4-190-cd/dam_stlmnt_pnt_prices";

#[tokio::test]
async fn sends_subscription_key_bearer_and_password_grant_form() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=password"))
        .and(body_string_contains("username=user%40example.com"))
        .and(body_string_contains("password=pw%26secret%3D1"))
        .and(body_string_contains("response_type=id_token"))
        .and(body_string_contains(
            "client_id=fec253ea-0d06-4272-a5e6-b478baeecd70",
        ))
        .and(body_string_contains(
            "scope=openid+fec253ea-0d06-4272-a5e6-b478baeecd70+offline_access",
        ))
        .respond_with(token_response("ID-1", "RT-1", "3600"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header("Ocp-Apim-Subscription-Key", "sub-key-123"))
        .and(header("Authorization", "Bearer ID-1"))
        .and(query_param("settlementPoint", "HB_HOUSTON"))
        .respond_with(json_body("dam_spp.json"))
        .expect(1)
        .mount(&server)
        .await;

    let page = client(&server, fast_retry(1))
        .fetch(&Query::<DayAheadSpp>::new().settlement_point("HB_HOUSTON"))
        .await
        .expect("typed fetch");
    assert_eq!(page.records.len(), 3);
    assert_eq!(page.meta.total_records, 24);
    assert!((page.records[2].price - 46.96).abs() < 1e-9);
}

#[tokio::test]
async fn token_is_cached_across_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_response("ID-1", "RT-1", "3600"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(json_body("dam_spp.json"))
        .expect(3)
        .mount(&server)
        .await;
    let c = client(&server, fast_retry(1));
    for _ in 0..3 {
        c.fetch(&Query::<DayAheadSpp>::new()).await.expect("fetch");
    }
}

#[tokio::test]
async fn expiring_token_is_renewed_with_the_refresh_token() {
    let server = MockServer::start().await;
    // First login hands out a token that is already inside the expiry skew window.
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=password"))
        .respond_with(token_response("ID-OLD", "RT-1", "1"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=RT-1"))
        .respond_with(token_response("ID-NEW", "RT-2", "3600"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header("Authorization", "Bearer ID-OLD"))
        .respond_with(json_body("dam_spp.json"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header("Authorization", "Bearer ID-NEW"))
        .respond_with(json_body("dam_spp.json"))
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, fast_retry(1));
    c.fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("first call logs in");
    c.fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("second call refreshes");
}

#[tokio::test]
async fn http_401_triggers_exactly_one_relogin() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_response("ID-1", "RT-1", "3600"))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(token_response("ID-2", "RT-2", "3600"))
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header("Authorization", "Bearer ID-1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"message": "token expired"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header("Authorization", "Bearer ID-2"))
        .respond_with(json_body("dam_spp.json"))
        .expect(1)
        .mount(&server)
        .await;
    client(&server, fast_retry(1))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("second login succeeds");
}

#[tokio::test]
async fn persistent_401_surfaces_after_one_relogin() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"message": "invalid subscription key"})),
        )
        .expect(2)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(1))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect_err("still unauthorized");
    assert!(
        matches!(&err, Error::Api { status: 401, message, .. } if message.contains("subscription")),
        "{err}"
    );
}

#[tokio::test]
async fn bad_credentials_are_an_auth_error_with_the_b2c_description() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "AADB2C90225: The username or password provided in the request are invalid."
        })))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(3))
        .products()
        .await
        .expect_err("login refused");
    assert!(
        matches!(&err, Error::Auth { status: 400, message } if message.starts_with("AADB2C90225")),
        "{err}"
    );
    assert!(!err.is_retryable(), "a refused grant must not be retried");
}

#[tokio::test]
async fn retries_429_honouring_retry_after_then_succeeds() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("slow down"),
        )
        .up_to_n_times(2)
        .with_priority(1)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(json_body("dam_spp.json"))
        .with_priority(2)
        .expect(1)
        .mount(&server)
        .await;
    let page = client(&server, fast_retry(5))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("succeeds on third attempt");
    assert_eq!(page.records.len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_attempts_on_5xx() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .expect(3)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(3))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect_err("exhausted");
    assert!(matches!(err, Error::Api { status: 503, .. }), "{err}");
}

#[tokio::test]
async fn unknown_filter_is_a_400_with_the_rejected_names_and_no_retry() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("content-type", "application/json")
                .set_body_bytes(fixture("error_unknown_params.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(5))
        .query(&ReportRequest::new("NP4-190-CD", "dam_stlmnt_pnt_prices").filter("bogusParam", 1))
        .await
        .expect_err("rejected");
    match err {
        Error::Api {
            status: 400,
            unknown_params,
            ..
        } => {
            assert_eq!(
                unknown_params,
                ["hourEndingFrom", "hourEndingTo", "bogusParam"]
            );
        }
        other => panic!("expected 400 Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn html_bot_check_with_200_is_retried_until_json_arrives() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<HTML><HEAD><TITLE>Loading</TITLE></HEAD></HTML>"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(json_body("dam_spp.json"))
        .with_priority(2)
        .mount(&server)
        .await;
    client(&server, fast_retry(3))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("recovers");
}

#[tokio::test]
async fn persistent_bot_check_is_a_bot_check_error() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string("<HTML>Loading</HTML>"),
        )
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(2))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect_err("never gets data");
    assert!(
        matches!(&err, Error::BotCheck { snippet } if snippet.contains("Loading")),
        "{err}"
    );
}

#[tokio::test]
async fn follows_the_bot_check_redirect_keeping_cookies() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    // With the challenge cookie the data is served; without it, bounce through a 302
    // that sets the cookie (what the CDN does).
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header_regex("cookie", "incap_ses=passed"))
        .respond_with(json_body("dam_spp.json"))
        .with_priority(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{DAM_PATH}?retry=1").as_str())
                .insert_header("set-cookie", "incap_ses=passed; Path=/")
                .insert_header("content-type", "text/html")
                .set_body_string("<HTML>Loading</HTML>"),
        )
        .with_priority(5)
        .mount(&server)
        .await;
    let page = client(&server, fast_retry(1))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("redirect followed with cookie");
    assert_eq!(page.records.len(), 3);
}

#[tokio::test]
async fn sends_a_browser_like_user_agent_by_default() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(header_regex("user-agent", "^Mozilla/5.0 .*gridapi-ercot"))
        .respond_with(json_body("dam_spp.json"))
        .expect(1)
        .mount(&server)
        .await;
    client(&server, fast_retry(1))
        .fetch(&Query::<DayAheadSpp>::new())
        .await
        .expect("UA matched");
}

/// Serves `pages` pages of two rows each, keyed by the `page` query parameter.
struct Paged {
    total_pages: u32,
}

impl Respond for Paged {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let page: u32 = request
            .url
            .query_pairs()
            .find(|(k, _)| k == "page")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(1);
        let rows: Vec<_> = (0..2)
            .map(|i| {
                json!([
                    "2026-11-17",
                    format!("{:02}:00", page * 2 + i - 1),
                    "HB_NORTH",
                    f64::from(page * 10 + i),
                    false
                ])
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({
            "_meta": {"totalRecords": self.total_pages * 2, "pageSize": 2,
                      "totalPages": self.total_pages, "currentPage": page},
            "report": {"reportName": "dam_stlmnt_pnt_prices", "reportDisplayName": "DAM",
                       "reportEMIL": "NP4-190-CD", "downloadLimit": 100},
            "fields": [
                {"name":"deliveryDate","label":"","cardinality":1,"dataType":"DATE","searchable":true,"sortable":true,"hasRange":true},
                {"name":"hourEnding","label":"","cardinality":2,"dataType":"STRING","searchable":true,"sortable":true,"hasRange":false},
                {"name":"settlementPoint","label":"","cardinality":3,"dataType":"STRING","searchable":true,"sortable":true,"hasRange":false},
                {"name":"settlementPointPrice","label":"","cardinality":4,"dataType":"DOUBLE","searchable":true,"sortable":true,"hasRange":true},
                {"name":"DSTFlag","label":"","cardinality":5,"dataType":"BOOLEAN","searchable":true,"sortable":true,"hasRange":false}],
            "data": rows,
        }))
    }
}

#[tokio::test]
async fn fetch_all_follows_every_page_in_order() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(query_param("size", "50000"))
        .respond_with(Paged { total_pages: 3 })
        .expect(3)
        .mount(&server)
        .await;
    let all = client(&server, fast_retry(1))
        .fetch_all(&Query::<DayAheadSpp>::new(), 10)
        .await
        .expect("all pages");
    let prices: Vec<f64> = all.iter().map(|r| r.price).collect();
    assert_eq!(prices, [10.0, 11.0, 20.0, 21.0, 30.0, 31.0]);
}

#[tokio::test]
async fn fetch_all_refuses_to_exceed_the_page_budget_without_fetching_the_rest() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .respond_with(Paged { total_pages: 40 })
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(1))
        .fetch_all(&Query::<DayAheadSpp>::new(), 5)
        .await
        .expect_err("over budget");
    assert!(
        matches!(
            err,
            Error::TooManyPages {
                limit: 5,
                total_pages: 40
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn query_all_respects_an_explicit_page_size_and_merges_meta() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(DAM_PATH))
        .and(query_param("size", "2"))
        .and(query_param("sort", "settlementPointPrice"))
        .and(query_param("dir", "desc"))
        .respond_with(Paged { total_pages: 2 })
        .expect(2)
        .mount(&server)
        .await;
    let merged = client(&server, fast_retry(1))
        .query_all(
            &ReportRequest::new("NP4-190-CD", "dam_stlmnt_pnt_prices")
                .size(2)
                .sort("settlementPointPrice", SortDir::Desc),
            4,
        )
        .await
        .expect("merged");
    assert_eq!(merged.data.len(), 4);
    assert_eq!(merged.meta.total_pages, 1);
}

#[tokio::test]
async fn typed_real_time_filters_reach_the_wire_with_api_names() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    let day = chrono_date(2026, 11, 17);
    Mock::given(method("GET"))
        .and(path("/api/public-reports/np6-905-cd/spp_node_zone_hub"))
        .and(query_param("deliveryDateFrom", "2026-11-17"))
        .and(query_param("deliveryDateTo", "2026-11-17"))
        .and(query_param("settlementPoint", "HB_NORTH"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("spp_hb_north.json")))
        .expect(1)
        .mount(&server)
        .await;
    let page = client(&server, fast_retry(1))
        .fetch(
            &Query::<RealTimeSpp>::new()
                .delivery_date(day, day)
                .settlement_point("HB_NORTH"),
        )
        .await
        .expect("typed");
    assert_eq!(page.records.len(), 3);
    assert!((page.records[0].price - 30.43).abs() < 1e-9);
    assert_eq!(
        page.records[0]
            .interval_ending()
            .expect("valid")
            .to_string(),
        "2026-11-17 00:15:00"
    );
}

fn chrono_date(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

#[tokio::test]
async fn lmp_query_uses_sced_timestamp_range() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    let t = |h, m| {
        chrono_date(2026, 11, 17)
            .and_hms_opt(h, m, 0)
            .expect("time")
    };
    Mock::given(method("GET"))
        .and(path("/api/public-reports/np6-788-cd/lmp_node_zone_hub"))
        .and(query_param("SCEDTimestampFrom", "2026-11-17T23:00:00"))
        .and(query_param("SCEDTimestampTo", "2026-11-17T23:10:00"))
        .respond_with(json_body("lmp_node.json"))
        .expect(1)
        .mount(&server)
        .await;
    let page = client(&server, fast_retry(1))
        .fetch(&Query::<RealTimeLmp>::new().sced_timestamp(t(23, 0), t(23, 10)))
        .await
        .expect("typed");
    assert_eq!(page.records.len(), 2);
}

#[tokio::test]
async fn catalog_lists_products_and_search_filters_them() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path(API))
        .respond_with(json_body("catalog_sample.json"))
        .expect(2)
        .mount(&server)
        .await;
    let c = client(&server, fast_retry(1));
    let all = c.products().await.expect("catalog");
    assert!(all.iter().any(|p| p.emil_id == "NP4-190-CD"));
    let hits = c.search_products("day-ahead").await.expect("search");
    assert!(!hits.is_empty() && hits.len() < all.len());
    assert!(hits.iter().all(|p| p.matches("day-ahead")));
}

#[tokio::test]
async fn product_lookup_lowercases_the_id_and_exposes_artifacts() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path("/api/public-reports/np6-905-cd"))
        .respond_with(json_body("product_np6_905.json"))
        .expect(1)
        .mount(&server)
        .await;
    let p = client(&server, fast_retry(1))
        .product("NP6-905-CD")
        .await
        .expect("product");
    assert_eq!(p.artifacts[0].name(), "spp_node_zone_hub");
}

#[tokio::test]
async fn unknown_product_is_a_404_api_error() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path("/api/public-reports/np9-999-cd"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(
                json!({"code": 404, "status": "NOT_FOUND", "message": "np9-999-cd"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server, fast_retry(3))
        .product("np9-999-cd")
        .await
        .expect_err("404");
    assert!(
        matches!(&err, Error::Api { status: 404, message, .. } if message == "np9-999-cd"),
        "{err}"
    );
}

#[tokio::test]
async fn archive_listing_download_and_extract() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("GET"))
        .and(path("/api/public-reports/archive/np6-905-cd"))
        .and(query_param("postDatetimeFrom", "2026-11-17T23:00:00"))
        .respond_with(json_body("archive_listing.json"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/public-reports/archive/np6-905-cd"))
        .and(query_param("download", "1300006200"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/zip")
                .insert_header(
                    "content-disposition",
                    "attachment; filename=1300006200.cdr.zip",
                )
                .set_body_bytes(fixture("posting.zip")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, fast_retry(1));
    let from = chrono_date(2026, 11, 17).and_hms_opt(23, 0, 0).expect("t");
    let to = chrono_date(2026, 11, 17)
        .and_hms_opt(23, 59, 59)
        .expect("t");
    let listing = c
        .archives(
            "NP6-905-CD",
            &ArchiveQuery::new().posted_between(from, to).size(2),
        )
        .await
        .expect("listing");
    assert_eq!(listing.entries.len(), 2);
    let file = c
        .download_archive("NP6-905-CD", listing.entries[0].doc_id)
        .await
        .expect("download");
    assert_eq!(file.file_name.as_deref(), Some("1300006200.cdr.zip"));
    let files = file.extract().expect("unzip");
    assert_eq!(files.len(), 1);
    let csv = files[0].text().expect("utf8");
    assert!(csv.lines().count() > 100, "a full day-slice of prices");
}

#[tokio::test]
async fn bulk_download_posts_doc_ids_and_unpacks_nested_zips() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    Mock::given(method("POST"))
        .and(path("/api/public-reports/archive/np6-905-cd/download"))
        .and(wiremock::matchers::body_json(
            json!({"docIds": [1300006200u64, 1300002912u64]}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/zip")
                .set_body_bytes(fixture("bulk_nested.zip")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = client(&server, fast_retry(1));
    let files = c
        .download_archives("np6-905-cd", &[1_300_006_200, 1_300_002_912])
        .await
        .expect("bulk")
        .extract()
        .expect("unzip");
    assert_eq!(files.len(), 2);
    let err = c
        .download_archives("np6-905-cd", &[])
        .await
        .expect_err("empty");
    assert!(matches!(err, Error::Config(_)));
}

#[tokio::test]
async fn archives_all_pages_through_listings() {
    let server = MockServer::start().await;
    mount_token(&server, "ID-1").await;
    let listing = |page: u32| {
        json!({
            "_meta": {"totalRecords": 3, "pageSize": 2, "totalPages": 2, "currentPage": page},
            "archives": if page == 1 {
                json!([{"docId": 3, "friendlyName": "c", "postDatetime": "2026-11-17T03:00:00.000"},
                       {"docId": 2, "friendlyName": "b", "postDatetime": "2026-11-17T02:00:00.000"}])
            } else {
                json!([{"docId": 1, "friendlyName": "a", "postDatetime": "2026-11-17T01:00:00.000"}])
            },
        })
    };
    for page in [1u32, 2] {
        Mock::given(method("GET"))
            .and(path("/api/public-reports/archive/np6-905-cd"))
            .and(query_param("page", page.to_string().as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(listing(page)))
            .expect(1)
            .mount(&server)
            .await;
    }
    let entries = client(&server, fast_retry(1))
        .archives_all("np6-905-cd", &ArchiveQuery::new().size(2), 5)
        .await
        .expect("all");
    let ids: Vec<u64> = entries.iter().map(|e| e.doc_id).collect();
    assert_eq!(ids, [3, 2, 1]);
}

#[tokio::test]
async fn fuel_mix_needs_no_token() {
    let server = MockServer::start().await;
    // No token mock: any call to /token would 404 and fail the test.
    Mock::given(method("GET"))
        .and(path("/fuel-mix.json"))
        .respond_with(json_body("fuel_mix.json"))
        .expect(1)
        .mount(&server)
        .await;
    let mix: FuelMix = client(&server, fast_retry(1))
        .fuel_mix()
        .await
        .expect("fuel mix");
    assert_eq!(mix.snapshots.len(), 2);
    assert!(mix.latest().expect("latest").generation_mw["Natural Gas"] > 0.0);
}

#[tokio::test]
async fn transport_failure_is_retryable_and_reports_the_cause() {
    // Nothing listens on this port.
    let c = Client::builder(Credentials::new("u", "p", "k"))
        .base_url("http://127.0.0.1:9/api/public-reports")
        .token_url("http://127.0.0.1:9/token")
        .retry(fast_retry(2))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("builds");
    let err = c.products().await.expect_err("connection refused");
    assert!(matches!(err, Error::Transport(_)), "{err}");
    assert!(err.is_retryable());
}

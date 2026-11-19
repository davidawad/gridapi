//! Offline tests of the client against a scripted local server: transport,
//! retry, error mapping, paging and route browsing.

#![allow(clippy::expect_used, clippy::panic, clippy::assert_is_empty)]

mod common;

use std::time::Duration;

use futures_util::TryStreamExt;
use gridapi_eia::datasets::rto::{self, RegionDataRow, RegionMetric};
use gridapi_eia::{Client, DataQuery, Error, Frequency, RetryPolicy, Row};

use common::{MockServer, Reply};

const KEY: &str = "TEST-KEY-abc123";

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn client(server: &MockServer) -> Client {
    Client::builder()
        .api_key(KEY)
        .base_url(format!("{}/v2", server.base))
        .bulk_base_url(server.base.clone())
        .user_agent("gridapi-eia-tests/0")
        .retry(fast_retry())
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client")
}

fn page_json(total: u64, rows: std::ops::Range<u64>) -> String {
    let data: Vec<String> = rows
        .map(|i| {
            format!(
                r#"{{"period":"2026-01-01T{:02}","respondent":"ERCO","type":"D","value":"{i}"}}"#,
                i % 24
            )
        })
        .collect();
    format!(
        r#"{{"response":{{"total":"{total}","data":[{}]}}}}"#,
        data.join(",")
    )
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")).map(str::to_owned))
}

#[tokio::test]
async fn sends_key_and_bracket_params_percent_encoded() {
    let fixture = include_str!("fixtures/data_region.json");
    let server = MockServer::script(vec![Reply::json(200, fixture)]);
    let q = rto::region_data_for("ERCO", RegionMetric::Demand).length(3);
    let page = client(&server)
        .data_page::<RegionDataRow>(&q)
        .await
        .expect("page");
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.total, 138_107);
    assert_eq!(page.date_format.as_deref(), Some("YYYY-MM-DD\"T\"HH24"));
    assert!(page
        .warnings
        .iter()
        .any(|w| w.warning == "incomplete return"));

    let url = &server.seen()[0];
    assert!(
        url.starts_with("/v2/electricity/rto/region-data/data?"),
        "{url}"
    );
    assert_eq!(query_param(url, "api_key").as_deref(), Some(KEY));
    assert_eq!(query_param(url, "frequency").as_deref(), Some("hourly"));
    assert_eq!(
        query_param(url, "facets%5Brespondent%5D%5B%5D").as_deref(),
        Some("ERCO")
    );
    assert_eq!(query_param(url, "data%5B0%5D").as_deref(), Some("value"));
    assert_eq!(query_param(url, "length").as_deref(), Some("3"));
}

#[tokio::test]
async fn retries_429_then_503_then_succeeds() {
    let ok = include_str!("fixtures/data_region.json");
    let server = MockServer::script(vec![
        Reply::json(429, r#"{"error":"slow down"}"#).with_header("Retry-After", "0"),
        Reply::json(503, "<html>unavailable</html>"),
        Reply::json(200, ok),
    ]);
    let q = rto::region_data();
    let page = client(&server)
        .data_page::<Row>(&q)
        .await
        .expect("eventually ok");
    assert_eq!(page.rows.len(), 3);
    assert_eq!(server.seen().len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_retries_with_the_last_status() {
    let server = MockServer::script(vec![Reply::json(503, "down")]);
    let err = client(&server)
        .data_page::<Row>(&rto::region_data())
        .await
        .expect_err("must fail");
    assert!(matches!(err, Error::Api { status: 503, .. }), "{err:?}");
    assert_eq!(server.seen().len(), 4, "1 attempt + 3 retries");
}

#[tokio::test]
async fn client_errors_are_not_retried_and_keep_eia_code() {
    let body = include_str!("fixtures/error_invalid_key.json");
    let server = MockServer::script(vec![Reply::json(403, body)]);
    let err = client(&server)
        .data_page::<Row>(&rto::region_data())
        .await
        .expect_err("must fail");
    match err {
        Error::Api { status, code, .. } => {
            assert_eq!(status, 403);
            assert_eq!(code.as_deref(), Some("API_KEY_INVALID"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(server.seen().len(), 1);
}

#[tokio::test]
async fn bad_request_message_is_surfaced() {
    let body = include_str!("fixtures/error_bad_frequency.json");
    let server = MockServer::script(vec![Reply::json(400, body)]);
    let err = client(&server)
        .data_page::<Row>(
            &DataQuery::new("electricity/rto/region-data").frequency(Frequency::Daily),
        )
        .await
        .expect_err("must fail");
    assert!(err.to_string().contains("Invalid frequency"), "{err}");
}

#[tokio::test]
async fn invalid_length_is_rejected_before_any_request() {
    let server = MockServer::script(vec![Reply::json(200, "{}")]);
    let err = client(&server)
        .data_page::<Row>(&rto::region_data().length(9000))
        .await
        .expect_err("must fail");
    assert!(matches!(err, Error::InvalidRequest(_)));
    assert!(server.seen().is_empty());
}

#[tokio::test]
async fn error_text_never_contains_the_api_key() {
    // Nothing listens on this port: the transport error must not echo the URL.
    let c = Client::builder()
        .api_key(KEY)
        .base_url("http://127.0.0.1:9/v2")
        .retry(RetryPolicy::none())
        .timeout(Duration::from_secs(2))
        .build()
        .expect("client");
    let err = c
        .data_page::<Row>(&rto::region_data())
        .await
        .expect_err("must fail");
    assert!(matches!(err, Error::Http(_)), "{err:?}");
    assert!(!err.to_string().contains(KEY), "{err}");
    assert!(!format!("{err:?}").contains(KEY));
}

#[tokio::test]
async fn data_all_pages_by_offset_until_total() {
    const TOTAL: u64 = 12;
    let server = MockServer::start(|_, url| {
        let offset: u64 = query_param(url, "offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let length: u64 = query_param(url, "length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        let end = (offset + length).min(TOTAL);
        Reply::json(200, page_json(TOTAL, offset..end))
    });
    let q = rto::region_data().length(5);
    let rows: Vec<Row> = client(&server).data_all(&q).await.expect("all rows");
    let values: Vec<String> = rows
        .iter()
        .map(|r| r["value"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(values, (0..12).map(|i| i.to_string()).collect::<Vec<_>>());

    let seen = server.seen();
    let offsets: Vec<_> = seen
        .iter()
        .map(|u| query_param(u, "offset").unwrap_or_default())
        .collect();
    assert_eq!(offsets, ["0", "5", "10"]);
    assert!(
        seen.iter()
            .all(|u| query_param(u, "sort%5B0%5D%5Bcolumn%5D").as_deref() == Some("period")),
        "default sort keeps offsets stable: {seen:?}"
    );
}

#[tokio::test]
async fn data_all_honours_start_offset_and_user_sort() {
    let server = MockServer::start(|_, url| {
        let offset: u64 = query_param(url, "offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Reply::json(200, page_json(8, offset.min(8)..(offset + 3).min(8)))
    });
    let q = rto::region_data()
        .length(3)
        .offset(6)
        .sort("value", gridapi_eia::Direction::Desc);
    let rows: Vec<Row> = client(&server).data_all(&q).await.expect("rows");
    assert_eq!(rows.len(), 2, "rows 6 and 7 only");
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        query_param(&seen[0], "sort%5B0%5D%5Bcolumn%5D").as_deref(),
        Some("value")
    );
    assert_eq!(
        query_param(&seen[0], "sort%5B0%5D%5Bdirection%5D").as_deref(),
        Some("desc")
    );
}

#[tokio::test]
async fn paging_stops_on_an_empty_page_even_if_total_overstates() {
    let server = MockServer::start(|i, _| {
        if i == 0 {
            Reply::json(200, page_json(100, 0..3))
        } else {
            Reply::json(200, page_json(100, 0..0))
        }
    });
    let rows: Vec<Row> = client(&server)
        .data_all(&rto::region_data().length(3))
        .await
        .expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(server.seen().len(), 2);
}

#[tokio::test]
async fn stream_is_lazy_and_can_be_dropped_early() {
    let server = MockServer::start(|_, url| {
        let offset: u64 = query_param(url, "offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Reply::json(200, page_json(1000, offset..offset + 10))
    });
    let c = client(&server);
    let q = rto::region_data().length(10);
    let first = c
        .data_stream::<Row>(&q)
        .try_next()
        .await
        .expect("ok")
        .expect("one page");
    assert_eq!(first.len(), 10);
    assert_eq!(
        server.seen().len(),
        1,
        "no request beyond the page consumed"
    );
}

#[tokio::test]
async fn empty_result_is_an_empty_vec() {
    let server = MockServer::script(vec![Reply::json(
        200,
        include_str!("fixtures/data_empty.json"),
    )]);
    let rows: Vec<RegionDataRow> = client(&server)
        .data_all(&rto::region_data_for("NOSUCHBA", RegionMetric::Demand))
        .await
        .expect("ok");
    assert!(rows.is_empty());
    assert_eq!(server.seen().len(), 1);
}

#[tokio::test]
async fn route_and_facet_browsing() {
    let server = MockServer::start(|_, url| {
        let path = url.split('?').next().unwrap_or_default();
        match path {
            "/v2/electricity/rto/region-data" => {
                Reply::json(200, include_str!("fixtures/meta_region_data.json"))
            }
            "/v2/electricity/rto" => Reply::json(200, include_str!("fixtures/meta_rto.json")),
            "/v2/electricity/rto/region-data/facet/type" => {
                Reply::json(200, include_str!("fixtures/facet_type.json"))
            }
            "/v2" | "/v2/" => Reply::json(200, include_str!("fixtures/meta_root.json")),
            _ => Reply::json(404, r#"{"error":"Not found.","code":404}"#),
        }
    });
    let c = client(&server);

    let root = c.root().await.expect("root");
    assert!(root.routes.iter().any(|r| r.id == "electricity"));

    let meta = c.route("electricity/rto/region-data").await.expect("meta");
    assert!(meta.is_dataset());
    assert_eq!(meta.start_period.as_deref(), Some("2019-01-01T00"));

    let types = c
        .facet_values("electricity/rto/region-data", "type")
        .await
        .expect("facets");
    let ids: Vec<_> = types.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["D", "DF", "NG", "TI"]);

    let err = c.route("nope").await.expect_err("404");
    assert!(matches!(err, Error::Api { status: 404, .. }), "{err:?}");
}

#[tokio::test]
async fn datasets_under_walks_the_tree() {
    let leaf = include_str!("fixtures/meta_region_data.json");
    let server = MockServer::start(move |_, url| {
        let path = url
            .split('?')
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        match path {
            "/v2/x" => Reply::json(
                200,
                r#"{"response":{"id":"x","name":"X","routes":[{"id":"a"},{"id":"b"}]}}"#,
            ),
            "/v2/x/a" | "/v2/x/b" => Reply::json(200, leaf),
            _ => Reply::json(404, r#"{"error":"Not found.","code":404}"#),
        }
    });
    let mut found = client(&server).datasets_under("x").await.expect("walk");
    found.sort_by(|l, r| l.0.cmp(&r.0));
    let paths: Vec<_> = found.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["x/a", "x/b"]);
    assert!(found.iter().all(|(_, m)| m.is_dataset()));
}

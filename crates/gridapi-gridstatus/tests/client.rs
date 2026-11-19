//! Client behaviour against synthetic fixtures (real shapes, invented values) and a scripted server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use common::{fixture, serve, Mock, Reply};
use gridapi_gridstatus::{
    Client, Error, Iso, Order, Query, Resample, ResampleFrequency, RetryPolicy,
};

const KEY: &str = "test-key-0123456789abcdef";

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(2),
        max_delay: Duration::from_millis(20),
    }
}

fn client(mock: &Mock) -> Client {
    Client::builder()
        .api_key(KEY)
        .base_url(mock.base_url.clone())
        .min_interval(Duration::ZERO)
        .retry(fast_retry())
        .build()
        .unwrap()
}

/// Regression (): the live catalog failed to decode with `invalid value: integer
/// -1, expected u64` (2026-11-18). The fixture is a real dataset record with the row
/// estimate and rank set to -1, the values the live catalog carried.
#[tokio::test]
async fn catalog_accepts_a_negative_row_estimate_and_rank() {
    let mock = serve(vec![Reply::ok(fixture(
        "datasets_list_unknown_row_estimate.json",
    ))])
    .await;
    let list = client(&mock).list_datasets().await.unwrap();
    let ds = list.get("ercot_spp_day_ahead_hourly").unwrap();
    assert_eq!(ds.number_of_rows_approximate, Some(-1));
    assert_eq!(ds.popularity_rank, Some(-1));
}

#[tokio::test]
async fn catalog_fixture_parses_and_supports_lookup() {
    let mock = serve(vec![Reply::ok(fixture("datasets_list_trimmed.json"))]).await;
    let list = client(&mock).list_datasets().await.unwrap();
    assert_eq!(list.datasets.len(), 4);
    let ds = list.get("ercot_spp_day_ahead_hourly").unwrap();
    assert_eq!(ds.name.as_deref(), Some("ERCOT SPP Day Ahead Hourly"));
    assert_eq!(ds.time_index_column.as_deref(), Some("interval_start_utc"));
    assert_eq!(ds.subseries_index_column.as_deref(), Some("location"));
    assert_eq!(ds.data_frequency.as_deref(), Some("1_HOUR"));
    assert_eq!(
        ds.column_names(),
        [
            "interval_start_utc",
            "interval_end_utc",
            "location",
            "location_type",
            "market",
            "spp"
        ]
    );
    assert_eq!(list.with_prefix("ercot_").count(), 3);
    assert_eq!(
        list.get("pjm_lmp_day_ahead_hourly")
            .unwrap()
            .all_columns
            .as_ref()
            .unwrap()
            .len(),
        11
    );

    let req = &mock.requests()[0];
    assert_eq!(req.method, "GET");
    assert_eq!(req.path(), "/v1/datasets");
    assert_eq!(req.headers["x-api-key"], KEY);
    assert!(req.headers["user-agent"].starts_with("gridapi-gridstatus/"));
}

#[tokio::test]
async fn dataset_metadata_fixture_parses() {
    let mock = serve(vec![Reply::ok(fixture("dataset_ercot_spp_da.json"))]).await;
    let ds = client(&mock)
        .dataset("ercot_spp_day_ahead_hourly")
        .await
        .unwrap();
    assert_eq!(ds.id, "ercot_spp_day_ahead_hourly");
    assert_eq!(ds.source.as_deref(), Some("ercot"));
    assert_eq!(ds.number_of_rows_approximate, Some(96_129_312));
    assert_eq!(ds.status.as_deref(), Some("active"));
    assert_eq!(
        ds.primary_key_columns.as_deref().unwrap(),
        ["interval_start_utc", "location"]
    );
    assert_eq!(
        mock.requests()[0].path(),
        "/v1/datasets/ercot_spp_day_ahead_hourly"
    );
}

#[tokio::test]
async fn typed_ercot_day_ahead_query_sends_params_and_parses_real_row() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_spp_da_hou.json"))
        .header("x-request-id", "7575a8d5-324c-4e4c")
        .header("x-limit", "3")
        .header("x-page", "1")
        .header("x-page-size", "50000")])
    .await;
    let q = Query::new()
        .start_time("2026-11-17T00:00:00Z")
        .end_time("2026-11-17T03:00:00Z")
        .location("HB_HOUSTON")
        .limit(3);
    let page = client(&mock).ercot_spp_day_ahead_hourly(&q).await.unwrap();

    assert_eq!(page.rows.len(), 3);
    let first = &page.rows[0];
    assert_eq!(first.location, "HB_HOUSTON");
    assert_eq!(first.location_type.as_deref(), Some("Trading Hub"));
    assert_eq!(first.market.as_deref(), Some("DAY_AHEAD_HOURLY"));
    assert_eq!(first.spp, Some(37.31));
    assert_eq!(
        first.interval_start_utc.to_rfc3339(),
        "2026-11-17T00:00:00+00:00"
    );
    assert_eq!(page.rows[2].spp, Some(55.36));
    assert!(!page.has_next_page());
    assert_eq!(page.meta.limit, Some(3));
    assert_eq!(page.meta.page_size, Some(50_000));
    assert_eq!(
        page.response.request_id.as_deref(),
        Some("7575a8d5-324c-4e4c")
    );
    assert_eq!(
        page.dataset.as_ref().unwrap().id,
        "ercot_spp_day_ahead_hourly"
    );

    let req = &mock.requests()[0];
    assert_eq!(req.path(), "/v1/datasets/ercot_spp_day_ahead_hourly/query");
    let sent = req.query();
    assert_eq!(sent["start_time"], "2026-11-17T00:00:00Z");
    assert_eq!(sent["end_time"], "2026-11-17T03:00:00Z");
    assert_eq!(sent["filter_column"], "location");
    assert_eq!(sent["filter_value"], "HB_HOUSTON");
    assert_eq!(sent["limit"], "3");
}

#[tokio::test]
async fn dynamic_records_expose_typed_accessors() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_spp_rt15_hou.json"))]).await;
    let page = client(&mock)
        .query_records("ercot_spp_real_time_15_min", &Query::new().limit(4))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 4);
    let r = &page.rows[0];
    assert_eq!(r.str("location"), Some("HB_HOUSTON"));
    assert_eq!(r.f64("spp"), Some(52.82));
    assert_eq!(r.str("market"), Some("REAL_TIME_15_MIN"));
    assert_eq!(
        r.timestamp("interval_end_utc").unwrap().to_rfc3339(),
        "2026-11-17T00:15:00+00:00"
    );
    assert_eq!(r.f64("nope"), None);
    assert_eq!(page.rows[3].f64("spp"), Some(73.3));
}

#[tokio::test]
async fn ercot_real_time_helper_targets_the_15_min_dataset_id() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_spp_rt15_hou.json"))]).await;
    let page = client(&mock)
        .ercot_spp_real_time_15_minute(&Query::new().location("HB_HOUSTON"))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 4);
    assert_eq!(page.rows[1].spp, Some(50.57));
    assert_eq!(
        mock.requests()[0].path(),
        "/v1/datasets/ercot_spp_real_time_15_min/query"
    );
}

#[tokio::test]
async fn pjm_lmp_rows_carry_components() {
    let mock = serve(vec![Reply::ok(fixture("query_pjm_lmp_da.json"))]).await;
    let page = client(&mock)
        .pjm_lmp_day_ahead_hourly(&Query::new().limit(3))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
    let r = &page.rows[0];
    assert_eq!(r.location, "02AMSTED138 KV  TR2");
    assert_eq!(r.location_id, Some(98_369_975));
    assert_eq!(r.location_short_name.as_deref(), Some("02AMSTED"));
    assert_eq!(r.lmp, Some(76.31));
    assert_eq!(r.energy, Some(100.56));
    assert_eq!(r.congestion, Some(-37.60));
    assert_eq!(r.loss, Some(-0.63));
    assert_eq!(r.ghg, None);
    assert_eq!(
        mock.requests()[0].path(),
        "/v1/datasets/pjm_lmp_day_ahead_hourly/query"
    );
}

#[tokio::test]
async fn fuel_mix_rows_keep_every_fuel() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_fuel_mix.json"))]).await;
    let page = client(&mock)
        .fuel_mix(Iso::Ercot, &Query::new().limit(3))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
    let r = &page.rows[0];
    assert_eq!(r.fuel("natural_gas"), Some(39_545.914_548_757_006));
    assert_eq!(r.fuel("wind"), Some(4_064.634_514_132_17));
    assert_eq!(r.megawatts().len(), 8);
    assert!(r.megawatts().contains_key("coal_and_lignite"));
    assert!(
        (r.total_megawatts() - 59_011.40).abs() < 1.0,
        "{}",
        r.total_megawatts()
    );
    assert_eq!(
        mock.requests()[0].path(),
        "/v1/datasets/ercot_fuel_mix/query"
    );
}

#[tokio::test]
async fn load_rows_parse_integer_load() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_load.json"))]).await;
    let page = client(&mock)
        .load(Iso::Ercot, &Query::new().limit(3))
        .await
        .unwrap();
    assert_eq!(
        page.rows.iter().map(|r| r.load).collect::<Vec<_>>(),
        [Some(64_828.0), Some(54_344.0), Some(34_573.0)]
    );
    assert!(page.rows[0].zones().is_empty());
    assert_eq!(mock.requests()[0].path(), "/v1/datasets/ercot_load/query");
}

#[tokio::test]
async fn ercot_has_no_lmp_dataset_helper() {
    let mock = serve(vec![Reply::ok("{}")]).await;
    let err = client(&mock)
        .lmp_day_ahead_hourly(Iso::Ercot, &Query::new())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidRequest(_)));
    assert!(mock.requests().is_empty());
}

#[tokio::test]
async fn usage_fixture_reports_remaining_quota() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let u = client(&mock).usage().await.unwrap();
    assert_eq!(u.plan_name, "Free");
    assert_eq!(u.limits.api_requests_limit, 250);
    assert_eq!(u.limits.api_rows_returned_limit, 500_000);
    assert_eq!(u.limits.per_second_api_rate_limit, 1);
    assert_eq!(u.current_period_usage.total_requests, 6);
    assert_eq!(u.remaining_requests(), 244);
    assert_eq!(u.remaining_rows(), 499_802);
    assert_eq!(mock.requests()[0].path(), "/v1/api_usage");
}

#[tokio::test]
async fn quota_headers_are_exposed_when_the_server_sends_them() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_load.json"))
        .header("x-ratelimit-limit", "250")
        .header("x-ratelimit-remaining", "100")
        .header("x-rows-in-response", "3")])
    .await;
    let page = client(&mock)
        .query_records("ercot_load", &Query::new())
        .await
        .unwrap();
    assert_eq!(page.response.rate_limit.limit, Some(250));
    assert_eq!(page.response.rate_limit.remaining, Some(100));
    assert_eq!(page.response.rows_in_response, Some(3));
}

#[tokio::test]
async fn resample_and_timezone_reach_the_wire() {
    let mock = serve(vec![Reply::ok(fixture("query_ercot_load.json"))]).await;
    let q = Query::new()
        .start_time("2026-11-01")
        .end_time("2026-11-08")
        .resample(Resample::new(ResampleFrequency::Day1))
        .timezone("market")
        .order(Order::Desc)
        .columns(["interval_start_utc", "load"]);
    client(&mock).query_records("ercot_load", &q).await.unwrap();
    let sent = mock.requests()[0].query();
    assert_eq!(sent["resample_frequency"], "1 day");
    assert_eq!(sent["timezone"], "market");
    assert_eq!(sent["order"], "desc");
    assert_eq!(sent["columns"], "interval_start_utc,load");
}

#[tokio::test]
async fn invalid_query_never_reaches_the_network() {
    let mock = serve(vec![Reply::ok("{}")]).await;
    let c = client(&mock);
    let err = c
        .query_records(
            "ercot_load",
            &Query::new().resample(Resample::new(ResampleFrequency::Day1)),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidRequest(_)));
    let err = c.dataset("../etc/passwd").await.unwrap_err();
    assert!(matches!(err, Error::InvalidRequest(_)));
    assert!(mock.requests().is_empty());
    assert_eq!(c.requests_sent(), 0);
}

#[tokio::test]
async fn retries_429_honouring_retry_after_then_succeeds() {
    let mock = serve(vec![
        Reply::status(429, r#"{"detail":"Rate limit exceeded"}"#).header("retry-after", "0"),
        Reply::ok(fixture("query_ercot_load.json")),
    ])
    .await;
    let c = client(&mock);
    let page = c.query_records("ercot_load", &Query::new()).await.unwrap();
    assert_eq!(page.rows.len(), 3);
    assert_eq!(mock.requests().len(), 2);
    assert_eq!(c.requests_sent(), 2);
}

#[tokio::test]
async fn retries_5xx_with_backoff() {
    let mock = serve(vec![
        Reply::status(503, "unavailable"),
        Reply::status(502, "bad gateway"),
        Reply::ok(fixture("api_usage.json")),
    ])
    .await;
    let u = client(&mock).usage().await.unwrap();
    assert_eq!(u.plan_name, "Free");
    assert_eq!(mock.requests().len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_retries_with_the_last_error() {
    let mock = serve(vec![Reply::status(500, r#"{"detail":"boom"}"#)]).await;
    let err = client(&mock).usage().await.unwrap_err();
    match err {
        Error::Api {
            status,
            ref message,
            ..
        } => {
            assert_eq!(status.as_u16(), 500);
            assert_eq!(message, "boom");
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(mock.requests().len(), 4, "1 attempt + 3 retries");
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let mock = serve(vec![Reply::status(401, r#"{"detail":"Invalid API key"}"#)]).await;
    let err = client(&mock).usage().await.unwrap_err();
    assert!(err.is_auth());
    assert!(!err.is_retryable());
    assert_eq!(mock.requests().len(), 1);
    assert!(err.to_string().contains("Invalid API key"));
    assert!(!err.to_string().contains(KEY));
}

#[tokio::test]
async fn huge_retry_after_aborts_instead_of_sleeping() {
    let mock = serve(vec![Reply::status(429, "{}").header("retry-after", "3600")]).await;
    let started = Instant::now();
    let err = client(&mock).usage().await.unwrap_err();
    assert!(err.is_rate_limited());
    assert_eq!(mock.requests().len(), 1);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn request_budget_is_a_hard_cap() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let c = Client::builder()
        .api_key(KEY)
        .base_url(mock.base_url.clone())
        .min_interval(Duration::ZERO)
        .request_budget(2)
        .build()
        .unwrap();
    c.usage().await.unwrap();
    c.usage().await.unwrap();
    let err = c.usage().await.unwrap_err();
    assert!(matches!(err, Error::BudgetExhausted { budget: 2 }));
    assert_eq!(mock.requests().len(), 2);
    assert_eq!(c.requests_sent(), 2);
}

#[tokio::test]
async fn spent_budget_during_retry_surfaces_the_real_error() {
    let mock = serve(vec![Reply::status(500, r#"{"detail":"boom"}"#)]).await;
    let c = Client::builder()
        .api_key(KEY)
        .base_url(mock.base_url.clone())
        .min_interval(Duration::ZERO)
        .retry(fast_retry())
        .request_budget(1)
        .build()
        .unwrap();
    let err = c.usage().await.unwrap_err();
    assert!(matches!(err, Error::Api { .. }), "{err:?}");
    assert_eq!(mock.requests().len(), 1);
}

#[tokio::test]
async fn throttle_spaces_sequential_requests() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let c = Client::builder()
        .api_key(KEY)
        .base_url(mock.base_url.clone())
        .min_interval(Duration::from_millis(150))
        .build()
        .unwrap();
    let started = Instant::now();
    for _ in 0..3 {
        c.usage().await.unwrap();
    }
    assert!(
        started.elapsed() >= Duration::from_millis(290),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn concurrent_clones_share_one_throttle() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let c = Client::builder()
        .api_key(KEY)
        .base_url(mock.base_url.clone())
        .min_interval(Duration::from_millis(100))
        .build()
        .unwrap();
    let started = Instant::now();
    let tasks: Vec<_> = (0..3)
        .map(|_| {
            let c = c.clone();
            tokio::spawn(async move { c.usage().await.unwrap() })
        })
        .collect();
    for t in tasks {
        t.await.unwrap();
    }
    assert!(
        started.elapsed() >= Duration::from_millis(190),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn separate_instances_do_not_throttle_each_other() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let build = || {
        Client::builder()
            .api_key(KEY)
            .base_url(mock.base_url.clone())
            .min_interval(Duration::from_secs(5))
            .build()
            .unwrap()
    };
    let (a, b) = (build(), build());
    let started = Instant::now();
    a.usage().await.unwrap();
    b.usage().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn walks_cursor_pages_and_stops_at_the_limit() {
    let page1 = r#"{"status_code":200,"data":[{"x":1},{"x":2}],"meta":{"page":1,"limit":3,"page_size":2,"hasNextPage":true,"cursor":"c1"}}"#;
    let page2 = r#"{"status_code":200,"data":[{"x":3},{"x":4}],"meta":{"page":2,"limit":3,"page_size":2,"hasNextPage":true,"cursor":"c2"}}"#;
    let mock = serve(vec![Reply::ok(page1), Reply::ok(page2)]).await;
    let all = client(&mock)
        .query_all::<serde_json::Value>("ercot_load", &Query::new().limit(3).page_size(2), 10)
        .await
        .unwrap();
    assert_eq!(all.rows.len(), 3, "truncated to the limit");
    assert_eq!(all.pages, 2);
    assert!(all.complete);
    let reqs = mock.requests();
    assert!(!reqs[0].query().contains_key("cursor"));
    assert_eq!(reqs[1].query()["cursor"], "c1");
    assert!(!reqs[1].query().contains_key("page"));
}

#[tokio::test]
async fn falls_back_to_page_numbers_without_a_cursor() {
    let page1 = r#"{"data":[{"x":1}],"meta":{"page":1,"hasNextPage":true,"cursor":null}}"#;
    let page2 = r#"{"data":[{"x":2}],"meta":{"page":2,"hasNextPage":false,"cursor":null}}"#;
    let mock = serve(vec![Reply::ok(page1), Reply::ok(page2)]).await;
    let all = client(&mock)
        .query_all::<serde_json::Value>("ercot_load", &Query::new().page_size(1), 10)
        .await
        .unwrap();
    assert_eq!(all.rows.len(), 2);
    assert!(all.complete);
    assert_eq!(mock.requests()[1].query()["page"], "2");
}

#[tokio::test]
async fn max_pages_bounds_request_spend() {
    let page = r#"{"data":[{"x":1}],"meta":{"page":1,"hasNextPage":true,"cursor":"c"}}"#;
    let mock = serve(vec![Reply::ok(page)]).await;
    let all = client(&mock)
        .query_all::<serde_json::Value>("ercot_load", &Query::new(), 2)
        .await
        .unwrap();
    assert_eq!(all.pages, 2);
    assert!(!all.complete);
    assert_eq!(mock.requests().len(), 2);
}

#[tokio::test]
async fn decode_failure_reports_a_snippet_not_the_key() {
    let mock = serve(vec![Reply::ok("<html>maintenance</html>")]).await;
    let err = client(&mock).usage().await.unwrap_err();
    assert!(matches!(err, Error::Decode { .. }));
    let text = err.to_string();
    assert!(text.contains("maintenance"));
    assert!(!text.contains(KEY));
}

#[tokio::test]
async fn dataset_updates_and_raw_escape_hatch() {
    let updates = r#"{"status_code":200,"data":[{"id":18759731,"dataset":"ieso_lmp_real_time_5_min","time_utc":"2025-04-25T00:46:52+00:00","num_rows_updated":0,"num_rows_inserted":288166,"is_backfill":false}],"meta":{"page":1}}"#;
    let mock = serve(vec![Reply::ok(updates), Reply::ok(r#"{"hello":"world"}"#)]).await;
    let c = client(&mock);
    let u = c
        .dataset_updates("ieso_lmp_real_time_5_min", Order::Desc, Some(1))
        .await
        .unwrap();
    assert_eq!(u[0].id, "18759731");
    assert_eq!(u[0].num_rows_inserted, 288_166);
    assert_eq!(u[0].is_backfill, Some(false));
    let (v, meta) = c
        .get_json("/pricing_locations", &[("limit", "1".to_owned())])
        .await
        .unwrap();
    assert_eq!(v["hello"], "world");
    assert_eq!(meta.status.as_u16(), 200);
    let reqs = mock.requests();
    assert_eq!(
        reqs[0].path(),
        "/v1/dataset-updates/ieso_lmp_real_time_5_min"
    );
    assert_eq!(reqs[0].query()["limit"], "1");
    assert_eq!(reqs[1].query()["limit"], "1");
    assert!(c.get_json("no-slash", &[]).await.is_err());
}

#[tokio::test]
async fn custom_user_agent_and_trailing_slash_base_url() {
    let mock = serve(vec![Reply::ok(fixture("api_usage.json"))]).await;
    let c = Client::builder()
        .api_key(KEY)
        .base_url(format!("{}/", mock.base_url))
        .user_agent("my-bot/9.9")
        .min_interval(Duration::ZERO)
        .build()
        .unwrap();
    c.usage().await.unwrap();
    let req = &mock.requests()[0];
    assert_eq!(req.path(), "/v1/api_usage");
    assert_eq!(req.headers["user-agent"], "my-bot/9.9");
}

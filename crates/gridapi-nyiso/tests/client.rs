//! Fixture-driven tests: real NYISO responses served from a local mock server.

// Test helpers fail loudly with `expect`/`panic!`, and `assert!(x.is_empty())` reads best.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::time::Duration;

use gridapi_nyiso::{
    chrono::{Duration as ChronoDuration, NaiveDate, Offset, Utc},
    Client, Dataset, Error, Level, RetryPolicy, Source,
};
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, Respond, ResponseTemplate,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

fn fast_retry(max_retries: u32) -> RetryPolicy {
    RetryPolicy {
        max_retries,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .base_url(server.uri())
        .retry(fast_retry(3))
        .build()
        .expect("client builds")
}

async fn serve_csv(server: &MockServer, url_path: &str, fixture_name: &str) {
    Mock::given(method("GET"))
        .and(path(url_path.to_owned()))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture(fixture_name)))
        .mount(server)
        .await;
}

fn offset_hours(t: &gridapi_nyiso::chrono::DateTime<gridapi_nyiso::chrono::FixedOffset>) -> i32 {
    t.offset().fix().local_minus_utc() / 3600
}

#[tokio::test]
async fn day_ahead_zone_lbmp_matches_the_published_file() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/damlbmp/20261117damlbmp_zone.csv",
        "damlbmp_zone_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Zone, day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 8);
    let first = &rows[0];
    assert_eq!(first.name, "CAPITL");
    assert_eq!(first.ptid, 61757);
    assert!((first.lbmp - 51.66).abs() < 1e-9, "CAPITL 00:00 is $51.66");
    assert!((first.marginal_cost_losses - 1.12).abs() < 1e-9);
    assert!(first.marginal_cost_congestion.abs() < 1e-9);
    assert!((first.energy_component() - 50.54).abs() < 1e-9);
    assert_eq!(first.timestamp.to_rfc3339(), "2026-11-17T00:00:00-05:00");
}

#[tokio::test]
async fn generator_level_uses_the_gen_file() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/damlbmp/20261117damlbmp_gen.csv",
        "damlbmp_gen_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Generator, day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "59TH STREET_GT_1");
    assert_eq!(rows[0].ptid, 24138);
}

#[tokio::test]
async fn real_time_and_integrated_lbmp_parse_quoted_files() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/realtime/20261117realtime_zone.csv",
        "realtime_zone_20261117.csv",
    )
    .await;
    serve_csv(
        &server,
        "/rtlbmp/20261117rtlbmp_zone.csv",
        "rtlbmp_zone_20261117.csv",
    )
    .await;
    let client = client_for(&server);
    let d = day(2026, 11, 17);

    let five_min = client
        .real_time_lbmp(Level::Zone, d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("realtime");
    assert_eq!(five_min.len(), 6);
    assert_eq!(
        five_min[0].timestamp.to_rfc3339(),
        "2026-11-17T00:05:00-05:00"
    );
    assert_eq!(five_min[1].name, "N.Y.C.");

    let hourly = client
        .integrated_real_time_lbmp(Level::Zone, d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("rtlbmp");
    assert_eq!(hourly.len(), 6);
    assert!((hourly[0].lbmp - 45.57).abs() < 1e-9);
}

#[tokio::test]
async fn load_datasets_decode() {
    let server = MockServer::start().await;
    serve_csv(&server, "/pal/20261117pal.csv", "pal_20261117.csv").await;
    serve_csv(&server, "/isolf/20261117isolf.csv", "isolf_20261117.csv").await;
    let client = client_for(&server);
    let d = day(2026, 11, 17);

    let load = client
        .actual_load(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("pal");
    assert_eq!(load.len(), 4);
    assert_eq!(load[1].name, "N.Y.C.");
    assert!((load[1].load_mw - 5455.4564).abs() < 1e-6);
    assert_eq!(
        offset_hours(&load[0].timestamp),
        -5,
        "EST from the Time Zone column"
    );

    let forecast = client
        .load_forecast(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("isolf");
    assert_eq!(forecast.len(), 3);
    assert_eq!(forecast[0].nyiso_mw, Some(14484.0));
    assert_eq!(
        forecast[0].zones.len(),
        11,
        "11 zones, NYISO total split out"
    );
    assert_eq!(forecast[0].zones.get("Capitl"), Some(&1166.0));
    assert_eq!(
        forecast[1].timestamp.to_rfc3339(),
        "2026-11-17T01:00:00-05:00"
    );
}

#[tokio::test]
async fn fuel_mix_ancillary_and_flows_decode() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/rtfuelmix/20261117rtfuelmix.csv",
        "rtfuelmix_20261117.csv",
    )
    .await;
    serve_csv(&server, "/damasp/20261117damasp.csv", "damasp_20261117.csv").await;
    serve_csv(&server, "/rtasp/20261117rtasp.csv", "rtasp_20261117.csv").await;
    serve_csv(
        &server,
        "/ExternalLimitsFlows/20261117ExternalLimitsFlows.csv",
        "externallimitsflows_20261117.csv",
    )
    .await;
    let client = client_for(&server);
    let d = day(2026, 11, 17);

    let mix = client
        .fuel_mix(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fuel mix");
    assert_eq!(mix.len(), 8);
    assert_eq!(mix[0].fuel_category, "Dual Fuel");
    assert!((mix[0].gen_mw - 2801.5).abs() < 1e-9);

    let da = client
        .day_ahead_ancillary(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("damasp");
    assert_eq!(da[0].name, "CAPITL");
    assert_eq!(da[0].spinning_10_min, Some(3.18));
    assert_eq!(da[0].regulation_capacity, Some(6.07));
    assert_eq!(
        da[0].regulation_movement, None,
        "day-ahead has no movement price"
    );

    let rt = client
        .real_time_ancillary(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("rtasp");
    assert_eq!(rt[0].regulation_movement, Some(0.2));
    assert_eq!(rt[0].regulation_capacity, Some(8.29));

    let flows = client
        .interface_flows(d)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("flows");
    assert_eq!(flows[0].interface_name, "CENTRAL EAST - VC");
    assert_eq!(flows[0].point_id, 23330);
    assert_eq!(flows[0].flow_mw, Some(1859.98));
    assert_eq!(flows[0].positive_limit_mw, Some(3306.0));
    assert_eq!(flows[0].negative_limit_mw, Some(-12099.0));
}

#[tokio::test]
async fn fall_back_day_without_a_zone_column_keeps_instants_distinct() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/damlbmp/20261101damlbmp_zone.csv",
        "damlbmp_zone_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Zone, day(2026, 11, 1))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");

    let offsets: Vec<i32> = rows.iter().map(|r| offset_hours(&r.timestamp)).collect();
    assert_eq!(offsets, [-4, -4, -5, -5, -5]);
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    assert!(
        instants
            .windows(2)
            .all(|w| w[1] - w[0] == ChronoDuration::hours(1)),
        "five consecutive hours, none duplicated or skipped: {instants:?}"
    );
}

#[tokio::test]
async fn fall_back_day_with_a_zone_column_trusts_the_column() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/rtfuelmix/20261101rtfuelmix.csv",
        "rtfuelmix_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .fuel_mix(day(2026, 11, 1))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");

    let offsets: Vec<i32> = rows.iter().map(|r| offset_hours(&r.timestamp)).collect();
    assert_eq!(offsets, [-4, -4, -5, -5]);
    assert_eq!(
        rows[1].timestamp.with_timezone(&Utc) + ChronoDuration::minutes(5),
        rows[2].timestamp.with_timezone(&Utc),
        "01:55 EDT is five minutes before 01:00 EST"
    );
}

#[tokio::test]
async fn old_days_come_from_one_monthly_archive_request() {
    let server = MockServer::start().await;
    let archive = Mock::given(method("GET"))
        .and(path("/damlbmp/20261101damlbmp_zone_csv.zip"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("damlbmp_zone_202611_csv.zip")),
        )
        .expect(1)
        .named("monthly archive, fetched once for the whole range");
    archive.mount(&server).await;
    let client = client_for(&server);

    let series = client
        .day_ahead_lbmp(Level::Zone, day(2026, 11, 1))
        .through(day(2026, 11, 2))
        .source(Source::ArchiveOnly)
        .fetch()
        .await
        .expect("fetches");

    assert_eq!(
        series.rows.len(),
        5,
        "day one has five rows; day two is not in the archive"
    );
    assert_eq!(series.missing, [day(2026, 11, 2)]);
}

#[tokio::test]
async fn auto_falls_back_to_the_archive_when_the_daily_file_is_gone() {
    let server = MockServer::start().await;
    // No daily file mounted: the server answers 404 for it.
    Mock::given(method("GET"))
        .and(path("/damlbmp/20261101damlbmp_zone_csv.zip"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("damlbmp_zone_202611_csv.zip")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Zone, day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("served from the archive");

    assert_eq!(rows.len(), 8);
    assert!((rows[0].lbmp - 51.66).abs() < 1e-9);
}

#[tokio::test]
async fn recent_days_use_the_daily_file_and_never_touch_the_archive() {
    let server = MockServer::start().await;
    // UTC and Eastern dates differ by at most a day; both are inside the daily window.
    let today = Utc::now().date_naive();
    let daily = format!("/damlbmp/{}damlbmp_zone.csv", today.format("%Y%m%d"));
    serve_csv(&server, &daily, "damlbmp_zone_20261117.csv").await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/damlbmp/{}damlbmp_zone_csv.zip",
            today.format("%Y%m01")
        )))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .named("archive must not be requested")
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Zone, today)
        .fetch_strict()
        .await
        .expect("served from the daily file");
    assert_eq!(rows.len(), 8);
}

#[tokio::test]
async fn a_day_nobody_published_is_reported_not_fatal() {
    let server = MockServer::start().await;
    let client = client_for(&server);
    let d = day(2025, 1, 4);

    let series = client
        .fuel_mix(d)
        .fetch()
        .await
        .expect("a 404 is a missing day, not an error");
    assert!(series.rows.is_empty());
    assert_eq!(series.missing, [d]);

    let err = client
        .fuel_mix(d)
        .fetch_strict()
        .await
        .expect_err("strict mode refuses missing days");
    assert!(
        matches!(err, Error::MissingDays { count: 1, first } if first == d),
        "got {err:?}"
    );
}

#[tokio::test]
async fn reversed_range_is_rejected_before_any_request() {
    let server = MockServer::start().await;
    let client = client_for(&server);
    let err = client
        .fuel_mix(day(2026, 11, 17))
        .through(day(2026, 11, 16))
        .fetch()
        .await
        .expect_err("end before start");
    assert!(matches!(err, Error::EmptyRange { .. }), "got {err:?}");
    assert!(server
        .received_requests()
        .await
        .expect("recorded")
        .is_empty());
}

/// Fails with the given status for the first `failures` calls, then serves the file.
struct FlakyThenOk {
    failures: usize,
    status: u16,
    retry_after: Option<&'static str>,
    calls: std::sync::atomic::AtomicUsize,
    body: Vec<u8>,
}

impl Respond for FlakyThenOk {
    fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n < self.failures {
            let mut r = ResponseTemplate::new(self.status);
            if let Some(v) = self.retry_after {
                r = r.insert_header("Retry-After", v);
            }
            r
        } else {
            ResponseTemplate::new(200).set_body_bytes(self.body.clone())
        }
    }
}

#[tokio::test]
async fn transient_5xx_and_429_are_retried_until_success() {
    for (status, retry_after) in [(503, None), (500, None), (429, Some("0"))] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rtfuelmix/20261117rtfuelmix.csv"))
            .respond_with(FlakyThenOk {
                failures: 2,
                status,
                retry_after,
                calls: 0.into(),
                body: fixture("rtfuelmix_20261117.csv"),
            })
            .expect(3)
            .mount(&server)
            .await;
        let client = client_for(&server);

        let rows = client
            .fuel_mix(day(2026, 11, 17))
            .source(Source::DailyOnly)
            .fetch_strict()
            .await
            .unwrap_or_else(|e| panic!("status {status}: {e}"));
        assert_eq!(rows.len(), 8, "status {status}");
    }
}

#[tokio::test]
async fn retries_are_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .expect(3) // one attempt plus two retries
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(server.uri())
        .retry(fast_retry(2))
        .build()
        .expect("builds");

    let err = client
        .fuel_mix(day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch()
        .await
        .expect_err("still failing after retries");
    assert!(
        matches!(err, Error::Status { status: 503, .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .fuel_mix(day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch()
        .await
        .expect_err("403");
    assert!(
        matches!(err, Error::Status { status: 403, .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn user_agent_is_configurable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("user-agent", "acme-research/9.9"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("rtfuelmix_20261117.csv")))
        .expect(1)
        .mount(&server)
        .await;
    let client = Client::builder()
        .base_url(server.uri())
        .user_agent("acme-research/9.9")
        .build()
        .expect("builds");

    client
        .fuel_mix(day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("matched the user-agent header");
}

#[tokio::test]
async fn default_user_agent_names_the_crate() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/rtfuelmix/20261117rtfuelmix.csv",
        "rtfuelmix_20261117.csv",
    )
    .await;
    let client = Client::builder()
        .base_url(server.uri())
        .build()
        .expect("builds");
    client
        .fuel_mix(day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");
    let requests = server.received_requests().await.expect("recorded");
    let ua = requests[0]
        .headers
        .get("user-agent")
        .expect("a user agent was sent")
        .to_str()
        .expect("ascii");
    assert!(ua.starts_with("gridapi-nyiso/"), "got {ua}");
}

#[tokio::test]
async fn legacy_ancillary_layout_is_refused_with_a_clear_error() {
    let server = MockServer::start().await;
    let legacy = "\"Time Stamp\",\"East 10 Min Spinning Reserve ($/MWHr\",\"East Regulation ($/MWHr)\"\n\"01/01/2005 00:00\",0.92,20\n";
    Mock::given(method("GET"))
        .and(path("/damasp/20050101damasp.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(legacy))
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .day_ahead_ancillary(day(2005, 1, 1))
        .source(Source::DailyOnly)
        .fetch()
        .await
        .expect_err("legacy layout");
    assert!(
        matches!(err, Error::UnsupportedLayout { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn truncated_old_headers_still_decode() {
    let server = MockServer::start().await;
    let old = "\"Time Stamp\",\"Name\",\"PTID\",\"LBMP ($/MWHr)\",\"Marginal Cost Losses ($/MWHr)\",\"Marginal Cost Congestion ($/MWH\"\n\"01/01/2001 00:00\",\"CAPITL\",61757,71.63,2.54,0.00\n";
    Mock::given(method("GET"))
        .and(path("/damlbmp/20010101damlbmp_zone.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(old))
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lbmp(Level::Zone, day(2001, 1, 1))
        .source(Source::DailyOnly)
        .fetch_strict()
        .await
        .expect("fetches");
    assert!((rows[0].lbmp - 71.63).abs() < 1e-9);
    assert!((rows[0].marginal_cost_losses - 2.54).abs() < 1e-9);
}

#[tokio::test]
async fn a_corrupt_archive_is_an_error_not_a_missing_day() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/damlbmp/20250101damlbmp_zone_csv.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"this is not a zip".to_vec()))
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .day_ahead_lbmp(Level::Zone, day(2025, 1, 1))
        .source(Source::ArchiveOnly)
        .fetch()
        .await
        .expect_err("corrupt zip");
    assert!(matches!(err, Error::Archive { .. }), "got {err:?}");
}

#[tokio::test]
async fn bad_cells_name_the_row_and_column() {
    let server = MockServer::start().await;
    let bad = "Time Stamp,Time Zone,Fuel Category,Gen MW\n11/17/2026 00:05:00,EST,Hydro,3500\n11/17/2026 00:05:00,EST,Wind,n/a\n";
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(bad))
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .fuel_mix(day(2026, 11, 17))
        .source(Source::DailyOnly)
        .fetch()
        .await
        .expect_err("n/a is not a number");
    assert!(
        matches!(&err, Error::BadCell { row: 3, column, value, .. } if column == "Gen MW" && value == "n/a"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn raw_csv_returns_the_bytes_untouched() {
    let server = MockServer::start().await;
    serve_csv(
        &server,
        "/rtfuelmix/20261117rtfuelmix.csv",
        "rtfuelmix_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let raw = client
        .raw_csv(Dataset::FuelMix, day(2026, 11, 17), Source::DailyOnly)
        .await
        .expect("fetches")
        .expect("present");
    assert_eq!(raw.as_ref(), fixture("rtfuelmix_20261117.csv").as_slice());
}

//! Fixture-driven tests: real ISO Express responses served from a local mock
//! server that reproduces the redirect on static reports and the `isox_token`
//! cookie gate on dynamic ones.

// Test helpers fail loudly with `expect`/`panic!`, and `assert!(x.is_empty())` reads best.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use gridapi_isone::{
    chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, Offset, Utc},
    express::{Express, Market, Revision},
    Error, LocationFilter, RetryPolicy, INTERNAL_HUB_ID, INTERNAL_HUB_NAME, ZONE_CONNECTICUT,
};
use wiremock::{
    matchers::{header, header_regex, method, path, query_param},
    Mock, MockServer, Request, Respond, ResponseTemplate,
};

const SESSION_PAGE: &str = "/isoexpress/web/reports/";
const TOKEN: &str = "test-token-123";

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

fn client_for(server: &MockServer) -> Express {
    Express::builder()
        .site_url(server.uri())
        .retry(fast_retry(3))
        .build()
        .expect("client builds")
}

fn hours(t: &DateTime<FixedOffset>) -> i32 {
    t.offset().fix().local_minus_utc() / 3600
}

/// Serve a static report the way ISO-NE does: the `/static-transform/csv/histRpts/...`
/// path 302s to `/histRpts/...`, which holds the file.
async fn serve_static(server: &MockServer, directory: &str, file: &str, fixture_name: &str) {
    let target = format!("/histRpts/{directory}/{file}");
    Mock::given(method("GET"))
        .and(path(format!(
            "/static-transform/csv/histRpts/{directory}/{file}"
        )))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", target.as_str()))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(target))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture(fixture_name)))
        .mount(server)
        .await;
}

/// The session page: sets the `isox_token` cookie.
async fn serve_session(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(SESSION_PAGE))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", format!("isox_token={TOKEN}; Path=/").as_str())
                .set_body_string("<html></html>"),
        )
        .mount(server)
        .await;
}

/// A dynamic report: 200 with the fixture when the cookie is present, else 403,
/// exactly as the live site behaves.
async fn serve_dynamic(server: &MockServer, report: &str, fixture_name: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/transform/csv/{report}")))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture(fixture_name)))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/transform/csv/{report}")))
        .respond_with(ResponseTemplate::new(403).set_body_string("<div>403</div>"))
        .with_priority(10)
        .mount(server)
        .await;
}

#[tokio::test]
async fn day_ahead_lmp_follows_the_redirect_and_reads_the_hub() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "da-lmp",
        "WW_DALMP_ISO_20261117.csv",
        "da_lmp_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let all = client
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(all.len(), 96);

    let hub = client
        .day_ahead_lmp(day(2026, 11, 17))
        .locations(LocationFilter::id(INTERNAL_HUB_ID))
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(hub.len(), 24);
    let he01 = &hub[0];
    assert_eq!(he01.location_id, Some(4000));
    assert_eq!(he01.location_name.as_deref(), Some(INTERNAL_HUB_NAME));
    assert_eq!(he01.location_type.as_deref(), Some("HUB"));
    assert_eq!(he01.hour_ending, "01");
    assert_eq!(
        he01.interval_start.to_rfc3339(),
        "2026-11-17T00:00:00-05:00"
    );
    assert!((he01.lmp - 61.21).abs() < 1e-9);
    assert!((he01.energy - 60.63).abs() < 1e-9);
    assert!(he01.congestion.abs() < 1e-9);
    assert!((he01.loss - 0.58).abs() < 1e-9);
    assert!((he01.energy + he01.congestion + he01.loss - he01.lmp).abs() < 0.011);
}

#[tokio::test]
async fn day_ahead_fall_back_day_has_25_distinct_hours() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "da-lmp",
        "WW_DALMP_ISO_20261101.csv",
        "da_lmp_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lmp(day(2026, 11, 1))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 25);
    let labels: Vec<&str> = rows
        .iter()
        .take(4)
        .map(|r| r.hour_ending.as_str())
        .collect();
    assert_eq!(labels, ["01", "02", "02X", "03"]);
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.interval_start.with_timezone(&Utc))
        .collect();
    assert!(
        instants
            .windows(2)
            .all(|w| w[1] - w[0] == ChronoDuration::hours(1)),
        "25 consecutive hours, none duplicated or skipped: {instants:?}"
    );
    assert_eq!(
        hours(&rows[1].interval_start),
        -4,
        "HE02 is the first 01:00, EDT"
    );
    assert_eq!(
        hours(&rows[2].interval_start),
        -5,
        "HE02X is the second 01:00, EST"
    );
}

#[tokio::test]
async fn day_ahead_spring_forward_day_has_23_hours() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "da-lmp",
        "WW_DALMP_ISO_20260308.csv",
        "da_lmp_20260308_spring.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .day_ahead_lmp(day(2026, 3, 8))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 23);
    assert!(
        rows.iter().all(|r| r.hour_ending != "03"),
        "there is no HE03"
    );
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.interval_start.with_timezone(&Utc))
        .collect();
    assert!(instants
        .windows(2)
        .all(|w| w[1] - w[0] == ChronoDuration::hours(1)));
    assert_eq!(hours(&rows[0].interval_start), -5);
    assert_eq!(hours(&rows[22].interval_start), -4, "HE24 is EDT");
}

#[tokio::test]
async fn real_time_final_has_ids_names_and_types() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "rt-lmp",
        "lmp_rt_final_20261110.csv",
        "rt_lmp_final_20261110.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .real_time_hourly_lmp(day(2026, 11, 10), Revision::Final)
        .locations(LocationFilter::id(4000))
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0].location_name.as_deref(), Some(".H.INTERNAL_HUB"));
    assert_eq!(rows[0].hour_ending, "01", "unquoted hour cells parse too");
}

#[tokio::test]
async fn real_time_prelim_names_locations_only_and_filters_by_name() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "rt-lmp",
        "lmp_rt_prelim_20261117.csv",
        "rt_lmp_prelim_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .real_time_hourly_lmp(day(2026, 11, 17), Revision::Preliminary)
        .locations(LocationFilter::all().with_name(INTERNAL_HUB_NAME))
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(rows.len(), 22, "the hub for the hours in the fixture");
    assert_eq!(rows[0].location_id, None, "the prelim file carries no ids");
    assert_eq!(rows[0].location_type, None);
    assert!(rows
        .iter()
        .all(|r| r.location_name.as_deref() == Some(INTERNAL_HUB_NAME)));

    let by_id = client
        .real_time_hourly_lmp(day(2026, 11, 17), Revision::Preliminary)
        .locations(LocationFilter::id(4000))
        .fetch()
        .await
        .expect("fetches");
    assert!(
        by_id.rows.is_empty(),
        "an id filter cannot match a names-only file"
    );
    assert!(by_id.missing.is_empty(), "but the day was found");
}

#[tokio::test]
async fn real_time_fall_back_final_keeps_the_x_hour() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "rt-lmp",
        "lmp_rt_final_20261101.csv",
        "rt_lmp_final_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);
    let rows = client
        .real_time_hourly_lmp(day(2026, 11, 1), Revision::Final)
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(rows.len(), 25);
    assert_eq!(rows[2].hour_ending, "02X");
    assert_eq!(hours(&rows[2].interval_start), -5);
}

async fn serve_five_minute_day(
    server: &MockServer,
    directory: &str,
    day: &str,
    block0_fixture: &str,
) {
    for (i, block) in ["00-04", "04-08", "08-12", "12-16", "16-20", "20-24"]
        .into_iter()
        .enumerate()
    {
        let file = format!("lmp_5min_{day}_{block}.csv");
        let target = format!("/histRpts/{directory}/{file}");
        Mock::given(method("GET"))
            .and(path(format!(
                "/static-transform/csv/histRpts/{directory}/{file}"
            )))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", target.as_str()))
            .expect(1)
            .mount(server)
            .await;
        let body = if i == 0 {
            fixture(block0_fixture)
        } else {
            b"No data exists for this period.\n".to_vec()
        };
        Mock::given(method("GET"))
            .and(path(target))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(server)
            .await;
    }
}

#[tokio::test]
async fn five_minute_lmp_reads_all_six_blocks_and_dates_them_from_the_request() {
    let server = MockServer::start().await;
    serve_five_minute_day(
        &server,
        "5min-rt-final",
        "20261110",
        "fivemin_final_20261110_00-04.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .five_minute_lmp(day(2026, 11, 10), Revision::Final)
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 4);
    let first = rows.iter().find(|r| r.location_id == 321).expect("loc 321");
    assert_eq!(first.timestamp.to_rfc3339(), "2026-11-10T00:00:00-05:00");
    assert!((first.lmp - 34.00).abs() < 1e-9);
    assert!((first.energy - 34.4).abs() < 1e-9);
    assert!((first.loss - -0.4).abs() < 1e-9);
}

#[tokio::test]
async fn five_minute_fall_back_block_has_a_clean_5_hour_run() {
    let server = MockServer::start().await;
    serve_five_minute_day(
        &server,
        "5min-rt-final",
        "20261101",
        "fivemin_final_20261101_00-04_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .five_minute_lmp(day(2026, 11, 1), Revision::Final)
        .locations(LocationFilter::id(INTERNAL_HUB_ID))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 60, "5 elapsed hours x 12 intervals");
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    assert!(
        instants
            .windows(2)
            .all(|w| w[1] - w[0] == ChronoDuration::minutes(5)),
        "a gapless, duplicate-free 5-minute run"
    );
    assert_eq!(hours(&rows[0].timestamp), -4);
    assert_eq!(hours(&rows[59].timestamp), -5);
}

#[tokio::test]
async fn latest_five_minute_prices_use_the_session_cookie() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/fiveminlmp/current"))
        .and(query_param("type", "prelim"))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("fivemin_current_prelim.csv")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .five_minute_lmp_latest(Revision::Preliminary, &LocationFilter::all())
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].location_id, 321);
    assert_eq!(
        rows[0].timestamp.to_rfc3339(),
        "2026-11-18T01:15:00-05:00",
        "full date-times need no request day"
    );
    assert!((rows[0].lmp - 49.12).abs() < 1e-9);
}

#[tokio::test]
async fn latest_hourly_prices_carry_ids_only() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/hourlylmp/current"))
        .and(query_param("type", "prelim"))
        .and(query_param("market", "rt"))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("hourly_current_rt_prelim.csv")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/hourlylmp/currentrollinginterval"))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("hourly_rolling_rt_prelim.csv")),
        )
        .mount(&server)
        .await;
    let client = client_for(&server);

    let latest = client
        .hourly_lmp_latest(
            Market::RealTime,
            Revision::Preliminary,
            &LocationFilter::all(),
        )
        .await
        .expect("latest");
    assert_eq!(latest.len(), 2);
    assert_eq!(latest[0].location_name, None);

    let rolling = client
        .hourly_lmp_rolling(
            Market::RealTime,
            Revision::Preliminary,
            &LocationFilter::id(4000),
        )
        .await
        .expect("rolling");
    assert_eq!(
        rolling.len(),
        3,
        "the hub for HE23, HE24 and the next day's HE01"
    );
    // Verified live: these are hour-ending labels, 23 begins at 22:00.
    assert_eq!(rolling[0].hour_ending, "23");
    assert_eq!(
        rolling[0].interval_start.to_rfc3339(),
        "2026-11-17T22:00:00-05:00"
    );
    assert!((rolling[0].lmp - 42.67).abs() < 1e-9);
    assert_eq!(
        rolling[2].interval_start.to_rfc3339(),
        "2026-11-18T00:00:00-05:00"
    );
}

/// Regression (): day-ahead hourly prices are served by the latest report as
/// `final` (captured live 2026-11-18, trimmed to two locations); `prelim` is not.
#[tokio::test]
async fn latest_hourly_day_ahead_is_served_as_final() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/hourlylmp/current"))
        .and(query_param("type", "final"))
        .and(query_param("market", "da"))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(fixture("hourly_current_da_final.csv")),
        )
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .hourly_lmp_latest(Market::DayAhead, Revision::Final, &LocationFilter::id(4000))
        .await
        .expect("day-ahead final");
    assert_eq!(rows.len(), 1);
    assert!((rows[0].lmp - 56.00).abs() < 1e-9);
    assert_eq!(rows[0].hour_ending, "22");
}

/// Regression (): ISO-NE answers HTTP 500 for day-ahead `prelim`, real-time
/// `final`, and every non-real-time-preliminary rolling window (checked live 2026-11-18).
/// Those pairs are refused locally, without a request.
#[tokio::test]
async fn hourly_pairs_iso_ne_cannot_serve_are_refused_without_a_request() {
    let server = MockServer::start().await;
    let client = client_for(&server);
    let all = LocationFilter::all();

    for (market, revision) in [
        (Market::DayAhead, Revision::Preliminary),
        (Market::RealTime, Revision::Final),
    ] {
        let err = client
            .hourly_lmp_latest(market, revision, &all)
            .await
            .expect_err("unsupported pair");
        assert!(matches!(err, Error::UnsupportedQuery(_)), "got {err:?}");
    }
    for (market, revision) in [
        (Market::DayAhead, Revision::Preliminary),
        (Market::DayAhead, Revision::Final),
        (Market::RealTime, Revision::Final),
    ] {
        let err = client
            .hourly_lmp_rolling(market, revision, &all)
            .await
            .expect_err("unsupported pair");
        assert!(matches!(err, Error::UnsupportedQuery(_)), "got {err:?}");
    }
    let seen = server.received_requests().await.expect("recording is on");
    assert!(seen.is_empty(), "nothing is sent: {seen:?}");
}

#[tokio::test]
async fn fuel_mix_decodes_and_requests_the_session_once() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SESSION_PAGE))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", format!("isox_token={TOKEN}; Path=/").as_str()),
        )
        .expect(1)
        .named("session page is fetched once for the whole client")
        .mount(&server)
        .await;
    serve_dynamic(&server, "genfuelmix", "genfuelmix_20261117.csv").await;
    let client = client_for(&server);

    let first = client
        .fuel_mix(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("first");
    let second = client
        .fuel_mix(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("second");

    assert_eq!(first.len(), 14);
    assert_eq!(first, second);
    assert_eq!(first[0].fuel_category, "Batteries");
    assert_eq!(first[0].rollup, "Batteries");
    assert!((first[0].gen_mw - 1.0).abs() < 1e-9);
    assert!(!first[0].marginal);
    assert_eq!(first[0].timestamp.to_rfc3339(), "2026-11-17T00:02:58-05:00");
    let gas = first
        .iter()
        .find(|r| r.fuel_category == "Natural Gas")
        .expect("gas");
    assert!((gas.gen_mw - 7892.0).abs() < 1e-9);
}

#[tokio::test]
async fn fuel_mix_fall_back_day_is_monotonic_with_one_offset_change() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(&server, "genfuelmix", "genfuelmix_20261101_fallback.csv").await;
    let client = client_for(&server);

    let rows = client
        .fuel_mix(day(2026, 11, 1))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 211);
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    assert!(
        instants.windows(2).all(|w| w[0] < w[1]),
        "strictly increasing instants"
    );
    let offsets: Vec<i32> = rows.iter().map(|r| hours(&r.timestamp)).collect();
    let changes = offsets.windows(2).filter(|w| w[0] != w[1]).count();
    assert_eq!(changes, 1, "EDT then EST, once");
    assert_eq!(offsets[0], -4);
    assert_eq!(offsets[rows.len() - 1], -5);
    let span = instants[instants.len() - 1] - instants[0];
    assert!(
        span > ChronoDuration::hours(24),
        "25 elapsed hours, got {span}"
    );
}

/// Live finding (2026-11-18): ISO Express binds `isox_token` to the `User-Agent` that
/// fetched it; the same jar with a different agent is refused. The client must use one
/// agent for both the session page and the report.
#[tokio::test]
async fn the_session_page_and_the_report_use_the_same_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SESSION_PAGE))
        .and(header("user-agent", "acme-research/9.9"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", format!("isox_token={TOKEN}; Path=/").as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/genfuelmix"))
        .and(header("user-agent", "acme-research/9.9"))
        .and(header_regex("cookie", &format!("isox_token={TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("genfuelmix_20261117.csv")))
        .expect(1)
        .mount(&server)
        .await;
    let client = Express::builder()
        .site_url(server.uri())
        .user_agent("acme-research/9.9")
        .retry(fast_retry(0))
        .build()
        .expect("builds");

    let rows = client
        .fuel_mix(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("both requests carried the configured agent");
    assert_eq!(rows.len(), 14);
}

#[tokio::test]
async fn a_403_refreshes_the_session_and_retries_once() {
    struct ForbiddenOnce(AtomicUsize);
    impl Respond for ForbiddenOnce {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(403)
            } else {
                ResponseTemplate::new(200).set_body_bytes(fixture("genfuelmix_20261117.csv"))
            }
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(SESSION_PAGE))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", format!("isox_token={TOKEN}; Path=/").as_str()),
        )
        .expect(2)
        .named("initial session plus one refresh")
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/genfuelmix"))
        .respond_with(ForbiddenOnce(AtomicUsize::new(0)))
        .expect(2)
        .mount(&server)
        .await;
    let client = client_for(&server);

    let rows = client
        .fuel_mix(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("recovers");
    assert_eq!(rows.len(), 14);
}

#[tokio::test]
async fn a_403_that_survives_a_refresh_is_a_session_error() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    Mock::given(method("GET"))
        .and(path("/transform/csv/genfuelmix"))
        .respond_with(ResponseTemplate::new(403))
        .expect(2)
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .fuel_mix(day(2026, 11, 17))
        .fetch()
        .await
        .expect_err("still forbidden");
    assert!(matches!(err, Error::SessionRejected { .. }), "got {err:?}");
}

#[tokio::test]
async fn the_no_data_body_means_a_missing_day_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("no_data.txt")))
        .mount(&server)
        .await;
    let client = client_for(&server);
    let d = day(2030, 11, 17);

    let series = client.day_ahead_lmp(d).fetch().await.expect("not an error");
    assert!(series.rows.is_empty());
    assert_eq!(series.missing, [d]);

    let err = client
        .day_ahead_lmp(d)
        .fetch_strict()
        .await
        .expect_err("strict");
    assert!(
        matches!(err, Error::MissingDays { count: 1, first, .. } if first == d),
        "got {err:?}"
    );
}

#[tokio::test]
async fn an_empty_dynamic_report_is_a_missing_day() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    let empty = "\"C\",\"Dispatch Fuel Mix Report\"\n\"H\",\"Date\",\"Time\",\"Fuel Category\",\"Fuel Category Rollup\",\"Gen Mw\",\"Marginal Flag\"\n\"H\",\"Local Date\",\"Local Time\",\"String\",\"String\",\"MWh\",\"String\"\n\"T\",\"0 lines\"\n";
    Mock::given(method("GET"))
        .and(path("/transform/csv/genfuelmix"))
        .respond_with(ResponseTemplate::new(200).set_body_string(empty))
        .mount(&server)
        .await;
    let client = client_for(&server);
    let d = day(2030, 11, 17);

    let series = client.fuel_mix(d).fetch().await.expect("fetches");
    assert!(series.rows.is_empty());
    assert_eq!(series.missing, [d]);
}

#[tokio::test]
async fn a_range_reports_which_days_were_missing() {
    let server = MockServer::start().await;
    serve_static(
        &server,
        "da-lmp",
        "WW_DALMP_ISO_20261117.csv",
        "da_lmp_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let series = client
        .day_ahead_lmp(day(2026, 11, 17))
        .through(day(2026, 11, 18))
        .locations(LocationFilter::id(4000))
        .fetch()
        .await
        .expect("fetches");

    assert_eq!(series.rows.len(), 24);
    assert_eq!(
        series.missing,
        [day(2026, 11, 18)],
        "the 18th has no mock, so a 404"
    );
}

#[tokio::test]
async fn reversed_range_is_rejected_before_any_request() {
    let server = MockServer::start().await;
    let client = client_for(&server);
    let err = client
        .day_ahead_lmp(day(2026, 11, 17))
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

#[tokio::test]
async fn a_truncated_download_is_detected_from_the_trailer() {
    let server = MockServer::start().await;
    let full = String::from_utf8(fixture("da_lmp_20261117.csv")).expect("utf8");
    // Keep the first 20 lines and the trailer, which still promises all 96 rows.
    let lines: Vec<&str> = full.lines().collect();
    let trailer = lines.last().copied().expect("fixture has a trailer");
    let kept: Vec<&str> = lines
        .iter()
        .take(20)
        .copied()
        .chain(std::iter::once(trailer))
        .collect();
    let cut = kept.join("\n") + "\n";
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(cut))
        .mount(&server)
        .await;
    let client = client_for(&server);

    let err = client
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch()
        .await
        .expect_err("the trailer promises 96 rows");
    assert!(
        matches!(err, Error::Truncated { expected: 96, .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn system_load_and_demand_reports_decode() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(
        &server,
        "hourlysystemdemand",
        "hourly_system_load_20261117.csv",
    )
    .await;
    serve_dynamic(&server, "hourlydayaheaddemand", "da_demand_20261117.csv").await;
    serve_dynamic(&server, "fiveminutesystemload", "fivemin_load_20261117.csv").await;
    let client = client_for(&server);
    let d = day(2026, 11, 17);

    let load = client
        .hourly_system_load(d)
        .fetch_strict()
        .await
        .expect("hourly load");
    assert_eq!(load.len(), 22);
    assert_eq!(load[0].hour_ending, "01");
    assert!((load[0].mwh - 9292.4).abs() < 1e-9);
    assert_eq!(
        load[0].interval_start.to_rfc3339(),
        "2026-11-17T00:00:00-05:00"
    );

    let da = client
        .day_ahead_demand(d)
        .fetch_strict()
        .await
        .expect("da demand");
    assert_eq!(da.len(), 24);
    assert!((da[0].mwh - 12550.0).abs() < 1e-9);

    let five = client
        .five_minute_system_load(d)
        .fetch_strict()
        .await
        .expect("5 min load");
    assert_eq!(five.len(), 6);
    assert!((five[0].total_load_mw - 10781.810).abs() < 1e-9);
    assert!((five[0].native_load_mw - 9464.252).abs() < 1e-9);
    assert!((five[0].asset_related_load_mw - 291.8).abs() < 1e-9);
    assert_eq!(five[0].total_load_with_solar_mw, Some(10781.810));
    assert_eq!(five[1].timestamp.to_rfc3339(), "2026-11-17T00:05:00-05:00");
}

#[tokio::test]
async fn hourly_load_on_the_fall_back_day_has_the_x_hour() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(
        &server,
        "hourlysystemdemand",
        "hourly_system_load_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .hourly_system_load(day(2026, 11, 1))
        .fetch_strict()
        .await
        .expect("fetches");

    let labels: Vec<&str> = rows.iter().map(|r| r.hour_ending.as_str()).collect();
    assert_eq!(labels, ["01", "02", "02X", "03", "04"]);
    let instants: Vec<_> = rows
        .iter()
        .map(|r| r.interval_start.with_timezone(&Utc))
        .collect();
    assert!(instants
        .windows(2)
        .all(|w| w[1] - w[0] == ChronoDuration::hours(1)));
}

#[tokio::test]
async fn five_minute_load_repeats_resolve_to_distinct_instants() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(
        &server,
        "fiveminutesystemload",
        "fivemin_load_20261101_fallback.csv",
    )
    .await;
    let client = client_for(&server);

    let rows = client
        .five_minute_system_load(day(2026, 11, 1))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 8);
    let offsets: Vec<i32> = rows.iter().map(|r| hours(&r.timestamp)).collect();
    assert_eq!(offsets, [-4, -4, -4, -4, -5, -5, -5, -5]);
    assert!(rows.windows(2).all(|w| w[0].timestamp < w[1].timestamp));
}

#[tokio::test]
async fn zonal_load_filters_by_zone_id() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(
        &server,
        "fiveminuteestimatedzonalload",
        "zonal_load_20261117.csv",
    )
    .await;
    let client = client_for(&server);

    let ct = client
        .zonal_load(day(2026, 11, 17))
        .locations(LocationFilter::id(ZONE_CONNECTICUT))
        .fetch_strict()
        .await
        .expect("fetches");

    assert_eq!(ct.len(), 2, "the 16 fixture rows hold 8 zones at 2 times");
    assert!(ct
        .iter()
        .all(|z| z.zone_name == ".Z.CONNECTICUT" && z.zone_id == 4004));
    assert!((ct[0].native_load_mw - 1992.500).abs() < 1e-9);
    assert!(ct[0].behind_the_meter_solar_mw.abs() < 1e-9);
}

#[tokio::test]
async fn regulation_prices_and_reserve_requirements_decode() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(&server, "fiveminrcp", "rcp_20261117.csv").await;
    serve_dynamic(
        &server,
        "hourlyrequirements",
        "reserve_requirements_20261117.csv",
    )
    .await;
    let client = client_for(&server);
    let d = day(2026, 11, 17);

    let rcp = client
        .regulation_prices(d, Revision::Preliminary)
        .fetch_strict()
        .await
        .expect("rcp");
    assert_eq!(rcp.len(), 5);
    assert!((rcp[0].service_price - 0.04).abs() < 1e-9);
    assert!((rcp[0].capacity_price - 120.89).abs() < 1e-9);
    assert_eq!(rcp[0].timestamp.to_rfc3339(), "2026-11-17T00:00:01-05:00");

    let req = client
        .reserve_requirements(d)
        .locations(LocationFilter::id(7000))
        .fetch_strict()
        .await
        .expect("requirements");
    assert_eq!(req.len(), 2, "system zone, two hours in the fixture");
    assert_eq!(req[0].reserve_zone_id, 7000);
    assert!((req[0].ten_minute_spinning_mw - 275.0).abs() < 1e-9);
    assert!((req[0].ten_minute_mw - 968.383).abs() < 1e-9);
    assert!((req[0].total_mw - 2485.716).abs() < 1e-9);
    assert_eq!(req[1].hour_ending, "02");
}

#[tokio::test]
async fn transient_failures_are_retried_and_client_errors_are_not() {
    struct FlakyThenOk(AtomicUsize);
    impl Respond for FlakyThenOk {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_bytes(fixture("da_lmp_20261117.csv"))
            }
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(FlakyThenOk(AtomicUsize::new(0)))
        .expect(3)
        .mount(&server)
        .await;
    let rows = client_for(&server)
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("succeeds on the third try");
    assert_eq!(rows.len(), 96);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&server)
        .await;
    let err = client_for(&server)
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch()
        .await
        .expect_err("400");
    assert!(
        matches!(err, Error::Status { status: 400, .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn retries_are_bounded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502))
        .expect(3)
        .mount(&server)
        .await;
    let client = Express::builder()
        .site_url(server.uri())
        .retry(fast_retry(2))
        .build()
        .expect("builds");
    let err = client
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch()
        .await
        .expect_err("502s");
    assert!(
        matches!(err, Error::Status { status: 502, .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn user_agent_is_configurable_and_defaults_to_the_crate() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(header("user-agent", "acme-research/9.9"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("da_lmp_20261117.csv")))
        .expect(1)
        .mount(&server)
        .await;
    Express::builder()
        .site_url(server.uri())
        .user_agent("acme-research/9.9")
        .build()
        .expect("builds")
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("matched the user-agent header");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture("da_lmp_20261117.csv")))
        .mount(&server)
        .await;
    client_for(&server)
        .day_ahead_lmp(day(2026, 11, 17))
        .fetch_strict()
        .await
        .expect("fetches");
    let requests = server.received_requests().await.expect("recorded");
    let ua = requests[0]
        .headers
        .get("user-agent")
        .expect("sent")
        .to_str()
        .expect("ascii");
    assert!(ua.starts_with("gridapi-isone/"), "got {ua}");
}

#[tokio::test]
async fn raw_csv_returns_untouched_bytes_and_none_for_no_data() {
    let server = MockServer::start().await;
    serve_session(&server).await;
    serve_dynamic(&server, "genfuelmix", "genfuelmix_20261117.csv").await;
    serve_static(
        &server,
        "da-lmp",
        "WW_DALMP_ISO_20301117.csv",
        "no_data.txt",
    )
    .await;
    let client = client_for(&server);

    let raw = client
        .raw_csv("/transform/csv/genfuelmix?start=20261117")
        .await
        .expect("fetches")
        .expect("present");
    assert_eq!(raw.as_ref(), fixture("genfuelmix_20261117.csv").as_slice());

    let none = client
        .raw_csv("/static-transform/csv/histRpts/da-lmp/WW_DALMP_ISO_20301117.csv")
        .await
        .expect("fetches");
    assert!(none.is_none());
}

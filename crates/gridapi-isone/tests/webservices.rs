//! Tests for the authenticated Web Services client against a mock server.
//!
//! The JSON fixtures in `tests/fixtures/ws/` are authored from ISO-NE's published
//! schema and the JAXB/Jettison conventions the API uses, because capturing real
//! responses needs an ISO Express account. See the README's "Verified" section.

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
    chrono::NaiveDate,
    express::Revision,
    webservices::{HourlyLmpFeed, Scope, WebServices, Window},
    Credentials, Error, RetryPolicy,
};
use wiremock::{
    matchers::{basic_auth, header, method, path},
    Mock, MockServer, Request, Respond, ResponseTemplate,
};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/ws/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 17).expect("date")
}

fn client_for(server: &MockServer) -> WebServices {
    WebServices::builder(Credentials::new("alice", "s3cret"))
        .base_url(server.uri())
        .retry(RetryPolicy {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        })
        .build()
        .expect("client builds")
}

async fn serve_json(server: &MockServer, url_path: &str, body: String) {
    Mock::given(method("GET"))
        .and(path(url_path.to_owned()))
        .and(basic_auth("alice", "s3cret"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn five_minute_hub_lmps_decode_and_send_basic_auth_and_json_accept() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/fiveminutelmp/prelim/current/location/4000.json"))
        .and(basic_auth("alice", "s3cret"))
        .and(header("accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("five_min_lmps_hub.json")))
        .expect(1)
        .mount(&server)
        .await;

    let rows = client_for(&server)
        .five_minute_lmp(
            Revision::Preliminary,
            Window::Current,
            Scope::Location(4000),
        )
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].location.id, 4000);
    assert_eq!(rows[0].location.name, ".H.INTERNAL_HUB");
    assert_eq!(rows[0].location.kind.as_deref(), Some("HUB"));
    assert_eq!(rows[0].lmp_total, Some(52.19));
    assert_eq!(rows[0].energy_component, Some(79.55));
    assert_eq!(rows[0].loss_component, Some(0.44));
    assert_eq!(rows[0].begin_date.to_rfc3339(), "2026-11-17T20:00:00-05:00");
    assert_eq!(rows[1].congestion_component, Some(0.1));
}

#[tokio::test]
async fn a_single_record_and_string_numbers_decode() {
    let server = MockServer::start().await;
    serve_json(
        &server,
        "/fiveminutelmp/final/day/20261117/location/4000.json",
        fixture("five_min_lmps_single.json"),
    )
    .await;

    let rows = client_for(&server)
        .five_minute_lmp(Revision::Final, Window::Day(day()), Scope::Location(4000))
        .await
        .expect("fetches");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].lmp_total, Some(52.19));
}

#[tokio::test]
async fn an_empty_result_is_an_empty_vec() {
    let server = MockServer::start().await;
    serve_json(
        &server,
        "/fiveminutelmp/prelim/current/all.json",
        fixture("empty.json"),
    )
    .await;

    let rows = client_for(&server)
        .five_minute_lmp(Revision::Preliminary, Window::Current, Scope::All)
        .await
        .expect("fetches");
    assert!(rows.is_empty());
}

#[tokio::test]
async fn hourly_lmp_feeds_map_to_their_paths() {
    let server = MockServer::start().await;
    serve_json(
        &server,
        "/hourlylmp/da/final/day/20261117.json",
        fixture("hourly_lmps_da.json"),
    )
    .await;
    serve_json(
        &server,
        "/hourlylmp/rt/prelim/current/location/4000.json",
        fixture("hourly_lmps_da.json"),
    )
    .await;
    serve_json(
        &server,
        "/hourlylmp/rt/final/day/20261117/location/4000.json",
        fixture("hourly_lmps_da.json"),
    )
    .await;
    let client = client_for(&server);

    let da = client
        .hourly_lmp(HourlyLmpFeed::DayAheadFinal, Window::Day(day()), Scope::All)
        .await
        .expect("da");
    assert_eq!(da.len(), 2);
    assert_eq!(da[0].lmp_total, Some(50.03));
    assert_eq!(da[1].begin_date.to_rfc3339(), "2026-11-17T01:00:00-05:00");

    client
        .hourly_lmp(
            HourlyLmpFeed::RealTimePreliminary,
            Window::Current,
            Scope::Location(4000),
        )
        .await
        .expect("rt prelim");
    client
        .hourly_lmp(
            HourlyLmpFeed::RealTimeFinal,
            Window::Day(day()),
            Scope::Location(4000),
        )
        .await
        .expect("rt final");
}

#[tokio::test]
async fn combinations_the_api_does_not_serve_are_refused_without_a_request() {
    let server = MockServer::start().await;
    let client = client_for(&server);

    let cases: Vec<Result<usize, Error>> = vec![
        client
            .hourly_lmp(HourlyLmpFeed::RealTimeFinal, Window::Current, Scope::All)
            .await
            .map(|v| v.len()),
        client
            .five_minute_lmp(Revision::Final, Window::Day(day()), Scope::All)
            .await
            .map(|v| v.len()),
        client
            .day_ahead_hourly_demand(Window::Day(day()), Scope::All)
            .await
            .map(|v| v.len()),
    ];
    for case in cases {
        let err = case.expect_err("unsupported");
        assert!(matches!(err, Error::UnsupportedQuery(_)), "got {err:?}");
    }
    assert!(server
        .received_requests()
        .await
        .expect("recorded")
        .is_empty());
}

#[tokio::test]
async fn fuel_mix_load_forecast_and_locations_decode() {
    let server = MockServer::start().await;
    serve_json(
        &server,
        "/genfuelmix/current.json",
        fixture("gen_fuel_mixes.json"),
    )
    .await;
    serve_json(
        &server,
        "/fiveminutesystemload/day/20261117.json",
        fixture("five_min_system_loads.json"),
    )
    .await;
    serve_json(&server, "/locations/all.json", fixture("locations.json")).await;
    let client = client_for(&server);

    let mix = client.fuel_mix(Window::Current).await.expect("mix");
    assert_eq!(mix.len(), 2);
    assert_eq!(mix[0].fuel_category_rollup, "Natural Gas");
    assert_eq!(mix[0].gen_mw, Some(7892.0));
    assert_eq!(mix[0].marginal_flag.as_deref(), Some("No"));

    let load = client
        .five_minute_system_load(Window::Day(day()))
        .await
        .expect("load");
    assert_eq!(load[0].load_mw, Some(10781.810));
    assert_eq!(load[0].ard_demand, Some(291.8));
    assert_eq!(load[0].system_load_btm_pv, Some(10781.810));

    let locations = client.locations().await.expect("locations");
    assert_eq!(locations.len(), 2);
    assert_eq!(locations[1].location_id, 4001);
    assert_eq!(locations[1].location_type, "LOAD ZONE");
}

#[tokio::test]
async fn demand_forecast_flows_and_reserves_use_their_documented_paths() {
    let server = MockServer::start().await;
    for (url_path, body) in [
        (
            "/hourlysysload/current/location/4000.json",
            r#"{"HourlySystemLoads":{"HourlySystemLoad":{"BeginDate":"2026-11-17T00:00:00.000-05:00","Location":{"@LocId":4000,"@LocType":"HUB","$":".H.INTERNAL_HUB"},"Load":9292.4}}}"#,
        ),
        (
            "/dayaheadhourlydemand/day/20261117/location/4000.json",
            r#"{"HourlyDaDemands":{"HourlyDaDemand":[{"BeginDate":"2026-11-17T00:00:00.000-05:00","Location":{"@LocId":4000,"$":".H.INTERNAL_HUB"},"Load":12550.0}]}}"#,
        ),
        (
            "/hourlyloadforecast/day/20261117.json",
            r#"{"HourlyLoadForecasts":{"HourlyLoadForecast":[{"BeginDate":"2026-11-17T00:00:00.000-05:00","CreationDate":"2026-11-16T10:00:00.000-05:00","LoadMw":11500,"LoadAdjustmentMw":0,"NetLoadMw":11500}]}}"#,
        ),
        (
            "/fiveminuteexternalflow/current/location/4010.json",
            r#"{"ExternalFlows":{"ExternalFlow":{"BeginDate":"2026-11-17T00:00:00.000-05:00","Location":{"@LocId":4010,"@LocType":"EXT. NODE","$":".I.SALBRYNB345 1"},"ActualFlow":-120.5,"ImportLimit":1000,"ExportLimit":-1000}}}"#,
        ),
        (
            "/fiveminutercp/prelim/current.json",
            r#"{"FiveMinRcps":{"FiveMinRcp":{"BeginDate":"2026-11-17T00:00:00.000-05:00","RegServiceClearingPrice":0.04,"RegCapacityClearingPrice":120.89}}}"#,
        ),
        (
            "/fiveminutereserveprice/final/day/20261117.json",
            r#"{"FiveMinReservePrices":{"FiveMinReservePrice":[{"BeginDate":"2026-11-17T00:00:00.000-05:00","ReserveZoneId":7000,"ReserveZoneName":"ROS","Total10MinRequirement":1982.4,"TmsrClearingPrice":0}]}}"#,
        ),
    ] {
        serve_json(&server, url_path, body.to_owned()).await;
    }
    let client = client_for(&server);

    let sysload = client
        .hourly_system_load(Window::Current, Scope::Location(4000))
        .await
        .expect("sysload");
    assert_eq!(sysload[0].load, Some(9292.4));
    assert_eq!(sysload[0].location.id, 4000);

    let demand = client
        .day_ahead_hourly_demand(Window::Day(day()), Scope::Location(4000))
        .await
        .expect("demand");
    assert_eq!(demand[0].load, Some(12550.0));

    let forecast = client
        .load_forecast(Window::Day(day()))
        .await
        .expect("forecast");
    assert_eq!(forecast[0].load_mw, Some(11500.0));
    assert_eq!(
        forecast[0].creation_date.to_rfc3339(),
        "2026-11-16T10:00:00-05:00"
    );

    let flows = client
        .external_flows(Window::Current, Scope::Location(4010))
        .await
        .expect("flows");
    assert_eq!(flows[0].actual_flow, Some(-120.5));
    assert_eq!(flows[0].location.kind.as_deref(), Some("EXT. NODE"));

    let rcp = client
        .regulation_prices(Revision::Preliminary, Window::Current)
        .await
        .expect("rcp");
    assert_eq!(rcp[0].reg_capacity_clearing_price, Some(120.89));

    let reserve = client
        .reserve_prices(Revision::Final, Window::Day(day()))
        .await
        .expect("reserve");
    assert_eq!(reserve[0].reserve_zone_id, Some(7000));
    assert_eq!(reserve[0].total_10_min_requirement, Some(1982.4));
}

#[tokio::test]
async fn bad_credentials_are_unauthorized_and_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("WWW-Authenticate", "Basic realm=\"DataSourceRealm\""),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client_for(&server).locations().await.expect_err("401");
    assert!(matches!(err, Error::Unauthorized { .. }), "got {err:?}");
    assert!(
        err.to_string().contains("ISONE_USERNAME"),
        "tells the user what to check: {err}"
    );
}

#[tokio::test]
async fn the_wrong_password_never_reaches_the_json_decoder() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(basic_auth("alice", "right"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture("locations.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401).set_body_string("<html>401</html>"))
        .with_priority(10)
        .mount(&server)
        .await;

    let err = WebServices::builder(Credentials::new("alice", "wrong"))
        .base_url(server.uri())
        .build()
        .expect("builds")
        .locations()
        .await
        .expect_err("wrong password");
    assert!(matches!(err, Error::Unauthorized { .. }), "got {err:?}");
}

#[tokio::test]
async fn server_errors_are_retried() {
    struct FlakyThenOk(AtomicUsize);
    impl Respond for FlakyThenOk {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            if self.0.fetch_add(1, Ordering::SeqCst) < 2 {
                ResponseTemplate::new(503).insert_header("Retry-After", "0")
            } else {
                ResponseTemplate::new(200).set_body_string(fixture("locations.json"))
            }
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(FlakyThenOk(AtomicUsize::new(0)))
        .expect(3)
        .mount(&server)
        .await;
    let locations = client_for(&server)
        .locations()
        .await
        .expect("third try works");
    assert_eq!(locations.len(), 2);
}

#[tokio::test]
async fn a_non_json_success_body_is_a_json_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;
    let err = client_for(&server).locations().await.expect_err("html");
    assert!(matches!(err, Error::Json { .. }), "got {err:?}");
}

#[tokio::test]
async fn a_record_missing_a_required_field_is_an_error_with_the_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"FiveMinLmps":{"FiveMinLmp":[{"Location":{"@LocId":4000,"$":"x"},"LmpTotal":1}]}}"#,
        ))
        .mount(&server)
        .await;
    let err = client_for(&server)
        .five_minute_lmp(Revision::Preliminary, Window::Current, Scope::All)
        .await
        .expect_err("no BeginDate");
    assert!(
        matches!(&err, Error::Json { url, .. } if url.ends_with("/fiveminutelmp/prelim/current/all.json")),
        "got {err:?}"
    );
}

#[tokio::test]
async fn raw_records_reach_endpoints_without_a_typed_method() {
    let server = MockServer::start().await;
    serve_json(
        &server,
        "/ams/current.json",
        r#"{"AnnualMaintenanceSchedule":{"AmsWeek":[{"week":1},{"week":2}]}}"#.to_owned(),
    )
    .await;
    let records = client_for(&server)
        .raw_records("/ams/current")
        .await
        .expect("fetches");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["week"], 1);
}

#[tokio::test]
async fn credentials_come_from_the_environment_names_the_brief_specifies() {
    // The names are a public contract; the lookup form avoids mutating process env in tests.
    let both = |name: &str| match name {
        "ISONE_USERNAME" => Some("alice".to_owned()),
        "ISONE_PASSWORD" => Some("s3cret".to_owned()),
        _ => None,
    };
    let creds = Credentials::from_lookup(both).expect("both present");
    assert_eq!(creds.username(), "alice");
    assert!(!format!("{creds:?}").contains("s3cret"));
}

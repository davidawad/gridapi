//! Every typed endpoint, decoded from synthetic fixtures (real shapes, invented values) served by an in-process server.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

mod common;

use chrono::NaiveDate;
use common::{fixture, Reply, Server};
#[cfg(feature = "miso-dataexchange")]
use gridapi_miso::PricingProduct;
use gridapi_miso::{
    Client, Component, Endpoints, Error, FiveMinuteWindow, FuelMixView, LmpReportKind, NodeType,
    Renewable, RetryPolicy,
};

async fn client_for(routes: Vec<(&str, Vec<Reply>)>) -> (Client, Server) {
    let server = Server::start(routes).await;
    let client = Client::builder()
        .endpoints(Endpoints::all_at(&server.base))
        .retry(RetryPolicy::none())
        .build()
        .expect("client");
    (client, server)
}

fn ok(path_fixture: &str) -> Vec<Reply> {
    vec![Reply::ok(fixture(path_fixture))]
}

#[tokio::test]
async fn ex_ante_lmp_decodes_hubs() {
    let (c, _s) = client_for(vec![(
        "/api/MarketPricing/GetExAnteLmp",
        ok("ex_ante_lmp.json"),
    )])
    .await;
    let p = c.ex_ante_lmp().await.expect("decode");
    assert_eq!(p.hubs.len(), 9);
    let hub = p.hub("ARKANSAS.HUB").expect("hub present");
    assert_eq!((hub.lmp, hub.loss, hub.congestion), (-21.81, -0.53, -41.41));
    assert_eq!(
        p.interval_utc().expect("interval").to_rfc3339(),
        "2026-11-18T06:10:00+00:00"
    );
}

#[tokio::test]
async fn consolidated_table_decodes_all_four_blocks() {
    let (c, _s) = client_for(vec![(
        "/api/MarketPricing/GetLmpConsolidatedTable",
        ok("lmp_consolidated_table.json"),
    )])
    .await;
    let t = c.lmp_consolidated_table().await.expect("decode");
    let five = t.five_minute.node("NIPS.NIPS").expect("node");
    assert_eq!((five.lmp, five.mcc), (19.39, 10.57));
    assert_eq!(five.reg_mcp, Some(0.0));
    let hourly = t.hourly_integrated.node("NIPS.NIPS").expect("hourly node");
    assert!(
        hourly.reg_mcp.is_none(),
        "hourly blocks carry no ancillary prices"
    );
    assert!(t.day_ahead_ex_post.label.starts_with("HE"));
    assert!(t.day_ahead_ex_ante.node("NIPS.NIPS").is_some());
}

#[tokio::test]
async fn five_minute_expost_decodes_rows_and_utc() {
    let (c, _s) = client_for(vec![(
        "/api/MarketPricing/GetRealTimeFiveMinExPost/Current",
        ok("five_min_current.json"),
    )])
    .await;
    let rows = c
        .five_minute_ex_post_lmp(FiveMinuteWindow::Current)
        .await
        .expect("decode");
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].node, "AMIL.GRTPLAINS");
    assert_eq!(
        (rows[0].lmp, rows[0].mlc, rows[0].mcc),
        (14.24, -0.52, 5.65)
    );
    assert_eq!(
        rows[0].interval_utc().to_rfc3339(),
        "2026-11-18T06:05:00+00:00"
    );
}

#[tokio::test]
async fn ancillary_and_fuel_mix_and_load() {
    let (c, _s) = client_for(vec![
        (
            "/api/MarketPricing/GetAncillaryServicesMcp",
            ok("ancillary_mcp.json"),
        ),
        ("/api/FuelMix", ok("fuel_mix.json")),
        ("/api/FuelMix/Today", ok("fuel_mix_today.json")),
        ("/api/RealTimeTotalLoad", ok("total_load.json")),
    ])
    .await;
    let mcp = c.ancillary_services_mcp().await.expect("mcp");
    assert_eq!(mcp.zones.len(), 2);
    assert_eq!(mcp.zones[0].zone, 1);
    assert_eq!(mcp.market_day, "11-18-2026");

    let mix = c.fuel_mix(FuelMixView::Latest).await.expect("mix");
    assert_eq!(mix.total_mw, 81533.0);
    let wind = mix
        .entries
        .iter()
        .find(|e| e.category == "Wind")
        .expect("wind");
    assert_eq!(wind.mw, 22732.0);
    let negative = mix
        .entries
        .iter()
        .find(|e| e.category == "Imports")
        .expect("imports");
    assert_eq!(negative.mw, -893.0);
    let today = c.fuel_mix(FuelMixView::Today).await.expect("today");
    assert_ne!(today.latest().len(), 0, "latest interval has entries");

    let load = c.total_load().await.expect("load");
    assert_eq!(load.cleared.len(), 4);
    assert_eq!(
        (load.cleared[0].hour_ending, load.cleared[0].mw),
        (1, 74014.0)
    );
    assert_eq!(load.forecast[0].mw, 61309.0);
    assert_eq!(load.actual[1].time, "00:05");
}

#[tokio::test]
async fn wind_solar_interchange_and_misc() {
    let (c, _s) = client_for(vec![
        ("/api/WindSolar/getwind", ok("wind_forecast_vs_actual.json")),
        ("/api/WindSolar/getwindforecast", ok("wind_forecast.json")),
        ("/api/WindSolar/getsolaractual", ok("solar_actual.json")),
        ("/api/Interchange/GetNai", ok("nai.json")),
        ("/api/Interchange/GetNsi", ok("nsi.json")),
        ("/api/Snapshot", ok("snapshot.json")),
        (
            "/api/BindingConstraints/RealTime",
            ok("binding_constraints.json"),
        ),
        ("/api/Ace", ok("ace.json")),
    ])
    .await;
    let fva = c.forecast_vs_actual(Renewable::Wind).await.expect("fva");
    assert_eq!(fva.len(), 3);
    assert_eq!(
        (fva[0].forecast_mw, fva[0].actual_mw),
        (26886.33, Some(28833.20))
    );
    assert_eq!(fva[1].actual_mw, None, "future hours have no actual yet");
    assert_eq!(
        c.renewable_forecast(Renewable::Wind).await.expect("fc")[0].mw,
        26886.33
    );
    assert_eq!(
        c.renewable_actual(Renewable::Solar).await.expect("act")[0].mw,
        -53.98
    );
    assert_eq!(c.net_actual_interchange().await.expect("nai").mw, 1222.94);
    let nsi = c.net_scheduled_interchange().await.expect("nsi");
    assert_eq!(nsi.flows_mw["PJM"], 483.0);
    assert!(!nsi.flows_mw.contains_key("timestamp"));
    let snap = c.snapshot().await.expect("snapshot");
    assert_eq!(snap[1].title, "Current Demand (MW)");
    assert_eq!(snap[1].value, "70,688");
    let bc = c.binding_constraints().await.expect("bc");
    assert_eq!(bc.len(), 3);
    assert_eq!(bc[0].name, "BASE_ZPFTCH");
    assert_eq!(bc[0].price, Some(-40.9));
    assert_eq!(c.ace().await.expect("ace").len(), 3);
}

#[tokio::test]
async fn lmp_report_fetches_and_parses_by_date() {
    let (c, s) = client_for(vec![(
        "/marketreports/20261116_da_expost_lmp.csv",
        ok("da_expost_lmp_20261116.csv"),
    )])
    .await;
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("date");
    let report = c
        .lmp_report(LmpReportKind::DayAheadExPost, date)
        .await
        .expect("report");
    assert_eq!(report.market_date, date);
    assert_eq!(report.lmp("ARKANSAS.HUB", 1), Some(22.20));
    assert_eq!(
        report
            .series("ARKANSAS.HUB", Component::Mcc)
            .map(<[_]>::len),
        Some(24)
    );
    assert_eq!(report.lmp_rows(&NodeType::Hub).count(), 3);
    assert_eq!(
        s.requests()[0].target,
        "/marketreports/20261116_da_expost_lmp.csv"
    );
}

#[tokio::test]
async fn unpublished_report_is_a_404_status_error() {
    let (c, _s) = client_for(vec![]).await;
    let date = NaiveDate::from_ymd_opt(2026, 11, 17).expect("date");
    let err = c
        .lmp_report(LmpReportKind::RealTimeFinal, date)
        .await
        .expect_err("404");
    assert!(matches!(err, Error::Status { status: 404, .. }), "{err}");
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn legacy_data_broker_reports_no_data() {
    let (c, s) = client_for(vec![(
        "/MISORTWDDataBroker/DataBrokerServices.asmx",
        ok("data_broker_no_data.json"),
    )])
    .await;
    let err = c.data_broker("getfuelmix").await.expect_err("retired");
    assert!(matches!(err, Error::NoData { .. }), "{err}");
    assert!(s.requests()[0].target.contains("messageType=getfuelmix"));
}

#[cfg(feature = "miso-dataexchange")]
#[tokio::test]
async fn data_exchange_requires_a_key_and_sends_it_in_the_gateway_header() {
    let server = Server::start(vec![(
        "/pricing/v1/day-ahead/2026-11-16/lmp-expost",
        vec![
            Reply::ok(r#"{"data":[{"node":"A","lmp":1.5}],"page":{"pageNumber":0,"totalPages":2,"lastPage":false}}"#),
            Reply::ok(r#"{"data":[{"node":"B","lmp":2.5}],"page":{"pageNumber":1,"totalPages":2,"lastPage":true}}"#),
        ],
    )])
    .await;
    let eps = Endpoints::all_at(&server.base);
    let keyless = Client::builder()
        .endpoints(eps.clone())
        .build()
        .expect("client");
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("date");
    let q = gridapi_miso::DataExchangeQuery::new();
    let err = keyless
        .pricing_page(PricingProduct::DayAheadExPost, date, &q)
        .await
        .expect_err("no key");
    assert!(matches!(err, Error::MissingApiKey), "{err}");
    assert!(
        server.requests().is_empty(),
        "nothing is sent without a key"
    );

    let keyed = Client::builder()
        .endpoints(eps)
        .api_key("secret-key")
        .build()
        .expect("client");
    let rows = keyed
        .data_exchange_all_pages(&PricingProduct::DayAheadExPost.path(date), &q)
        .await
        .expect("pages");
    assert_eq!(rows.len(), 2);
    let seen = server.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0]
            .headers
            .get("ocp-apim-subscription-key")
            .map(String::as_str),
        Some("secret-key")
    );
    assert!(
        seen[1].target.contains("pageNumber=1"),
        "{}",
        seen[1].target
    );
}

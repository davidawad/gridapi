//! The file browser, price, load, generation mix and chart paths, served by an in-process server.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

mod common;

use chrono::NaiveDate;
use common::{fixture, Reply, Server};
use gridapi_spp::{
    at_location, BalancingArea, Chart, Client, EntryKind, FileSource, GenMixWindow, RetryPolicy,
    DA_LMP_BY_LOCATION,
};

async fn client_for(routes: Vec<(&str, Vec<Reply>)>) -> (Client, Server) {
    let server = Server::start(routes).await;
    let client = Client::builder()
        .base_url(server.base.clone())
        .retry(RetryPolicy::none())
        .build()
        .expect("client");
    (client, server)
}

fn ok(name: &str) -> Vec<Reply> {
    vec![Reply::ok(fixture(name))]
}

const DOWNLOAD_DA: &str = "/file-browser-api/download/da-lmp-by-settlement-location";
const DOWNLOAD_RT: &str = "/file-browser-api/download/rtbm-lmp-by-location";

#[tokio::test]
async fn da_lmp_downloads_the_dated_file_and_decodes_hubs() {
    let (c, s) = client_for(vec![(DOWNLOAD_DA, ok("da_lmp_by_location.csv"))]).await;
    let date = NaiveDate::from_ymd_opt(2026, 11, 18).expect("date");
    let rows = c.da_lmp_by_location(date).await.expect("da");
    let target = &s.requests()[0].target;
    assert!(
        target.contains("path=%2F2026%2F11%2FBy_Day%2FDA-LMP-SL-202611180100.csv")
            || target.contains("path=/2026/11/By_Day/DA-LMP-SL-202611180100.csv"),
        "{target}"
    );
    let crsp: Vec<_> = at_location(&rows, "CRSP_HUB").collect();
    assert_eq!(crsp.len(), 3, "three hours in the fixture");
    assert_eq!(crsp[0].lmp, 17.4286);
    assert_eq!(crsp[0].balancing_authority, "SWPW");
    assert!(rows.iter().filter(|r| r.is_hub()).count() >= 6);
    assert!(
        !s.requests()[0].headers.contains_key("x-spp-ui-token"),
        "anonymous: no UI token"
    );
}

#[tokio::test]
async fn rt_lmp_decodes_the_five_minute_layout() {
    let (c, s) = client_for(vec![(DOWNLOAD_RT, ok("rt_lmp_by_location.csv"))]).await;
    let t = NaiveDate::from_ymd_opt(2026, 11, 17)
        .expect("date")
        .and_hms_opt(23, 55, 0)
        .expect("time");
    let rows = c.rt_lmp_by_location(t).await.expect("rt");
    assert_ne!(rows.len(), 0, "no rt rows");
    assert!(rows.iter().all(|r| r.interval_end == t));
    assert!(
        s.requests()[0]
            .target
            .contains("RTBM-LMP-SL-202611172355.csv"),
        "{}",
        s.requests()[0].target
    );
}

#[tokio::test]
async fn missing_file_is_a_not_found_error() {
    let (c, _s) = client_for(vec![]).await;
    let date = NaiveDate::from_ymd_opt(2030, 1, 1).expect("date");
    let err = c.da_lmp_by_location(date).await.expect_err("404");
    assert!(err.is_not_found(), "{err}");
}

#[tokio::test]
async fn list_decodes_entries() {
    let (c, s) = client_for(vec![("/file-browser-api/", ok("list_years.json"))]).await;
    let entries = c.list(&DA_LMP_BY_LOCATION, "/").await.expect("list");
    assert_eq!(entries.len(), 3);
    assert!(entries.iter().all(|e| e.kind == EntryKind::Folder));
    let target = &s.requests()[0].target;
    assert!(
        target.contains("fsName=da-lmp-by-settlement-location"),
        "{target}"
    );
    assert!(target.contains("type=folder"), "{target}");
}

#[tokio::test]
async fn newest_file_descends_year_month_and_day() {
    let folder = |name: &str, path: &str| {
        format!(
            r#"{{"fsName":"s","items":null,"modified":null,"name":"{name}","path":"{path}","size":null,"type":"folder"}}"#
        )
    };
    let file = |name: &str, path: &str| {
        format!(
            r#"{{"fsName":"s","items":null,"modified":"11/17/2026 11:47:13 PM","name":"{name}","path":"{path}","size":9,"type":"file"}}"#
        )
    };
    let years = format!("[{},{}]", folder("2025", "/2025"), folder("2026", "/2026"));
    let months = format!(
        "[{},{}]",
        folder("10", "/2026/10"),
        folder("11", "/2026/11")
    );
    let days = format!(
        "[{},{}]",
        folder("16", "/2026/11/By_Interval/16"),
        folder("17", "/2026/11/By_Interval/17")
    );
    let files = format!(
        "[{},{}]",
        file(
            "RTBM-LMP-SL-202611172350.csv",
            "/2026/11/By_Interval/17/RTBM-LMP-SL-202611172350.csv"
        ),
        file(
            "RTBM-LMP-SL-202611172355.csv",
            "/2026/11/By_Interval/17/RTBM-LMP-SL-202611172355.csv"
        )
    );
    let (c, s) = client_for(vec![(
        "/file-browser-api/",
        vec![
            Reply::ok(years),
            Reply::ok(months),
            Reply::ok(days),
            Reply::ok(files),
        ],
    )])
    .await;
    let src = FileSource::new("rtbm-lmp-by-location");
    let newest = c.newest_file(&src, "By_Interval").await.expect("newest");
    assert_eq!(newest.name, "RTBM-LMP-SL-202611172355.csv");
    assert_eq!(s.requests().len(), 4);
}

/// Regression (): the live root of `da-lmp-by-settlement-location` lists a
/// `RePrice` folder after the years (listing captured 2026-11-18). `newest_file` took it for
/// the newest year and failed with "nothing found under `/RePrice/*/By_Day`".
#[tokio::test]
async fn newest_file_ignores_the_reprice_folder_beside_the_years() {
    let folder = |name: &str, path: &str| {
        format!(
            r#"{{"fsName":"s","items":null,"modified":null,"name":"{name}","path":"{path}","size":null,"type":"folder"}}"#
        )
    };
    let months = format!(
        "[{},{}]",
        folder("10", "/2026/10"),
        folder("11", "/2026/11")
    );
    let file = r#"[{"fsName":"s","items":null,"modified":"11/17/2026 11:47:13 PM","name":"DA-LMP-SL-202611180100.csv","path":"/2026/11/By_Day/DA-LMP-SL-202611180100.csv","size":9,"type":"file"}]"#;
    let (c, s) = client_for(vec![(
        "/file-browser-api/",
        vec![
            Reply::ok(fixture("list_root_with_reprice.json")),
            Reply::ok(months),
            Reply::ok(file),
        ],
    )])
    .await;
    let newest = c
        .newest_file(&DA_LMP_BY_LOCATION, "By_Day")
        .await
        .expect("newest, not RePrice");
    assert_eq!(newest.name, "DA-LMP-SL-202611180100.csv");
    let seen = s.requests();
    assert_eq!(seen.len(), 3);
    assert!(
        seen[1].target.contains("2026") && !seen[1].target.contains("RePrice"),
        "{}",
        seen[1].target
    );
}

#[tokio::test]
async fn newest_file_falls_back_to_the_previous_month_when_the_new_one_is_empty() {
    let folder = |name: &str, path: &str| {
        format!(
            r#"{{"fsName":"s","items":null,"modified":null,"name":"{name}","path":"{path}","size":null,"type":"folder"}}"#
        )
    };
    let years = format!("[{}]", folder("2026", "/2026"));
    let months = format!(
        "[{},{}]",
        folder("10", "/2026/10"),
        folder("11", "/2026/11")
    );
    let file = r#"[{"fsName":"s","items":null,"modified":null,"name":"DA-LMP-SL-202610310100.csv","path":"/2026/10/By_Day/DA-LMP-SL-202610310100.csv","size":9,"type":"file"}]"#;
    let (c, _s) = client_for(vec![(
        "/file-browser-api/",
        vec![
            Reply::ok(years),
            Reply::ok(months),
            Reply::ok("[]"), // 2026/11/By_Day is empty
            Reply::ok(file), // falls back to 2026/10/By_Day
        ],
    )])
    .await;
    let newest = c
        .newest_file(&FileSource::new("x"), "By_Day")
        .await
        .expect("fallback");
    assert_eq!(newest.name, "DA-LMP-SL-202610310100.csv");
}

#[tokio::test]
async fn menu_lists_file_browser_pages() {
    let (c, _s) = client_for(vec![("/api/menu/", ok("menu.json"))]).await;
    let pages = c.file_browser_pages().await.expect("menu");
    let slugs: Vec<&str> = pages.iter().map(|p| p.source.as_str()).collect();
    assert!(
        slugs.contains(&"da-lmp-by-settlement-location"),
        "{slugs:?}"
    );
    assert!(slugs.contains(&"rtbm-lmp-by-location"), "{slugs:?}");
    let da = pages
        .iter()
        .find(|p| p.source.as_str() == "da-lmp-by-settlement-location")
        .expect("da page");
    assert_eq!(da.title, "LMP By Settlement Location");
    assert!(
        da.menu_path.contains("Day-Ahead Market"),
        "{}",
        da.menu_path
    );
}

#[tokio::test]
async fn load_generation_mix_and_chart_decode() {
    let (c, _s) = client_for(vec![
        (
            "/file-browser-api/download/hourly-load",
            ok("hourly_load_daily.csv"),
        ),
        (
            "/file-browser-api/download/generation-mix-historical",
            ok("genmix_2hour_spp.csv"),
        ),
        (
            "/chart-api/load-forecast/asChart",
            ok("chart_load_forecast.json"),
        ),
    ])
    .await;
    let date = NaiveDate::from_ymd_opt(2026, 1, 5).expect("date");
    let load = c.hourly_load_daily(date).await.expect("load");
    assert!(load.iter().any(|z| z.zone == "CSWS" && z.mw > 638.0));

    let mix = c
        .generation_mix(BalancingArea::Spp, GenMixWindow::TwoHour)
        .await
        .expect("mix");
    assert_eq!(mix.len(), 5);
    let wind = mix[0].fuel("Wind").expect("wind");
    assert!(wind.total_mw() > 10_000.0, "{wind:?}");
    assert!(mix[0].load_mw.expect("load") > 20_000.0);

    let chart = c.chart(Chart::LoadForecast).await.expect("chart");
    assert_eq!(chart.times.len(), 4);
    assert!(chart.latest("Actual Load").is_some());
}

//! Client behaviour against a scripted local server: pacing, retries, chunking, redirects.

// Tests return `Result` so `?` can propagate, and assert inside them.
#![allow(
    clippy::panic_in_result_fn,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod support;

use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use gridapi_caiso::{
    AncillaryType, Client, Error, LmpComponent, MarketRun, OutlookDataset, Query, QueryName,
    RetryPolicy, TradingHub,
};
use support::{fixture, serve, zip_of, Canned, MockServer, TestResult};

fn utc(d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, d, h, 0, 0)
        .single()
        .unwrap_or_default()
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 4,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(20),
    }
}

fn client_for(server: &MockServer) -> Result<Client, Error> {
    Client::builder()
        .oasis_base_url(format!("{}/oasisapi", server.url()))
        .outlook_base_url(format!("{}/outlook", server.url()))
        .min_interval(Duration::ZERO)
        .retry(fast_retry())
        .request_timeout(Duration::from_secs(5))
        .user_agent("gridapi-caiso-test/0.0")
        .build()
}

fn csv_zip(body: &[u8]) -> Result<Canned, Box<dyn std::error::Error>> {
    Ok(Canned::ok(zip_of(&[("data.csv", body)])?))
}

fn error_zip(file: &str) -> Result<Canned, Box<dyn std::error::Error>> {
    Ok(Canned::ok(zip_of(&[("err.xml", &fixture(file)?)])?))
}

#[tokio::test]
async fn day_ahead_lmp_sends_documented_parameters_and_parses() -> TestResult {
    let server = serve(vec![csv_zip(&fixture("prc_lmp_dam.csv")?)?]).await?;
    let client = client_for(&server)?;
    let rows = client
        .day_ahead_lmp(&TradingHub::all_node_ids(), utc(10, 8), utc(10, 9))
        .await?;
    assert_eq!(rows.len(), 15);

    let head = server.requests().remove(0);
    assert!(head.starts_with("GET /oasisapi/SingleZip?"), "{head}");
    assert!(head
        .to_lowercase()
        .contains("user-agent: gridapi-caiso-test/0.0"));
    for (key, want) in [
        ("queryname", "PRC_LMP"),
        ("startdatetime", "20261110T08:00-0000"),
        ("enddatetime", "20261110T09:00-0000"),
        ("version", "12"),
        ("market_run_id", "DAM"),
        ("node", "TH_NP15_GEN-APND,TH_SP15_GEN-APND,TH_ZP26_GEN-APND"),
        ("resultformat", "6"),
    ] {
        assert_eq!(
            server.query_param(0, key).as_deref(),
            Some(want),
            "param {key}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn real_time_and_rtpd_use_their_own_report_versions_and_runs() -> TestResult {
    let server = serve(vec![
        csv_zip(&fixture("prc_intvl_lmp_rtm.csv")?)?,
        csv_zip(&fixture("prc_rtpd_lmp.csv")?)?,
    ])
    .await?;
    let client = client_for(&server)?;
    let rtm = client
        .real_time_lmp(&[TradingHub::Np15.node_id()], utc(10, 8), utc(10, 9))
        .await?;
    let rtpd = client
        .rtpd_lmp(&[TradingHub::Np15.node_id()], utc(10, 8), utc(10, 9))
        .await?;
    assert!(rtm
        .iter()
        .all(|r| r.market_run_id.as_deref() == Some("RTM")));
    assert!(rtpd.iter().any(|r| r.component == LmpComponent::Lmp));
    assert_eq!(
        server.query_param(0, "queryname").as_deref(),
        Some("PRC_INTVL_LMP")
    );
    assert_eq!(server.query_param(0, "version").as_deref(), Some("3"));
    assert_eq!(
        server.query_param(0, "market_run_id").as_deref(),
        Some("RTM")
    );
    assert_eq!(
        server.query_param(1, "queryname").as_deref(),
        Some("PRC_RTPD_LMP")
    );
    assert_eq!(
        server.query_param(1, "market_run_id").as_deref(),
        Some("RTPD")
    );
    Ok(())
}

#[tokio::test]
async fn load_schedule_and_node_helpers_hit_the_right_reports() -> TestResult {
    let server = serve(vec![
        csv_zip(&fixture("sld_fcst.csv")?)?,
        csv_zip(&fixture("ene_slrs.csv")?)?,
        csv_zip(&fixture("atl_pnode.csv")?)?,
        csv_zip(&fixture("atl_apnode.csv")?)?,
    ])
    .await?;
    let client = client_for(&server)?;
    let load = client
        .load_forecast(MarketRun::Dam, Some("CA ISO-TAC"), utc(10, 8), utc(10, 10))
        .await?;
    let sched = client
        .system_schedules(MarketRun::Dam, None, utc(10, 8), utc(10, 10))
        .await?;
    let pn = client.pnodes(utc(10, 8)).await?;
    let apn = client.apnodes(utc(10, 8)).await?;
    assert!(!load.is_empty() && !sched.is_empty());
    assert_eq!((pn.len(), apn.len()), (4, 4));
    let names: Vec<_> = (0..4)
        .map(|i| server.query_param(i, "queryname").unwrap_or_default())
        .collect();
    assert_eq!(names, ["SLD_FCST", "ENE_SLRS", "ATL_PNODE", "ATL_APNODE"]);
    assert!(
        server.requests()[0].contains("tac_area_name=CA+ISO-TAC"),
        "{}",
        server.requests()[0]
    );
    assert!(!server.requests()[1].contains("tac_zone_name"));
    Ok(())
}

#[tokio::test]
async fn oasis_error_in_a_200_response_is_not_retried() -> TestResult {
    let server = serve(vec![error_zip("error_no_data.xml")?]).await?;
    let client = client_for(&server)?;
    let err = client
        .day_ahead_lmp(&["NOT_A_NODE"], utc(10, 8), utc(10, 9))
        .await
        .err()
        .ok_or("expected an error")?;
    assert!(err.is_no_data(), "{err}");
    assert_eq!(server.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn http_429_then_success_is_retried() -> TestResult {
    let server = serve(vec![
        Canned::status(
            429,
            "CAISO Acceptable Use Policy Violation. Please retry your request after 5 seconds.",
        ),
        csv_zip(&fixture("prc_lmp_dam.csv")?)?,
    ])
    .await?;
    let client = client_for(&server)?;
    let rows = client
        .day_ahead_lmp(&[TradingHub::Np15.node_id()], utc(10, 8), utc(10, 9))
        .await?;
    assert_eq!(rows.len(), 15);
    assert_eq!(server.requests().len(), 2);
    Ok(())
}

#[tokio::test]
async fn server_errors_and_html_bodies_are_retried_until_attempts_run_out() -> TestResult {
    let server = serve(vec![Canned::status(503, "busy")]).await?;
    let client = client_for(&server)?;
    let err = client
        .day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9))
        .await
        .err()
        .ok_or("expected failure")?;
    assert!(matches!(err, Error::Status { status: 503, .. }), "{err}");
    assert_eq!(server.requests().len(), 4, "max_attempts");

    let html = serve(vec![
        Canned::ok(b"<html>maintenance</html>".to_vec()),
        csv_zip(&fixture("prc_lmp_dam.csv")?)?,
    ])
    .await?;
    let rows = client_for(&html)?
        .day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9))
        .await?;
    assert_eq!(rows.len(), 15);
    Ok(())
}

#[tokio::test]
async fn client_errors_are_not_retried() -> TestResult {
    let server = serve(vec![Canned::status(404, "nope")]).await?;
    let err = client_for(&server)?
        .day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9))
        .await
        .err()
        .ok_or("expected failure")?;
    assert!(matches!(err, Error::Status { status: 404, .. }));
    assert_eq!(server.requests().len(), 1);
    Ok(())
}

#[tokio::test]
async fn slow_responses_time_out_and_are_retried() -> TestResult {
    let server = serve(vec![
        csv_zip(&fixture("prc_lmp_dam.csv")?)?.delayed(Duration::from_millis(600)),
        csv_zip(&fixture("prc_lmp_dam.csv")?)?,
    ])
    .await?;
    let client = Client::builder()
        .oasis_base_url(format!("{}/oasisapi", server.url()))
        .min_interval(Duration::ZERO)
        .retry(fast_retry())
        .request_timeout(Duration::from_millis(200))
        .build()?;
    let rows = client.day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9)).await?;
    assert_eq!(rows.len(), 15);
    assert_eq!(server.requests().len(), 2);
    Ok(())
}

#[tokio::test]
async fn single_fetch_rejects_windows_over_31_days_without_sending() -> TestResult {
    let server = serve(vec![Canned::status(500, "unreachable")]).await?;
    let client = client_for(&server)?;
    let query = Query::builder(
        QueryName::PrcLmp,
        utc(1, 0),
        utc(1, 0) + TimeDelta::days(32),
    )
    .market_run(MarketRun::Dam)
    .node("N")
    .build()?;
    let err = client
        .fetch_records(&query)
        .await
        .err()
        .ok_or("expected error")?;
    assert!(matches!(err, Error::InvalidQuery(_)), "{err}");
    assert!(server.requests().is_empty());
    Ok(())
}

#[tokio::test]
async fn long_ranges_are_chunked_contiguously() -> TestResult {
    let server = serve(vec![csv_zip(&fixture("prc_lmp_dam.csv")?)?]).await?;
    let client = client_for(&server)?;
    let start = utc(1, 0);
    let end = start + TimeDelta::days(70);
    let rows = client.day_ahead_lmp(&["N"], start, end).await?;
    assert_eq!(rows.len(), 45, "3 chunks x 15 rows");
    assert_eq!(server.requests().len(), 3);

    let bounds: Vec<(String, String)> = (0..3)
        .map(|i| {
            (
                server.query_param(i, "startdatetime").unwrap_or_default(),
                server.query_param(i, "enddatetime").unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(bounds[0].0, "20261101T00:00-0000");
    assert_eq!(bounds[0].1, bounds[1].0);
    assert_eq!(bounds[1].1, bounds[2].0);
    assert_eq!(bounds[2].1, "20270110T00:00-0000");
    Ok(())
}

#[tokio::test]
async fn empty_chunks_are_skipped_but_all_empty_is_an_error() -> TestResult {
    let some = serve(vec![
        csv_zip(&fixture("prc_lmp_dam.csv")?)?,
        error_zip("error_no_data.xml")?,
    ])
    .await?;
    let rows = client_for(&some)?
        .day_ahead_lmp(&["N"], utc(1, 0), utc(1, 0) + TimeDelta::days(40))
        .await?;
    assert_eq!(rows.len(), 15, "second chunk had no data");

    let none = serve(vec![error_zip("error_no_data.xml")?]).await?;
    let err = client_for(&none)?
        .day_ahead_lmp(&["N"], utc(1, 0), utc(1, 0) + TimeDelta::days(40))
        .await
        .err()
        .ok_or("expected error")?;
    assert!(err.is_no_data());
    Ok(())
}

#[tokio::test]
async fn empty_node_list_is_rejected_locally() -> TestResult {
    let server = serve(vec![Canned::status(500, "unreachable")]).await?;
    let err = client_for(&server)?
        .day_ahead_lmp(&[], utc(10, 8), utc(10, 9))
        .await
        .err()
        .ok_or("expected error")?;
    assert!(matches!(err, Error::InvalidQuery(_)));
    assert!(server.requests().is_empty());
    Ok(())
}

#[tokio::test]
async fn requests_from_one_client_are_paced() -> TestResult {
    let server = serve(vec![csv_zip(&fixture("prc_lmp_dam.csv")?)?]).await?;
    let client = Client::builder()
        .oasis_base_url(format!("{}/oasisapi", server.url()))
        .min_interval(Duration::from_millis(300))
        .retry(fast_retry())
        .build()?;
    let begin = Instant::now();
    for _ in 0..3 {
        client.day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9)).await?;
    }
    assert!(
        begin.elapsed() >= Duration::from_millis(600),
        "3 requests at 300ms spacing took {:?}",
        begin.elapsed()
    );
    Ok(())
}

#[tokio::test]
async fn all_region_as_prices_follow_the_group_redirect_day_by_day() -> TestResult {
    // SingleZip redirects to GroupZip, which returns the whole Pacific day.
    let day = fixture("prc_as.csv")?;
    let server = serve(vec![
        Canned::status(302, "").header("Location", "/oasisapi/GroupZip?groupid=DAM_PRC_AS_GRP&startdatetime=20261110T08:00-0000&resultformat=6&version=1"),
        csv_zip(&day)?,
    ])
    .await?;
    let client = client_for(&server)?;
    // Window selects only the 08:00Z hour; the group file's 09:00Z row is filtered out.
    let rows = client
        .ancillary_prices(
            MarketRun::Dam,
            AncillaryType::All,
            "ALL",
            utc(10, 8),
            utc(10, 9),
        )
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].interval_start, utc(10, 8));
    let reqs = server.requests();
    assert!(reqs[0].starts_with("GET /oasisapi/SingleZip?"));
    assert!(
        reqs[1].starts_with("GET /oasisapi/GroupZip?"),
        "{}",
        reqs[1]
    );
    assert_eq!(server.query_param(0, "anc_type").as_deref(), Some("ALL"));
    assert_eq!(server.query_param(0, "anc_region").as_deref(), Some("ALL"));
    Ok(())
}

#[tokio::test]
async fn specific_as_product_is_a_single_request() -> TestResult {
    let server = serve(vec![csv_zip(&fixture("prc_as.csv")?)?]).await?;
    let rows = client_for(&server)?
        .ancillary_prices(
            MarketRun::Dam,
            AncillaryType::RegulationUp,
            "AS_CAISO_EXP",
            utc(10, 8),
            utc(10, 10),
        )
        .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(server.requests().len(), 1);
    assert_eq!(server.query_param(0, "anc_type").as_deref(), Some("RU"));
    Ok(())
}

#[tokio::test]
async fn outlook_current_is_dated_from_last_modified() -> TestResult {
    // 2026-11-18 06:03:36 GMT is 22:03 PST on 2026-11-17.
    let server = serve(vec![Canned::ok(fixture("outlook_fuelsource.csv")?)
        .header("Last-Modified", "Wed, 18 Nov 2026 06:03:36 GMT")])
    .await?;
    let series = client_for(&server)?
        .outlook(OutlookDataset::FuelSource)
        .await?;
    assert_eq!(
        series.date,
        NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("date")?
    );
    assert_eq!(series.points[0].time_utc, Some(utc(17, 8)));
    assert!(server.requests()[0].starts_with("GET /outlook/current/fuelsource.csv"));
    Ok(())
}

#[tokio::test]
async fn outlook_history_uses_the_dated_path() -> TestResult {
    let server = serve(vec![Canned::ok(fixture("outlook_demand.csv")?)]).await?;
    let date = NaiveDate::from_ymd_opt(2026, 11, 10).ok_or("date")?;
    let series = client_for(&server)?
        .outlook_on(OutlookDataset::Demand, date)
        .await?;
    assert_eq!(series.date, date);
    assert!(server.requests()[0].starts_with("GET /outlook/history/20261110/demand.csv"));
    Ok(())
}

#[tokio::test]
async fn oversized_responses_are_refused() -> TestResult {
    let server = serve(vec![csv_zip(&fixture("prc_lmp_dam.csv")?)?]).await?;
    let client = Client::builder()
        .oasis_base_url(format!("{}/oasisapi", server.url()))
        .min_interval(Duration::ZERO)
        .max_response_bytes(100)
        .retry(RetryPolicy::none())
        .build()?;
    let err = client
        .day_ahead_lmp(&["N"], utc(10, 8), utc(10, 9))
        .await
        .err()
        .ok_or("expected error")?;
    assert!(
        matches!(err, Error::ResponseTooLarge { limit: 100 }),
        "{err}"
    );
    Ok(())
}

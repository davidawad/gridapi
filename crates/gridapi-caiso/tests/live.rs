//! Live checks against oasis.caiso.com and caiso.com. Run explicitly:
//! `cargo test -p gridapi-caiso --test live -- --ignored --nocapture`.
//!
//! One client is shared by all OASIS requests so they are paced together; the
//! whole suite takes about a minute because OASIS allows one request per 5 s.
//!
//! NEVER run these in public CI: OASIS allows one request per 5 s and its terms bar load or benchmarking use. Use your own credentials on your own machine.

// Tests return `Result` so `?` can propagate, and assert inside them.
#![allow(
    clippy::panic_in_result_fn,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use gridapi_caiso::{
    pivot_lmp, AncillaryType, Client, LmpComponent, MarketRun, OutlookDataset, TradingHub,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// 2026-11-10 00:00 PST, a settled past day.
fn day_start() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 10, 8, 0, 0)
        .single()
        .unwrap_or_default()
}

fn live_client() -> Result<Client, gridapi_caiso::Error> {
    Client::builder()
        .user_agent("gridapi-caiso-live-test/0.1 (https://example.invalid/gridapi-caiso)")
        .request_timeout(Duration::from_secs(240))
        .build()
}

#[tokio::test]
#[ignore = "live network: oasis.caiso.com"]
async fn oasis_live_suite() -> TestResult {
    let client = live_client()?;
    let (start, end) = (day_start(), day_start() + TimeDelta::days(1));
    let hubs = TradingHub::all_node_ids();

    // Day-ahead hub LMPs: 24 hours x 3 hubs.
    let dam = client.day_ahead_lmp(&hubs, start, end).await?;
    let points = pivot_lmp(dam.clone());
    println!("DAM rows={} points={}", dam.len(), points.len());
    assert_eq!(points.len(), 72);
    let np15_h1 = points
        .iter()
        .find(|p| p.node == "TH_NP15_GEN-APND" && p.interval_start == start)
        .ok_or("no NP15 hour 1")?;
    println!(
        "DAM NP15 {} lmp={:?} energy={:?} cong={:?} loss={:?} ghg={:?}",
        np15_h1.interval_start,
        np15_h1.lmp,
        np15_h1.energy,
        np15_h1.congestion,
        np15_h1.loss,
        np15_h1.ghg
    );
    assert!(points
        .iter()
        .all(|p| p.lmp.is_some_and(|v| (-1000.0..=3000.0).contains(&v))));
    assert!(dam.iter().any(|r| r.component == LmpComponent::Ghg));

    // Real-time 5-minute LMPs for the first hour of that day.
    let rtm = client
        .real_time_lmp(
            &[TradingHub::Np15.node_id()],
            start,
            start + TimeDelta::hours(1),
        )
        .await?;
    let rtm_lmp: Vec<_> = rtm
        .iter()
        .filter(|r| r.component == LmpComponent::Lmp)
        .collect();
    println!(
        "RTM rows={} lmp-intervals={} first={:?}",
        rtm.len(),
        rtm_lmp.len(),
        rtm_lmp.first().map(|r| r.price)
    );
    assert_eq!(rtm_lmp.len(), 12);

    // 15-minute pre-dispatch.
    let rtpd = client
        .rtpd_lmp(
            &[TradingHub::Sp15.node_id()],
            start,
            start + TimeDelta::hours(2),
        )
        .await?;
    let rtpd_lmp: Vec<_> = rtpd
        .iter()
        .filter(|r| r.component == LmpComponent::Lmp)
        .collect();
    println!(
        "RTPD rows={} lmp-intervals={} first={:?}",
        rtpd.len(),
        rtpd_lmp.len(),
        rtpd_lmp.first().map(|r| r.price)
    );
    assert_eq!(rtpd_lmp.len(), 8);

    // Load forecast, system total.
    let load = client
        .load_forecast(MarketRun::Dam, Some("CA ISO-TAC"), start, end)
        .await?;
    println!(
        "SLD_FCST CA ISO-TAC rows={} peak={:?}",
        load.len(),
        load.iter().map(|r| r.mw).fold(f64::MIN, f64::max)
    );
    assert_eq!(load.len(), 24);
    assert!(load.iter().all(|r| r.mw > 10_000.0 && r.mw < 60_000.0));

    // System load and resource schedules.
    let slrs = client
        .system_schedules(MarketRun::Dam, None, start, start + TimeDelta::hours(2))
        .await?;
    let gen_mw = slrs
        .iter()
        .find(|r| r.data_item == "ISO_TOT_GEN_MW" && r.interval_start == start)
        .map(|r| r.mw);
    println!("ENE_SLRS rows={} ISO_TOT_GEN_MW[h1]={gen_mw:?}", slrs.len());
    assert!(gen_mw.is_some_and(|v| v > 5_000.0));

    // Ancillary prices: one product/region, then the grouped (redirect) form.
    let ru = client
        .ancillary_prices(
            MarketRun::Dam,
            AncillaryType::RegulationUp,
            "AS_CAISO_EXP",
            start,
            end,
        )
        .await?;
    println!(
        "PRC_AS RU AS_CAISO_EXP rows={} first={:?}",
        ru.len(),
        ru.first().map(|r| r.price)
    );
    assert_eq!(ru.len(), 24);
    let all_as = client
        .ancillary_prices(MarketRun::Dam, AncillaryType::All, "ALL", start, end)
        .await?;
    println!("PRC_AS ALL rows={}", all_as.len());
    assert!(all_as.len() > 24);
    assert!(all_as
        .iter()
        .all(|r| r.interval_start >= start && r.interval_start < end));

    // Node lists.
    let ap = client.apnodes(start).await?;
    let found: Vec<_> = hubs
        .iter()
        .filter(|h| ap.iter().any(|n| n.id == **h))
        .collect();
    println!("ATL_APNODE rows={} hubs-found={found:?}", ap.len());
    assert_eq!(found.len(), 3);
    let pn = client.pnodes(start).await?;
    println!("ATL_PNODE rows={}", pn.len());
    assert!(pn.len() > 1000);

    // OASIS error documents: an unknown node is "no data" (HTTP 200 + error zip).
    let err = client
        .day_ahead_lmp(&["NOT_A_NODE"], start, start + TimeDelta::hours(1))
        .await
        .err()
        .ok_or("unknown node should fail")?;
    println!("unknown node -> {err}");
    assert!(err.is_no_data());

    // Chunking across the 31-day limit: 45 days of NP15 = 2 windows.
    let long_end = start + TimeDelta::days(45);
    let long = client
        .day_ahead_lmp(&[TradingHub::Np15.node_id()], start, long_end)
        .await?;
    let long_points = pivot_lmp(long);
    println!("DAM NP15 45 days points={}", long_points.len());
    assert_eq!(long_points.len(), 45 * 24);
    Ok(())
}

#[tokio::test]
#[ignore = "live network: www.caiso.com"]
async fn outlook_live_suite() -> TestResult {
    let client = live_client()?;
    for dataset in [
        OutlookDataset::FuelSource,
        OutlookDataset::Demand,
        OutlookDataset::NetDemand,
        OutlookDataset::Storage,
        OutlookDataset::Co2,
    ] {
        let series = client.outlook(dataset).await?;
        let latest = series.latest().ok_or("no measured rows")?;
        println!(
            "outlook {:?} date={} cols={} rows={} latest={} values={:?}",
            dataset,
            series.date,
            series.columns.len(),
            series.points.len(),
            latest.local_time,
            latest.values
        );
        let measured = series
            .points
            .iter()
            .filter(|p| p.values.iter().any(Option::is_some))
            .count();
        assert!(
            measured >= 12,
            "{dataset:?} has only {measured} measured rows"
        );
    }
    let fuel = client.outlook(OutlookDataset::FuelSource).await?;
    let total: f64 = fuel
        .latest()
        .map(|p| p.values.iter().flatten().sum())
        .unwrap_or_default();
    assert!(total > 5_000.0, "total generation {total} MW");

    let demand = client.outlook(OutlookDataset::Demand).await?;
    let now = demand
        .latest_in("Current demand")
        .ok_or("no measured demand")?;
    println!(
        "outlook Current demand {:?} MW at {}",
        demand.value(now, "Current demand"),
        now.local_time
    );
    assert!(demand
        .value(now, "Current demand")
        .is_some_and(|mw| mw > 10_000.0));

    let past = NaiveDate::from_ymd_opt(2026, 11, 10).ok_or("date")?;
    let hist = client.outlook_on(OutlookDataset::Demand, past).await?;
    println!("outlook history {past} demand rows={}", hist.points.len());
    assert!(
        hist.points
            .iter()
            .filter(|p| hist.value(p, "Current demand").is_some())
            .count()
            > 250
    );
    Ok(())
}

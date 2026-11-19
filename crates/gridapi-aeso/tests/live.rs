//! Live checks against <https://apimgw.aeso.ca/public>. Need `AESO_API_KEY`.
//!
//! Run with:
//! `AESO_API_KEY=... cargo test -p gridapi-aeso --test live -- --ignored --nocapture`
//! Assertions use dates AESO has already settled, so they stay true.
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

use chrono::NaiveDate;
use gridapi_aeso::{
    merit_order_latest_date, AssetListQuery, Client, DateRange, InterchangeQuery, Intertie,
    OutageQuery,
};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
}

fn client() -> Client {
    Client::from_env().expect("set AESO_API_KEY to run the live tests")
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_pool_price_midnight_2026_11_16_is_0_92() {
    let rows = client()
        .pool_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("fetch");
    println!(
        "pool price {} MPT = {:?} CAD/MWh (forecast {:?}, 30d avg {:?}); {} hours",
        rows[0].begin_mpt,
        rows[0].pool_price,
        rows[0].forecast_pool_price,
        rows[0].rolling_30day_average,
        rows.len()
    );
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0].pool_price, Some(0.92));
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_system_marginal_price_and_load() {
    let cl = client();
    let smp = cl
        .system_marginal_price(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("smp");
    println!(
        "SMP intervals on 2026-11-16: {}; first = {:?}",
        smp.len(),
        smp.first().and_then(|s| s.system_marginal_price)
    );
    assert!(!smp.is_empty());
    let cur = cl
        .system_marginal_price_current()
        .await
        .expect("current smp");
    println!(
        "current SMP: {:?}",
        cur.first().map(|s| (s.begin_mpt, s.system_marginal_price))
    );
    assert_eq!(cur.len(), 1);
    let load = cl
        .alberta_internal_load(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("load");
    println!(
        "AIL 2026-11-16 00:00 MPT = {:?} MW (forecast {:?})",
        load[0].alberta_internal_load, load[0].forecast_alberta_internal_load
    );
    assert!(load[0].alberta_internal_load.is_some());
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_current_supply_demand_and_assets() {
    let cl = client();
    let s = cl.current_supply_demand().await.expect("csd");
    println!(
        "CSD {}: AIL {:?} MW, net gen {:?} MW, net interchange {:?} MW, fuels {}, paths {:?}",
        s.effective_mpt,
        s.alberta_internal_load,
        s.total_net_generation,
        s.net_actual_interchange,
        s.generation_data_list.len(),
        s.interchange_list
            .iter()
            .map(|f| (&f.path, f.actual_flow))
            .collect::<Vec<_>>()
    );
    assert!(s.alberta_internal_load.is_some_and(|l| l > 5000.0));
    assert!(s.fuel("WIND").is_some());
    let a = cl.generation_assets_current().await.expect("assets");
    println!(
        "CSD asset list: {} assets, gas net gen {:.0} MW",
        a.asset_list.len(),
        a.net_generation_by_fuel("GAS")
    );
    assert!(a.asset_list.len() > 100);
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_interties() {
    let cl = client();
    let q = InterchangeQuery::new(DateRange::day(d(2026, 11, 16)))
        .hours(1, 3)
        .expect("hours");
    let r = cl.interchange_capability(&q).await.expect("interchange");
    let bc =
        r.bc.as_ref()
            .and_then(|a| a.allocations.first())
            .and_then(|a| a.import.as_ref());
    println!(
        "BC import HE1 2026-11-16: TTC {:?} ATC {:?}; allocations {}",
        bc.and_then(|t| t.ttc),
        bc.and_then(|t| t.atc),
        r.all().count()
    );
    assert_eq!(bc.and_then(|t| t.ttc), Some(800.0));
    let outages = cl
        .intertie_outages(
            &OutageQuery::new(DateRange::new(d(2026, 9, 1), d(2026, 11, 17)).expect("range"))
                .intertie(Intertie::Bc)
                .expect("bc"),
        )
        .await
        .expect("outages");
    println!(
        "BC-affecting intertie outages 2026-09-01..2026-11-17: {}",
        outages.len()
    );
    assert!(!outages.is_empty());
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_merit_order_and_its_sixty_day_lag() {
    let cl = client();
    let hours = cl
        .energy_merit_order(d(2026, 9, 1))
        .await
        .expect("merit order");
    let marginal = hours[0].marginal_block();
    println!(
        "merit order 2026-09-01 00:00 MPT: {} blocks, marginal dispatched block {:?}",
        hours[0].energy_blocks.len(),
        marginal.map(|b| (&b.asset_id, b.block_price))
    );
    assert_eq!(hours.len(), 24);
    println!(
        "latest published merit-order date relative to 2026-11-18: {}",
        merit_order_latest_date(d(2026, 11, 18))
    );
    let err = cl
        .energy_merit_order(d(2026, 11, 10))
        .await
        .expect_err("inside the lag window");
    println!("merit order inside the lag: {err}");
    assert!(err.is_no_data());
}

#[tokio::test]
#[ignore = "network + AESO_API_KEY"]
async fn live_capacity_outages_assets_and_participants() {
    let cl = client();
    let cap = cl
        .aies_gen_capacity(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("capacity");
    println!(
        "AIES capacity: {} fuel groups; first {:?}/{:?} MC {:?}",
        cap.len(),
        cap[0].fuel_type,
        cap[0].sub_fuel_type,
        cap[0].hours[0].outage_grouping.maximum_capability
    );
    assert!(cap.len() >= 5);
    let lo = cl
        .load_outage_forecast(DateRange::day(d(2026, 11, 16)))
        .await
        .expect("load outage");
    assert_eq!(lo.len(), 24);
    let a = cl
        .asset_list(&AssetListQuery::all().asset_id("AST1"))
        .await
        .expect("asset");
    println!(
        "asset AST1: {:?}",
        a.first().map(|x| (&x.asset_name, &x.pool_participant_name))
    );
    assert_eq!(a[0].asset_name, "AST1 Example");
    let p = cl.pool_participants().await.expect("participants");
    println!("pool participants: {}", p.len());
    assert!(p.len() > 100);
}

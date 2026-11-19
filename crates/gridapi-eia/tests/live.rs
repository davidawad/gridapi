//! Live tests against the real EIA API and www.eia.gov. All `#[ignore]`d:
//!
//! ```text
//! EIA_API_KEY=... cargo test -p gridapi-eia --test live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The key is read from `EIA_API_KEY` at runtime and never printed. Each test
//! prints the values it observed (the README "Verified" section is copied
//! from this output).
//!
//! NEVER run these in public CI: they use your own EIA key and the real EIA API. Use your own credentials on your own machine.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout,
    clippy::many_single_char_names
)]

use std::collections::BTreeSet;

use gridapi_eia::datasets::generators::{self, GeneratorCapacityRow};
use gridapi_eia::datasets::natural_gas::{self, PriceRow};
use gridapi_eia::datasets::retail::{self, RetailSalesRow, Sector};
use gridapi_eia::datasets::rto::{
    self, FuelType, FuelTypeRow, InterchangeRow, RegionDataRow, RegionMetric,
};
use gridapi_eia::{Client, Direction, Error};

fn client() -> Client {
    Client::from_env().expect("set EIA_API_KEY to run live tests")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[tokio::test]
#[ignore = "live network"]
async fn live_browse_routes() {
    let c = client();
    let root = c.root().await.expect("root");
    let ids: Vec<_> = root.routes.iter().map(|r| r.id.as_str()).collect();
    println!("[{}] root routes: {ids:?}", now());
    assert!(ids.contains(&"electricity") && ids.contains(&"natural-gas"));

    let rto = c.route("electricity/rto").await.expect("rto");
    let leaf: Vec<_> = rto.routes.iter().map(|r| r.id.as_str()).collect();
    println!("electricity/rto routes: {leaf:?}");
    assert!(leaf.contains(&"region-data") && leaf.contains(&"fuel-type-data"));

    let meta = c.route("electricity/rto/region-data").await.expect("meta");
    println!(
        "region-data: freq={:?} facets={:?} period {:?}..{:?}",
        meta.frequency
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        meta.facets
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        meta.start_period,
        meta.end_period
    );
    assert!(meta.is_dataset());

    let bas = c
        .facet_values("electricity/rto/region-data", "respondent")
        .await
        .expect("facets");
    println!("respondent facet values: {}", bas.len());
    assert!(bas.iter().any(|b| b.id == "ERCO") && bas.iter().any(|b| b.id == "PJM"));

    let under = c.datasets_under("electricity/rto").await.expect("walk");
    println!(
        "datasets under electricity/rto: {:?}",
        under.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(under.len(), 8);
}

#[tokio::test]
#[ignore = "live network"]
async fn live_invalid_key_is_a_typed_error() {
    let c = Client::new("not-a-real-key").expect("client");
    let err = c.root().await.expect_err("bad key");
    println!("invalid key -> {err}");
    assert!(
        matches!(&err, Error::Api { status: 403, code: Some(c), .. } if c == "API_KEY_INVALID")
    );
}

#[tokio::test]
#[ignore = "live network"]
async fn live_rto_region_fuel_interchange() {
    let c = client();

    let q = rto::region_data_for("ERCO", RegionMetric::Demand)
        .sort("period", Direction::Desc)
        .length(24);
    let page = c.data_page::<RegionDataRow>(&q).await.expect("region");
    let r = &page.rows[0];
    println!(
        "ERCO demand: latest {} = {:?} {} ({} rows in series; page of {})",
        r.period,
        r.value,
        r.value_units.as_deref().unwrap_or("?"),
        page.total,
        page.rows.len()
    );
    assert_eq!(page.rows.len(), 24);
    assert!(
        r.value.is_some_and(|v| v > 10_000.0),
        "ERCOT demand is tens of GW"
    );

    let q = rto::fuel_type_data_for("ERCO", Some(FuelType::NaturalGas))
        .sort("period", Direction::Desc)
        .length(3);
    let f = c.data_page::<FuelTypeRow>(&q).await.expect("fuel").rows;
    println!(
        "ERCO natural gas net gen: {:?}",
        f.iter().map(|x| (&x.period, x.value)).collect::<Vec<_>>()
    );
    assert!(f.iter().all(|x| x.fueltype == "NG" && x.value.is_some()));

    let q = rto::interchange_from("ERCO")
        .sort("period", Direction::Desc)
        .length(5);
    let i = c
        .data_page::<InterchangeRow>(&q)
        .await
        .expect("interchange")
        .rows;
    println!(
        "ERCO interchange: {:?}",
        i.iter()
            .map(|x| (&x.period, &x.toba, x.value))
            .collect::<Vec<_>>()
    );
    assert!(i.iter().all(|x| x.fromba == "ERCO"));
}

#[tokio::test]
#[ignore = "live network"]
async fn live_paging_past_the_5000_row_cap() {
    let c = client();
    // One full year of hourly ERCOT demand: 8760 rows = two pages.
    let q = rto::region_data_for("ERCO", RegionMetric::Demand)
        .start("2025-01-01T00")
        .end("2025-12-31T23");
    let probe = c
        .data_page::<RegionDataRow>(&q.clone().length(1))
        .await
        .expect("probe");
    let rows: Vec<RegionDataRow> = c.data_all(&q).await.expect("all");
    let periods: BTreeSet<_> = rows.iter().map(|r| r.period.clone()).collect();
    println!(
        "ERCO hourly demand 2025: total={} fetched={} unique periods={} first={} last={}",
        probe.total,
        rows.len(),
        periods.len(),
        rows.first().map_or("", |r| r.period.as_str()),
        rows.last().map_or("", |r| r.period.as_str())
    );
    assert!(rows.len() > 5000, "must have crossed the cap");
    assert_eq!(rows.len() as u64, probe.total, "every row fetched");
    assert_eq!(
        periods.len(),
        rows.len(),
        "no duplicates across page boundaries"
    );
    assert!(
        rows.windows(2).all(|w| w[0].period <= w[1].period),
        "ascending"
    );
}

#[tokio::test]
#[ignore = "live network"]
async fn live_generators_retail_gas() {
    let c = client();

    let q = generators::operating_generator_capacity()
        .facet("stateid", ["VA"])
        .facet("energy_source_code", ["NG"])
        .start("2026-08")
        .end("2026-08");
    let gens: Vec<GeneratorCapacityRow> = c.data_all(&q).await.expect("gens");
    let mw: f64 = gens.iter().filter_map(|g| g.nameplate_capacity_mw).sum();
    println!(
        "860M API 2026-08 VA natural gas: {} generators, {mw:.1} MW nameplate; first: {:?} {:?} {:?} MW",
        gens.len(),
        gens.first().and_then(|g| g.plant_name.as_deref()),
        gens.first().and_then(|g| g.generatorid.as_deref()),
        gens.first().and_then(|g| g.nameplate_capacity_mw)
    );
    assert!(!gens.is_empty() && mw > 1000.0);
    assert!(gens.iter().all(|g| g.period == "2026-08"));

    let rs = c
        .data_page::<RetailSalesRow>(
            &retail::monthly_for("TX", Sector::Commercial)
                .sort("period", Direction::Desc)
                .length(3),
        )
        .await
        .expect("retail")
        .rows;
    println!(
        "retail TX commercial: {:?}",
        rs.iter()
            .map(|r| (&r.period, r.price, r.sales))
            .collect::<Vec<_>>()
    );
    assert!(
        rs[0].price.is_some_and(|p| (3.0..40.0).contains(&p)),
        "cents/kWh"
    );

    let hh = c
        .data_page::<PriceRow>(
            &natural_gas::henry_hub_spot_daily()
                .sort("period", Direction::Desc)
                .length(5),
        )
        .await
        .expect("gas")
        .rows;
    println!(
        "Henry Hub spot: {:?}",
        hh.iter().map(|r| (&r.period, r.value)).collect::<Vec<_>>()
    );
    assert!(hh.iter().all(|r| r.series.as_deref() == Some("RNGWHHD")));
    assert!(
        hh[0].value.is_some_and(|v| (0.5..30.0).contains(&v)),
        "$/MMBtu"
    );
}

#[cfg(feature = "bulk")]
mod bulk {
    use super::{client, now};
    use gridapi_eia::bulk::eia860m::{self, Release, Sheet};
    use gridapi_eia::bulk::eia930::{
        BalanceReader, Dataset, HalfYear, InterchangeReader, SixMonthFile, SubregionReader,
    };

    #[tokio::test]
    #[ignore = "live network, downloads 14 MB"]
    async fn live_eia860m_workbook() {
        let c = client();
        let dir = tempfile::tempdir().expect("tmp");
        let dest = dir.path().join("860m.xlsx");
        let release = c
            .download_latest_eia860m(Release::current_utc(), &dest)
            .await
            .expect("download");
        let bytes = std::fs::metadata(&dest).expect("meta").len();
        println!("[{}] 860M latest release {release}, {bytes} bytes", now());

        let d2 = dest.clone();
        let (op, planned, retired, canceled) = tokio::task::spawn_blocking(move || {
            (
                eia860m::read_sheet(&d2, Sheet::Operating).expect("operating"),
                eia860m::read_sheet(&d2, Sheet::Planned).expect("planned"),
                eia860m::read_sheet(&d2, Sheet::Retired).expect("retired"),
                eia860m::read_sheet(&d2, Sheet::CanceledOrPostponed).expect("canceled"),
            )
        })
        .await
        .expect("join");
        let mw = |v: &[eia860m::GeneratorRow]| v.iter().filter_map(|g| g.nameplate_mw).sum::<f64>();
        println!(
            "860M sheets: operating={} ({:.0} MW) planned={} ({:.0} MW) retired={} canceled={}",
            op.len(),
            mw(&op),
            planned.len(),
            mw(&planned),
            retired.len(),
            canceled.len()
        );
        assert!(
            op.len() > 20_000
                && planned.len() > 500
                && retired.len() > 3000
                && canceled.len() > 500
        );
        assert!(planned
            .iter()
            .all(|g| g.planned_operation.is_some() || g.status_code().is_some()));
        assert!(retired.iter().any(|g| g.retirement.is_some()));
        let g = &op[0];
        println!(
            "first operating row: {} / {} {} {} MW {}",
            g.plant_name,
            g.generator_id,
            g.state,
            g.nameplate_mw.unwrap_or(0.0),
            g.status
        );
    }

    #[tokio::test]
    #[ignore = "live network, downloads about 100 MB"]
    async fn live_eia930_six_month_files() {
        let c = client();
        let dir = tempfile::tempdir().expect("tmp");
        let mk = |dataset| SixMonthFile {
            dataset,
            year: 2026,
            half: HalfYear::JulDec,
        };

        let bal = dir.path().join("bal.csv");
        let n = c
            .download_eia930(mk(Dataset::Balance), &bal)
            .await
            .expect("balance");
        let (rows, errs, last, ba_count) = tokio::task::spawn_blocking(move || {
            let mut rows = 0u64;
            let mut errs = 0u64;
            let mut last = None;
            let mut bas = std::collections::BTreeSet::new();
            for r in BalanceReader::open(&bal).expect("open") {
                match r {
                    Ok(r) => {
                        rows += 1;
                        bas.insert(r.balancing_authority.clone());
                        last = Some((
                            r.balancing_authority,
                            r.utc_end,
                            r.demand_mw,
                            r.net_generation_mw,
                        ));
                    }
                    Err(_) => errs += 1,
                }
            }
            (rows, errs, last, bas.len())
        })
        .await
        .expect("join");
        println!("[{}] 930 BALANCE 2026 Jul-Dec: {n} bytes, {rows} rows, {errs} parse errors, {ba_count} BAs, last row {last:?}", now());
        assert!(rows > 100_000 && errs == 0);

        let sub = dir.path().join("sub.csv");
        c.download_eia930(mk(Dataset::Subregion), &sub)
            .await
            .expect("subregion");
        let sub_rows = SubregionReader::open(&sub).expect("open").flatten().count();
        println!("930 SUBREGION 2026 Jul-Dec: {sub_rows} rows");
        assert!(sub_rows > 10_000);

        let ic = dir.path().join("ic.csv");
        c.download_eia930(mk(Dataset::Interchange), &ic)
            .await
            .expect("interchange");
        let mut it = InterchangeReader::open(&ic).expect("open");
        let first = it.next().expect("row").expect("ok");
        let ic_rows = 1 + it.flatten().count();
        println!(
            "930 INTERCHANGE 2026 Jul-Dec: {ic_rows} rows; first {} -> {} {:?} MW",
            first.balancing_authority, first.diba, first.interchange_mw
        );
        assert!(ic_rows > 100_000);
    }
}

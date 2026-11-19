//! Typed-row decoding against small real OASIS responses (no network).

// Tests return `Result` so `?` can propagate, and assert inside them.
#![allow(
    clippy::panic_in_result_fn,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

mod support;

use chrono::{DateTime, TimeZone, Utc};
use gridapi_caiso::{
    decode_response, parse_outlook_csv, pivot_lmp, AncillaryPriceRow, Error, FromRecord,
    LmpComponent, LmpRow, LoadForecastRow, NodeRow, OutlookDataset, SystemScheduleRow,
};
use support::{fixture, zip_of, TestResult};

fn rows<T: FromRecord>(name: &str, data: &[u8]) -> Result<Vec<T>, Box<dyn std::error::Error>> {
    let zipped = zip_of(&[(name, data)])?;
    Ok(decode_response(&zipped)?
        .iter()
        .map(T::from_record)
        .collect::<Result<Vec<_>, _>>()?)
}

fn utc(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 10, h, m, 0)
        .single()
        .unwrap_or_default()
}

#[test]
fn day_ahead_lmp_csv_for_three_hubs() -> TestResult {
    let lmp: Vec<LmpRow> = rows("prc_lmp.csv", &fixture("prc_lmp_dam.csv")?)?;
    assert_eq!(lmp.len(), 15, "3 hubs x 5 components x 1 hour");
    let np15 = lmp
        .iter()
        .find(|r| r.node == "TH_NP15_GEN-APND" && r.component == LmpComponent::Lmp)
        .ok_or("no NP15 LMP row")?;
    assert!((np15.price - 46.18405).abs() < 1e-9);
    assert_eq!(np15.interval_start, utc(8, 0));
    assert_eq!(np15.interval_end, utc(9, 0));
    assert_eq!(np15.market_run_id.as_deref(), Some("DAM"));
    assert_eq!(np15.data_item, "LMP_PRC");

    let points = pivot_lmp(lmp);
    assert_eq!(points.len(), 3);
    let sp15 = points
        .iter()
        .find(|p| p.node == "TH_SP15_GEN-APND")
        .ok_or("no SP15 point")?;
    assert_eq!(sp15.lmp, Some(47.61829));
    assert_eq!(sp15.congestion, Some(-0.29821));
    assert_eq!(sp15.energy, Some(25.73741));
    assert_eq!(sp15.loss, Some(-0.69817));
    assert_eq!(sp15.ghg, Some(21.47904));
    Ok(())
}

#[test]
fn real_time_lmp_uses_the_value_column_at_five_minutes() -> TestResult {
    let lmp: Vec<LmpRow> = rows("rtm.csv", &fixture("prc_intvl_lmp_rtm.csv")?)?;
    assert!(!lmp.is_empty());
    assert!(lmp
        .iter()
        .all(|r| r.interval_end - r.interval_start == chrono::TimeDelta::minutes(5)));
    assert!(lmp
        .iter()
        .all(|r| r.market_run_id.as_deref() == Some("RTM")));
    Ok(())
}

#[test]
fn rtpd_lmp_uses_the_prc_column_at_fifteen_minutes() -> TestResult {
    let lmp: Vec<LmpRow> = rows("rtpd.csv", &fixture("prc_rtpd_lmp.csv")?)?;
    let first = lmp.first().ok_or("empty")?;
    assert_eq!(first.component, LmpComponent::Lmp);
    assert!((first.price - 62.06948).abs() < 1e-9);
    assert_eq!(
        first.interval_end - first.interval_start,
        chrono::TimeDelta::minutes(15)
    );
    Ok(())
}

#[test]
fn load_forecast_rows() -> TestResult {
    let rows: Vec<LoadForecastRow> = rows("sld.csv", &fixture("sld_fcst.csv")?)?;
    let ava = rows
        .iter()
        .find(|r| r.tac_area == "AVA" && r.interval_start == utc(8, 0))
        .ok_or("no AVA 08:00 row")?;
    assert!((ava.mw - 1139.9).abs() < 1e-9);
    assert_eq!(ava.data_item, "SYS_FCST_DA_MW");
    assert_eq!(ava.label.as_deref(), Some("Demand Forecast Day Ahead"));
    Ok(())
}

#[test]
fn system_load_and_resource_schedule_rows() -> TestResult {
    let rows: Vec<SystemScheduleRow> = rows("slrs.csv", &fixture("ene_slrs.csv")?)?;
    let generation = rows
        .iter()
        .find(|r| r.data_item == "ISO_TOT_GEN_MW" && r.interval_start == utc(8, 0))
        .ok_or("no generation row")?;
    assert_eq!(generation.tac_zone, "Caiso_Totals");
    assert_eq!(generation.schedule.as_deref(), Some("Generation"));
    assert!((generation.mw - 18773.22).abs() < 1e-9);
    Ok(())
}

#[test]
fn ancillary_price_rows() -> TestResult {
    let rows: Vec<AncillaryPriceRow> = rows("as.csv", &fixture("prc_as.csv")?)?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].anc_type, "RU");
    assert_eq!(rows[0].anc_region, "AS_CAISO_EXP");
    assert_eq!(rows[0].data_item, "RU_CLR_PRC");
    assert!((rows[0].price - 0.76822).abs() < 1e-9);
    Ok(())
}

#[test]
fn node_lists_for_both_pnodes_and_apnodes() -> TestResult {
    let ap: Vec<NodeRow> = rows("ap.csv", &fixture("atl_apnode.csv")?)?;
    assert_eq!(ap.len(), 4);
    assert_eq!(ap[0].id, "OTMESA_2_PL1X3-APND");
    assert_eq!(ap[0].node_type.as_deref(), Some("AG"));
    assert!(ap[0].effective_start.is_some());

    let p: Vec<NodeRow> = rows("p.csv", &fixture("atl_pnode.csv")?)?;
    assert_eq!(p[0].id, "RESERVE_6_N001");
    assert_eq!(p[0].node_type, None, "blank PNODE_TYPE is None, not empty");
    Ok(())
}

#[test]
fn xml_report_decodes_to_the_same_lmp_values_as_csv() -> TestResult {
    let lmp: Vec<LmpRow> = rows("prc_lmp.xml", &fixture("prc_lmp_dam.xml")?)?;
    let total = lmp
        .iter()
        .find(|r| r.component == LmpComponent::Lmp)
        .ok_or("no LMP row in XML")?;
    assert_eq!(total.node, "TH_NP15_GEN-APND");
    assert!((total.price - 46.18405).abs() < 1e-9);
    assert_eq!(total.interval_start, utc(8, 0));
    Ok(())
}

#[test]
fn oasis_error_documents_become_typed_errors() -> TestResult {
    for (file, code) in [("error_no_data.xml", 1000), ("error_31_days.xml", 1004)] {
        let zipped = zip_of(&[("err.xml", &fixture(file)?)])?;
        match decode_response(&zipped) {
            Err(Error::Oasis {
                code: got,
                description,
            }) => {
                assert_eq!(got, code, "{file}");
                assert!(!description.is_empty());
            }
            other => return Err(format!("{file}: expected Oasis error, got {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn outlook_fixtures_parse() -> TestResult {
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("bad date")?;

    let fuel = parse_outlook_csv(
        OutlookDataset::FuelSource,
        date,
        &fixture("outlook_fuelsource.csv")?,
    )?;
    assert_eq!(fuel.columns.first().map(String::as_str), Some("Solar"));
    assert!(fuel.column_index("Natural Gas").is_some());
    assert_eq!(fuel.value(&fuel.points[0], "Wind"), Some(6932.0));
    assert_eq!(fuel.value(&fuel.points[0], "Solar"), Some(-88.0));

    let demand = parse_outlook_csv(
        OutlookDataset::Demand,
        date,
        &fixture("outlook_demand.csv")?,
    )?;
    assert_eq!(
        demand.value(&demand.points[0], "Current demand"),
        Some(14705.0)
    );
    assert_eq!(demand.value(&demand.points[0], "Demand response"), None);
    assert_eq!(demand.points.len(), 6);

    for (dataset, file) in [
        (OutlookDataset::NetDemand, "outlook_netdemand.csv"),
        (OutlookDataset::Storage, "outlook_storage.csv"),
        (OutlookDataset::Co2, "outlook_co2.csv"),
    ] {
        let series = parse_outlook_csv(dataset, date, &fixture(file)?)?;
        assert!(series.latest().is_some(), "{file}");
    }
    Ok(())
}

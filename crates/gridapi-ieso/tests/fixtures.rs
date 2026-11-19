//! Decoder tests over small real responses captured from
//! reports-public.ieso.ca on 2026-11-18 (see tests/fixtures).
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

use chrono::NaiveDate;
use gridapi_ieso::data::adequacy::parse_adequacy;
use gridapi_ieso::data::demand::parse_demand;
use gridapi_ieso::data::generation::{parse_output_by_fuel_hourly, parse_output_capability};
use gridapi_ieso::data::intertie::parse_intertie_schedule_flow;
use gridapi_ieso::data::lmp::{parse_hourly_lmp, parse_interval_lmp, ONTARIO_ZONAL_PRICE};
use gridapi_ieso::data::zonal::{
    parse_ontario_zonal_hourly, parse_ontario_zonal_realtime, parse_virtual_zonal_realtime,
};
use gridapi_ieso::Error;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
}

#[test]
fn day_ahead_ontario_zonal_price_hour_one_decodes() {
    let doc = parse_ontario_zonal_hourly(include_str!("fixtures/da_ontario_zonal_20261117.xml"))
        .expect("decodes");
    assert_eq!(doc.body.delivery_date, d(2026, 11, 17));
    assert_eq!(doc.revision, Some(1));
    assert_eq!(
        doc.created_at.map(|t| t.to_string()).as_deref(),
        Some("2026-11-16 13:31:19")
    );
    assert_eq!(doc.body.hours.len(), 24);
    let h1 = doc.body.hour(1).expect("hour 1");
    assert_eq!(h1.zonal_price, Some(28.47));
    assert_eq!(h1.loss_price, Some(-0.04));
    assert_eq!(h1.congestion_price, Some(0.04));
    assert_eq!(h1.flag.as_deref(), Some("DSO-RD"));
    assert_eq!(
        doc.body.start_utc(1).map(|t| t.to_rfc3339()).as_deref(),
        Some("2026-11-17T05:00:00+00:00")
    );
}

#[test]
fn predispatch_zonal_price_has_no_flag_column() {
    let doc = parse_ontario_zonal_hourly(include_str!("fixtures/predisp_ontario_zonal.xml"))
        .expect("decodes");
    assert_eq!(doc.body.hours.len(), 24);
    assert_eq!(doc.body.hour(1).and_then(|h| h.zonal_price), Some(42.89));
    assert!(doc.body.hours.iter().all(|h| h.flag.is_none()));
}

#[test]
fn realtime_zonal_price_marks_unpublished_intervals_as_none() {
    let doc = parse_ontario_zonal_realtime(include_str!("fixtures/rt_ontario_zonal.xml"))
        .expect("decodes");
    let b = &doc.body;
    assert_eq!((b.delivery_date, b.delivery_hour), (d(2026, 11, 18), 2));
    assert_eq!(b.intervals.len(), 12);
    assert_eq!(b.intervals[0].lmp, Some(42.95));
    assert_eq!(b.intervals[2].lmp, Some(34.79));
    assert_eq!(b.intervals[3].lmp, None, "interval 4 not yet published");
    assert_eq!(b.intervals[3].flag, None);
    assert_eq!(b.average.as_ref().and_then(|a| a.lmp), Some(42.95));
    let published: Vec<_> = b
        .published()
        .map(|(t, i)| (t.to_rfc3339(), i.interval))
        .collect();
    assert_eq!(
        published.first(),
        Some(&("2026-11-18T06:00:00+00:00".to_owned(), 1)),
        "hour 2 on 2026-11-18 EST starts 06:00 UTC"
    );
    assert_eq!(published.len(), 3);
}

#[test]
fn virtual_zonal_prices_group_intervals_by_zone() {
    let doc = parse_virtual_zonal_realtime(include_str!("fixtures/rt_zonal_energy_prices.xml"))
        .expect("decodes");
    assert_eq!(doc.body.delivery_hour, 2);
    assert_eq!(doc.body.zones.len(), 2);
    let east = &doc.body.zones[0];
    assert_eq!(east.name, "EAST:HUB");
    assert_eq!(east.intervals.len(), 12);
    assert_eq!(east.intervals[0].zonal_price, Some(44.68));
    assert_eq!(east.intervals[0].loss_price, Some(1.41));
    assert_eq!(east.intervals[0].flag, Some(0));
    assert_eq!(east.intervals[11].zonal_price, None);
}

#[test]
fn day_ahead_lmp_csv_exposes_nodes_and_the_ontario_zonal_row() {
    let file = parse_hourly_lmp(include_str!("fixtures/da_lmp_20261117.csv")).expect("decodes");
    assert_eq!(file.delivery_date, d(2026, 11, 17));
    assert_eq!(file.created_at.to_string(), "2026-11-16 13:35:36");
    assert_eq!(file.rows.len(), 15);
    let zonal: Vec<_> = file.location(ONTARIO_ZONAL_PRICE).collect();
    assert_eq!(zonal.len(), 3);
    assert_eq!((zonal[0].hour, zonal[0].lmp), (1, Some(28.47)));
    assert_eq!(zonal[0].congestion_price, Some(0.04));
    let node = file
        .location("ABITIBISF-LT.AG_T1")
        .next()
        .expect("node row");
    assert_eq!(node.loss_price, Some(-1.55));
}

#[test]
fn realtime_lmp_csv_carries_the_interval_column() {
    let file = parse_interval_lmp(include_str!("fixtures/rt_lmp_2026111802.csv")).expect("decodes");
    assert_eq!(file.delivery_date, d(2026, 11, 18));
    assert_eq!(file.rows.len(), 11);
    let first = &file.rows[0];
    assert_eq!((first.hour, first.interval), (2, 1));
    assert_eq!(first.location, "ABITIBISF-LT.AG_T1");
    assert_eq!(first.lmp, Some(29.02));
}

#[test]
fn lmp_header_that_is_not_a_created_line_is_malformed() {
    let err = parse_hourly_lmp("Delivery Hour,Pricing Location,LMP\n1,X,1\n").expect_err("bad");
    assert!(matches!(err, Error::Malformed(_)), "{err}");
}

#[test]
fn lmp_with_blank_price_cells_decodes_them_as_none() {
    let text = "CREATED AT 2026/11/18 01:08:18 FOR 2026/11/18\n\
                Delivery Hour,Interval,Pricing Location,LMP,Energy Loss Price,Energy Congestion Price\n\
                2,4,NODE,,,\n";
    let file = parse_interval_lmp(text).expect("decodes");
    assert_eq!(file.rows[0].lmp, None);
}

#[test]
fn demand_csv_skips_comment_lines_and_reads_both_demand_columns() {
    let report = parse_demand(include_str!("fixtures/demand_2026.csv")).expect("decodes");
    assert_eq!(
        report.created_at.map(|t| t.to_string()).as_deref(),
        Some("2026-11-17 07:30:13")
    );
    assert_eq!(report.rows.len(), 12);
    let first = &report.rows[0];
    assert_eq!(first.date, d(2026, 1, 1));
    assert_eq!(
        (first.hour, first.market_demand, first.ontario_demand),
        (1, Some(15424.0), Some(19643.0))
    );
    let last = report.rows.last().expect("rows");
    assert_eq!(
        (last.date, last.hour, last.ontario_demand),
        (d(2026, 11, 17), 1, Some(17076.0))
    );
}

#[test]
fn demand_without_a_header_is_malformed() {
    let err = parse_demand("\\\\Hourly Demand Report,,,\n").expect_err("no header");
    assert!(matches!(err, Error::Malformed(_)));
}

#[test]
fn generator_output_capability_sums_by_fuel() {
    let doc = parse_output_capability(include_str!("fixtures/gen_output_capability.xml"))
        .expect("decodes");
    assert_eq!(doc.body.date, d(2026, 11, 17));
    assert_eq!(doc.body.generators.len(), 2);
    let idle = &doc.body.generators[0];
    assert_eq!(
        (idle.name.as_str(), idle.fuel_type.as_str()),
        ("BRUCEA-G1", "NUCLEAR")
    );
    assert_eq!(idle.outputs.len(), 24);
    assert_eq!(idle.capabilities.len(), 24);
    assert_eq!(idle.available_capacity.len(), 24);
    let gas = doc.body.output_by_fuel("gas");
    assert!((gas.iter().sum::<f64>() - 11977.0).abs() < 1e-9, "{gas:?}");
    assert!(doc.body.output_by_fuel("coal").iter().all(|v| *v == 0.0));
}

#[test]
fn fuel_hourly_decodes_quality_and_output() {
    let doc = parse_output_by_fuel_hourly(include_str!("fixtures/gen_output_by_fuel_hourly.xml"))
        .expect("decodes");
    assert_eq!(doc.body.year, 2026);
    assert_eq!(doc.body.days.len(), 1);
    let day = &doc.body.days[0];
    assert_eq!(day.day, d(2026, 1, 1));
    assert_eq!(day.hours.len(), 2);
    let nuclear = &day.hours[0].fuels[0];
    assert_eq!(nuclear.fuel, "NUCLEAR");
    assert_eq!(nuclear.output_mw, Some(11043.0));
    assert_eq!(nuclear.quality, Some(0));
    let gas = &day.hours[0].fuels[1];
    assert_eq!((gas.fuel.as_str(), gas.quality), ("GAS", Some(-1)));
    assert_eq!(day.hours[0].fuels.len(), 7);
}

#[test]
fn intertie_schedule_flow_has_schedules_and_five_minute_flows() {
    let doc = parse_intertie_schedule_flow(include_str!("fixtures/intertie_schedule_flow.xml"))
        .expect("decodes");
    assert_eq!(doc.body.date, d(2026, 11, 17));
    assert_eq!(doc.body.zones.len(), 2);
    let mb = doc.body.zone("MANITOBA").expect("manitoba");
    assert_eq!(mb.schedules.len(), 24);
    assert_eq!(mb.schedules[0].hour, 1);
    assert_eq!(mb.schedules[0].import_mw, Some(0.0));
    assert_eq!(mb.actuals.len(), 24);
    assert_eq!(mb.actuals[0].flow_mw, Some(-0.3));
    assert_eq!((mb.actuals[0].hour, mb.actuals[0].interval), (1, 1));
    assert!(doc.body.zone("ATLANTIS").is_none());
}

#[test]
fn intertie_totals_decode_when_present() {
    let xml = r#"<?xml version="1.0"?>
<IMODocument xmlns="http://www.theIMO.com/schema"><IMODocHeader><DocRevision>2</DocRevision>
<CreatedAt>2026-11-18T00:30:58</CreatedAt></IMODocHeader>
<IMODocBody><Date>2026-11-17</Date>
<Totals><Schedules><Schedule><Hour>1</Hour><Import>446</Import><Export>2286</Export></Schedule></Schedules>
<Actuals><Actual><Hour>24</Hour><Interval>12</Interval><Flow>2407.9</Flow></Actual></Actuals></Totals>
</IMODocBody></IMODocument>"#;
    let doc = parse_intertie_schedule_flow(xml).expect("decodes");
    let totals = doc.body.totals.expect("totals");
    assert_eq!(totals.schedules[0].export_mw, Some(2286.0));
    assert_eq!(totals.actuals[0].flow_mw, Some(2407.9));
    assert!(doc.body.zones.is_empty());
}

#[test]
fn adequacy_exposes_headline_series_and_fuel_resources() {
    let doc = parse_adequacy(include_str!("fixtures/adequacy3_20261117.xml")).expect("decodes");
    let b = &doc.body;
    assert_eq!(b.delivery_date, d(2026, 11, 17));
    assert_eq!(b.supply.capacities.len(), 24);
    assert_eq!(b.supply.capacities[0].value, Some(46768.0));
    assert_eq!(b.supply.energies[0].value, Some(27763.0));
    assert_eq!(b.supply.total_supplies[0].value, Some(23971.0));
    assert_eq!(b.supply.bottled_capacities[0].value, Some(911.0));
    assert_eq!(b.supply.regulations[0].value, Some(177.0));
    assert_eq!(b.supply.internal_resources.by_fuel.len(), 2);
    assert_eq!(b.supply.internal_resources.by_fuel[0].fuel_type, "Nuclear");
    assert_eq!(
        b.supply.internal_resources.by_fuel[0].capacities[0].value,
        Some(16992.0)
    );
    assert_eq!(b.demand.ontario_demand.forecast[0].value, Some(18444.0));
    assert_eq!(b.demand.ontario_demand.peak[0].value, Some(20124.0));
    assert_eq!(b.demand.ontario_demand.wind_embedded[0].value, Some(112.0));
    assert_eq!(b.demand.total_requirements[0].value, Some(15177.0));
    assert_eq!(b.demand.excess_capacities[0].value, Some(4675.0));
    assert_eq!(b.demand.unscheduled_imports[0].value, Some(688.0));
}

#[test]
fn old_style_imo_envelope_and_new_envelope_both_decode() {
    // IntertieScheduleFlow uses IMODocument/IMODocBody; Adequacy3 uses Document/DocBody.
    assert!(
        parse_intertie_schedule_flow(include_str!("fixtures/intertie_schedule_flow.xml")).is_ok()
    );
    assert!(parse_adequacy(include_str!("fixtures/adequacy3_20261117.xml")).is_ok());
}

#[test]
fn garbage_xml_is_an_xml_error_not_a_panic() {
    let err = parse_ontario_zonal_hourly("<Document><DocHeader>").expect_err("truncated");
    assert!(matches!(err, Error::Xml(_)), "{err}");
    let err = parse_ontario_zonal_hourly("not xml at all").expect_err("not xml");
    assert!(matches!(err, Error::Xml(_)), "{err}");
}

#[test]
fn a_utf8_byte_order_mark_is_tolerated() {
    let text = format!(
        "\u{feff}{}",
        include_str!("fixtures/da_ontario_zonal_20261117.xml")
    );
    assert!(parse_ontario_zonal_hourly(&text).is_ok());
}

#[test]
fn csv_files_with_crlf_line_endings_decode_the_same() {
    let demand = include_str!("fixtures/demand_2026.csv").replace('\n', "\r\n");
    let report = parse_demand(&demand).expect("decodes CRLF demand");
    assert_eq!(report.rows.len(), 12);
    assert_eq!(report.rows[0].ontario_demand, Some(19643.0));
    assert!(report.created_at.is_some());

    let lmp = include_str!("fixtures/da_lmp_20261117.csv").replace('\n', "\r\n");
    let file = parse_hourly_lmp(&lmp).expect("decodes CRLF lmp");
    assert_eq!(file.delivery_date, d(2026, 11, 17));
    assert_eq!(file.rows.len(), 15);
}

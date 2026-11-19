//! Decoder tests over small real responses captured from apimgw.aeso.ca on
//! 2026-11-18 (see tests/fixtures).
#![allow(
    clippy::expect_used,
    clippy::assert_is_empty,
    reason = "tests panic on purpose; a failed expect is the test failure"
)]

use chrono::NaiveDate;
use gridapi_aeso::decode;
use gridapi_aeso::model::market::Direction;
use gridapi_aeso::Error;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
}

#[test]
fn pool_price_midnight_16_nov_2026_decodes() {
    let rows =
        decode::pool_price(include_str!("fixtures/pool_price_20261116.json")).expect("decodes");
    assert_eq!(rows.len(), 24);
    let first = &rows[0];
    assert_eq!(first.pool_price, Some(1.08));
    assert_eq!(first.forecast_pool_price, Some(5.30));
    assert_eq!(first.rolling_30day_average, Some(43.72));
    assert_eq!(first.begin_mpt.to_string(), "2026-11-16 00:00:00");
    assert_eq!(first.begin_utc.to_rfc3339(), "2026-11-16T06:00:00+00:00");
    assert_eq!(rows[23].pool_price, Some(42.09));
}

#[test]
fn unsettled_hours_have_no_pool_price_but_keep_the_forecast() {
    let body = r#"{"timestamp":"2026-11-18 06:24:20.755+0000","responseCode":"200","return":{"Pool Price Report":[
        {"begin_datetime_utc":"2026-11-18 06:00","begin_datetime_mpt":"2026-11-18 00:00","pool_price":"","forecast_pool_price":"13.80","rolling_30day_avg":""}]}}"#;
    let rows = decode::pool_price(body).expect("decodes");
    assert_eq!(rows[0].pool_price, None);
    assert_eq!(rows[0].forecast_pool_price, Some(13.80));
    assert_eq!(rows[0].rolling_30day_average, None);
}

#[test]
fn an_empty_pool_price_report_decodes_to_no_rows() {
    let body = r#"{"timestamp":"t","responseCode":"200","return":{"Pool Price Report":[]}}"#;
    assert!(decode::pool_price(body).expect("decodes").is_empty());
}

#[test]
fn system_marginal_price_intervals_carry_start_end_and_volume() {
    let rows = decode::system_marginal_price(include_str!("fixtures/system_marginal_price.json"))
        .expect("decodes");
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].system_marginal_price, Some(30.61));
    assert_eq!(rows[0].volume, Some(30.0));
    assert_eq!(rows[0].begin_utc.to_rfc3339(), "2026-11-17T05:36:00+00:00");
    assert_eq!(rows[0].end_utc.to_rfc3339(), "2026-11-17T06:00:00+00:00");
    assert!(rows[0].end_mpt > rows[0].begin_mpt);
}

#[test]
fn current_system_marginal_price_has_no_end_time() {
    let rows = decode::current_system_marginal_price(include_str!(
        "fixtures/system_marginal_price_current.json"
    ))
    .expect("decodes");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].system_marginal_price, Some(19.74));
    assert_eq!(rows[0].volume, Some(60.0));
    assert_eq!(rows[0].begin_mpt.to_string(), "2026-11-18 00:31:00");
}

#[test]
fn alberta_internal_load_has_actual_and_forecast() {
    let rows = decode::internal_load(include_str!("fixtures/alberta_internal_load.json"))
        .expect("decodes");
    assert_eq!(rows.len(), 24);
    assert_eq!(rows[0].alberta_internal_load, Some(9188.0));
    assert_eq!(rows[0].forecast_alberta_internal_load, Some(7682.0));
    assert_eq!(rows[23].alberta_internal_load, Some(13533.0));
}

#[test]
fn csd_summary_exposes_totals_fuels_and_interchange_paths() {
    let s = decode::supply_demand_summary(include_str!("fixtures/csd_summary_v2.json"))
        .expect("decodes");
    assert_eq!(s.alberta_internal_load, Some(8054.0));
    assert_eq!(s.total_net_generation, Some(14758.0));
    assert_eq!(s.net_actual_interchange, Some(660.0));
    assert_eq!(
        s.dispatched_contingency_reserve_total,
        Some(353.0),
        "typo'd wire name is mapped"
    );
    assert_eq!(s.effective_utc.to_rfc3339(), "2026-11-18T06:45:00+00:00");
    assert_eq!(s.generation_data_list.len(), 9);
    let wind = s.fuel("wind").expect("wind");
    assert_eq!(wind.aggregated_net_generation, Some(3169.0));
    assert_eq!(wind.aggregated_maximum_capability, Some(5684.0));
    assert_eq!(s.flow("British Columbia"), Some(660.0));
    assert_eq!(s.flow("saskatchewan"), Some(-64.0), "negative is export");
    assert_eq!(s.flow("Atlantis"), None);
}

#[test]
fn generation_assets_list_each_unit_with_fuel_and_sub_fuel() {
    let a = decode::generation_assets(include_str!("fixtures/csd_generation_assets.json"))
        .expect("decodes");
    assert_eq!(a.asset_list.len(), 9);
    let acd1 = &a.asset_list[0];
    assert_eq!(
        (acd1.asset.as_str(), acd1.fuel_type.as_str()),
        ("AST13", "SOLAR")
    );
    assert_eq!(acd1.sub_fuel_type, None, "empty string means none");
    assert_eq!(acd1.maximum_capability, Some(140.0));
    let alp1 = a
        .asset_list
        .iter()
        .find(|x| x.asset == "AST1")
        .expect("AST1");
    assert_eq!(alp1.sub_fuel_type.as_deref(), Some("SIMPLE_CYCLE"));
    assert!(a.net_generation_by_fuel("gas") > 0.0);
    assert_eq!(a.net_generation_by_fuel("coal"), 0.0);
}

#[test]
fn interchange_capability_reads_import_and_export_per_hour() {
    let r = decode::interchange(include_str!("fixtures/itc_interchange.json")).expect("decodes");
    let bc = r.bc.as_ref().expect("bc");
    assert_eq!(bc.allocations.len(), 2);
    let h1 = &bc.allocations[0];
    assert_eq!((h1.he, h1.date, h1.flowgate), (1, d(2026, 11, 16), false));
    let imp = h1.import.as_ref().expect("import");
    assert_eq!(imp.transfer_type, "BC_IMPORT");
    assert_eq!(
        (imp.ttc, imp.atc, imp.trm_total),
        (Some(1106.0), Some(732.0), Some(69.0))
    );
    assert_eq!(imp.reason, None);
    assert_eq!(h1.export.as_ref().and_then(|e| e.gross_offer), Some(497.0));
    let fg = r.bc_matl_flowgate.as_ref().expect("flowgate");
    let fg_imp = fg.allocations[0].import.as_ref().expect("import");
    assert!(fg.allocations[0].flowgate);
    assert_eq!(
        fg_imp.reason.as_deref(),
        Some("ISO rules Section 205.9 Limit")
    );
    assert_eq!(fg_imp.trm_allocation, None, "flowgates carry no allocation");
    assert!(
        r.system_flowgate.is_some(),
        "the API's `SystemlFlowgate` spelling is accepted"
    );
    assert_eq!(r.all().count(), 10);
}

#[test]
fn an_empty_interchange_reply_is_an_empty_report() {
    let r = decode::interchange(include_str!("fixtures/itc_empty.json")).expect("decodes");
    assert_eq!(r.all().count(), 0);
}

#[test]
fn intertie_outages_list_elements_and_affected_lines() {
    let o = decode::intertie_outages(include_str!("fixtures/itc_outages.json")).expect("decodes");
    assert_eq!(o.len(), 3);
    assert_eq!(o[0].element, "Path 1 Outage - AIES Islanding");
    assert_eq!(o[0].affected.len(), 3);
    assert_eq!(o[0].affected[0].display_value, "BC/MATL");
    assert_eq!(o[0].affected[0].affected_lines, vec!["BC_MATL".to_owned()]);
    assert_eq!(o[0].from_local.to_string(), "2026-08-24 09:00:00");
    assert_eq!(o[0].to_local.to_string(), "2026-09-02 00:02:00");
}

#[test]
fn no_outages_decodes_to_an_empty_list() {
    let body = r#"{"message":"ok","responseCode":"200","return":{"Outages":{"Outage":[]}}}"#;
    assert!(decode::intertie_outages(body).expect("decodes").is_empty());
}

#[test]
fn merit_order_blocks_decode_flags_prices_and_dispatch() {
    let hours = decode::merit_order(include_str!("fixtures/merit_order.json")).expect("decodes");
    assert_eq!(hours.len(), 2);
    let h = &hours[0];
    assert_eq!(h.begin_mpt.to_string(), "2026-09-01 00:00:00");
    assert_eq!(h.begin_utc.to_rfc3339(), "2026-09-01T06:00:00+00:00");
    assert_eq!(h.energy_blocks.len(), 5);
    let alp1 = &h.energy_blocks[0];
    assert_eq!(alp1.asset_id, "AST1");
    assert_eq!(alp1.import_or_export, None);
    assert_eq!(alp1.block_price, Some(1363.22));
    assert!(!alp1.dispatched);
    assert!(alp1.flexible);
    assert_eq!(alp1.offer_control, "Example Energy Inc.");
    let tvs1 = &h.energy_blocks[3];
    assert!(tvs1.dispatched);
    assert_eq!(
        (tvs1.from_mw, tvs1.to_mw, tvs1.dispatched_mw),
        (Some(0.0), Some(458.0), Some(458.0))
    );
    let marginal = h.marginal_block().expect("a dispatched block exists");
    assert_eq!(marginal.block_price, Some(0.0));
}

#[test]
fn merit_order_import_export_markers_are_typed() {
    let body = r#"{"return":{"data":[{"begin_dateTime_utc":"2026-09-01 06:00","begin_dateTime_mpt":"2026-09-01 00:00","energy_blocks":[
      {"import_or_export":"I","asset_ID":"BC","block_number":1,"block_price":"8.00","from_MW":0,"to_MW":5,"block_size":5,"available_MW":5,"dispatched?":"Y","dispatched_MW":5,"flexible?":"N","offer_control":"x"},
      {"import_or_export":"E","asset_ID":"SK","block_number":"2","block_price":"","from_MW":0,"to_MW":5,"block_size":5,"available_MW":5,"dispatched?":"N","dispatched_MW":0,"flexible?":"N","offer_control":"y"}]}]}}"#;
    let hours = decode::merit_order(body).expect("decodes");
    let b = &hours[0].energy_blocks;
    assert_eq!(b[0].import_or_export, Some(Direction::Import));
    assert_eq!(b[1].import_or_export, Some(Direction::Export));
    assert_eq!(b[1].block_number, 2, "numeric strings are accepted");
    assert_eq!(b[1].block_price, None);
}

#[test]
fn merit_order_rejects_an_unknown_direction_marker() {
    let body = r#"{"return":{"data":[{"begin_dateTime_utc":"2026-09-01 06:00","begin_dateTime_mpt":"2026-09-01 00:00","energy_blocks":[
      {"import_or_export":"Q","asset_ID":"BC","block_number":1,"block_price":"8.00","from_MW":0,"to_MW":5,"block_size":5,"available_MW":5,"dispatched?":"Y","dispatched_MW":5,"flexible?":"N","offer_control":"x"}]}]}}"#;
    assert!(matches!(
        decode::merit_order(body),
        Err(Error::Decode { .. })
    ));
}

#[test]
fn aies_gen_capacity_groups_outage_volumes_by_fuel() {
    let f = decode::aies_gen_capacity(include_str!("fixtures/aies_gen_capacity.json"))
        .expect("decodes");
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].fuel_type, "GAS");
    assert_eq!(f[0].sub_fuel_type.as_deref(), Some("COGENERATION"));
    assert_eq!(f[0].hours.len(), 3);
    let g = &f[0].hours[0].outage_grouping;
    assert_eq!(
        (
            g.maximum_capability,
            g.operational_outage,
            g.mothball_outage,
            g.available_capability
        ),
        (Some(6125.0), Some(1293.0), Some(0.0), Some(4832.0))
    );
}

#[test]
fn load_outage_forecast_hours_decode() {
    let h = decode::load_outage_forecast(include_str!("fixtures/load_outage_forecast.json"))
        .expect("decodes");
    assert_eq!(h.len(), 4);
    assert_eq!(h[0].load_outage_forecast, Some(0.0));
}

#[test]
fn asset_list_and_participants_decode() {
    let a = decode::asset_list(include_str!("fixtures/asset_list.json")).expect("decodes");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].asset_id, "AST1");
    assert_eq!(a[0].asset_type, "SOURCE");
    assert_eq!(a[0].pool_participant_id, "PPI1");
    assert_eq!(a[0].net_to_grid_asset_flag, None);
    let p = decode::pool_participants(include_str!("fixtures/pool_participants.json"))
        .expect("decodes");
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].pool_participant_id, "PPI2");
    assert_eq!(p[0].agent_list.len(), 2);
    assert!(p[0]
        .corporate_contact
        .as_deref()
        .is_some_and(|c| c.contains("Example City")));
}

#[test]
fn empty_unit_commitment_list_is_fine() {
    let u = decode::unit_commitment(include_str!("fixtures/unit_commitment_empty.json"))
        .expect("decodes");
    assert!(u.is_empty());
}

#[test]
fn unit_commitment_directive_decodes_all_eight_timestamps() {
    let body = r#"{"return":{"unit_commitment":[{"asset_ID":"BR5","issued_time_utc":"2024-08-21 19:46:50","issued_time_mpt":"2024-08-21 13:46:50","begin_time_utc":"2024-08-21 20:00:00","begin_time_mpt":"2024-08-21 14:00:00","operation_start_time_utc":"2024-08-22 20:00:00","operation_start_time_mpt":"2024-08-22 14:00:00","operation_end_time_utc":"2024-08-23 02:00:00","operation_end_time_mpt":"2024-08-22 20:00:00"}]}}"#;
    let u = decode::unit_commitment(body).expect("decodes");
    assert_eq!(u[0].asset_id, "BR5");
    assert_eq!(u[0].issued_utc.to_rfc3339(), "2024-08-21T19:46:50+00:00");
    assert_eq!(u[0].operation_end_mpt.to_string(), "2024-08-22 20:00:00");
}

#[test]
fn metered_volume_nests_participant_asset_and_hours() {
    let body = r#"{"return":[{"pool_participant_ID":"PPI1","asset_list":[{"asset_ID":"AST1","asset_class":"IPP","metered_volume_list":[
      {"begin_date_utc":"2026-11-01 06:00","begin_date_mpt":"2026-11-01 00:00","metered_volume":"0"},
      {"begin_date_utc":"2026-11-01 07:00","begin_date_mpt":"2026-11-01 01:00","metered_volume":"3.5"}]}]}]}"#;
    let m = decode::metered_volume(body).expect("decodes");
    assert_eq!(m[0].pool_participant_id, "PPI1");
    let a = &m[0].asset_list[0];
    assert_eq!(
        (a.asset_id.as_str(), a.asset_class.as_deref()),
        ("AST1", Some("IPP"))
    );
    assert_eq!(a.metered_volume_list[1].metered_volume, Some(3.5));
}

#[test]
fn operating_reserve_offers_decode_products_and_prices() {
    let body = r#"{"return":{"Operating Reserve Trade Merit Order":[{"begin_datetime_utc":"2026-09-01 06:00","begin_datetime_mpt":"2026-09-01 00:00","operating_reserve_blocks":[
      {"commodity":"ACTIVE","product":"RR","asset_ID":"AST10","volume":"5","active_price":"34.45","premium_price":"0.00","activation_price":"0.00","offer_control":"Air Liquide Canada Inc."}]}]}}"#;
    let h = decode::operating_reserve_offers(body).expect("decodes");
    let b = &h[0].operating_reserve_blocks[0];
    assert_eq!((b.commodity.as_str(), b.product.as_str()), ("ACTIVE", "RR"));
    assert_eq!((b.volume, b.active_price), (Some(5.0), Some(34.45)));
}

#[test]
fn a_wrong_shape_is_a_decode_error_naming_the_endpoint() {
    let err = decode::supply_demand_summary(r#"{"return":{"nope":1}}"#).expect_err("wrong shape");
    assert!(
        matches!(&err, Error::Decode { endpoint, .. } if endpoint.contains("csd")),
        "{err}"
    );
    let err = decode::pool_price("<html>").expect_err("not json");
    assert!(matches!(err, Error::Decode { .. }));
}

#[test]
fn a_bad_number_in_a_price_cell_is_a_decode_error_not_a_zero() {
    let body = r#"{"return":{"Pool Price Report":[{"begin_datetime_utc":"2026-11-18 06:00","begin_datetime_mpt":"2026-11-18 00:00","pool_price":"abc","forecast_pool_price":"1","rolling_30day_avg":"1"}]}}"#;
    assert!(matches!(
        decode::pool_price(body),
        Err(Error::Decode { .. })
    ));
}

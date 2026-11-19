//! Live checks against api.ercot.com, shared by `tests/live.rs` (ignored tests) and
//! `examples/live_smoke.rs`. Each check returns a one-line summary of sample values or
//! a failure message. Wide tolerances: only sanity-bound the market data.

// Included by both tests/live.rs and examples/live_smoke.rs; each target uses a subset.
#![allow(dead_code)]

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Duration, NaiveDate, NaiveDateTime};
use gridapi_ercot::{
    ActualLoad, ArchiveQuery, Client, DayAheadSpp, Error, HourEnding, LoadForecast, Query,
    RealTimeLmp, RealTimeSpp, ReportRequest, SettlementPointType, SolarActual5Min,
    SystemGeneration, WindActual5Min,
};

pub type Check = Result<String, String>;

/// Approximate Central Standard Time "now" (UTC-6); checks look back with wide margins.
pub fn central_now() -> NaiveDateTime {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0);
    chrono::DateTime::from_timestamp(secs - 6 * 3600, 0)
        .expect("timestamp in range")
        .naive_utc()
}

fn yesterday() -> NaiveDate {
    central_now().date() - Duration::days(1)
}

fn ctx<T>(r: Result<T, Error>, what: &str) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {e}"))
}

fn ensure(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(msg())
    }
}

/// Every check, by name, for the smoke example.
pub async fn all(client: &Client) -> Vec<(&'static str, Check)> {
    vec![
        ("catalog", catalog(client).await),
        ("product", product(client).await),
        ("rt_spp", rt_spp(client).await),
        ("dam_spp", dam_spp(client).await),
        ("rt_lmp", rt_lmp(client).await),
        ("actual_load", actual_load(client).await),
        ("load_forecast", load_forecast(client).await),
        ("wind_5min", wind_5min(client).await),
        ("solar_5min", solar_5min(client).await),
        ("system_generation", system_generation(client).await),
        ("fuel_mix", fuel_mix(client).await),
        ("archive", archive(client).await),
        ("generic_query_all", generic_query_all(client).await),
        ("unknown_param_400", unknown_param(client).await),
    ]
}

pub async fn catalog(client: &Client) -> Check {
    let products = ctx(client.products().await, "products")?;
    ensure(products.len() >= 100, || {
        format!("only {} products", products.len())
    })?;
    for id in [
        "NP6-905-CD",
        "NP4-190-CD",
        "NP6-788-CD",
        "NP6-345-CD",
        "NP3-565-CD",
        "NP4-733-CD",
        "NP4-738-CD",
        "NP6-625-CD",
    ] {
        let p = products
            .iter()
            .find(|p| p.emil_id == id)
            .ok_or_else(|| format!("{id} missing from catalog"))?;
        ensure(p.is_queryable(), || format!("{id} has no artifacts"))?;
    }
    let queryable = products.iter().filter(|p| p.is_queryable()).count();
    Ok(format!(
        "{} products ({queryable} queryable)",
        products.len()
    ))
}

pub async fn product(client: &Client) -> Check {
    let p = ctx(client.product("NP6-905-CD").await, "product")?;
    ensure(p.artifact("spp_node_zone_hub").is_some(), || {
        "artifact missing".into()
    })?;
    Ok(format!(
        "{} | {} | last post {:?}",
        p.emil_id, p.name, p.last_post_datetime
    ))
}

pub async fn rt_spp(client: &Client) -> Check {
    let today = central_now().date();
    let latest = ctx(
        client.latest_real_time_spp("HB_NORTH", today).await,
        "latest rt spp",
    )?
    .ok_or("no HB_NORTH rows in the last two days")?;
    ensure(
        latest.settlement_point_type == SettlementPointType::Hub,
        || format!("{latest:?}"),
    )?;
    ensure((-300.0..=10_000.0).contains(&latest.price), || {
        format!("implausible {latest:?}")
    })?;
    let ending = latest
        .interval_ending()
        .ok_or_else(|| format!("bad interval {latest:?}"))?;
    ensure(central_now() - ending < Duration::hours(3), || {
        format!("stale: {latest:?}")
    })?;
    let page = ctx(
        client
            .fetch(
                &Query::<RealTimeSpp>::new()
                    .delivery_date(today, today)
                    .settlement_point_type(&SettlementPointType::Hub)
                    .delivery_hour(1, 1)
                    .size(500),
            )
            .await,
        "rt spp hubs",
    )?;
    ensure(page.records.iter().all(|r| r.delivery_hour == 1), || {
        "hour filter ignored".into()
    })?;
    let hubs: std::collections::BTreeSet<_> = page
        .records
        .iter()
        .map(|r| r.settlement_point.as_str())
        .collect();
    ensure(
        hubs.contains("HB_NORTH") && hubs.contains("HB_HOUSTON"),
        || format!("hubs: {hubs:?}"),
    )?;
    Ok(format!(
        "latest HB_NORTH {} h{} i{} (ends {ending}) = {:.2} $/MWh | {} hubs in hour 1",
        latest.delivery_date,
        latest.delivery_hour,
        latest.delivery_interval,
        latest.price,
        hubs.len()
    ))
}

pub async fn dam_spp(client: &Client) -> Check {
    let day = yesterday();
    let rows = ctx(
        client
            .fetch_all(
                &Query::<DayAheadSpp>::new()
                    .delivery_date(day, day)
                    .settlement_point("HB_HOUSTON"),
                3,
            )
            .await,
        "dam spp",
    )?;
    ensure((23..=25).contains(&rows.len()), || {
        format!("{} hourly rows for {day}", rows.len())
    })?;
    let peak = rows.iter().map(|r| r.price).fold(f64::MIN, f64::max);
    let he7 = rows
        .iter()
        .find(|r| r.hour_ending.get() == 7)
        .map(|r| r.price);
    let one = ctx(
        client
            .fetch(
                &Query::<DayAheadSpp>::new()
                    .delivery_date(day, day)
                    .settlement_point("HB_HOUSTON")
                    .hour_ending(HourEnding::try_new(7).expect("valid hour")),
            )
            .await,
        "dam spp HE07 exact filter",
    )?;
    ensure(one.records.len() == 1, || {
        format!("HE07 filter returned {}", one.records.len())
    })?;
    Ok(format!(
        "HB_HOUSTON {day}: {} hours, HE07 {:?}, peak {peak:.2} $/MWh",
        rows.len(),
        he7
    ))
}

pub async fn rt_lmp(client: &Client) -> Check {
    let now = central_now();
    let rows = ctx(
        client
            .fetch(
                &Query::<RealTimeLmp>::new()
                    .sced_timestamp(now - Duration::hours(3), now + Duration::hours(1))
                    .settlement_point("HB_NORTH")
                    .size(50),
            )
            .await,
        "rt lmp",
    )?;
    ensure(!rows.records.is_empty(), || {
        "no LMP rows in the last 3h".into()
    })?;
    let last = &rows.records[0];
    Ok(format!(
        "{} SCED rows | HB_NORTH {} LMP {:.2} $/MWh",
        rows.meta.total_records, last.sced_timestamp, last.lmp
    ))
}

pub async fn actual_load(client: &Client) -> Check {
    // NP6-345-CD posts the finished day the next morning, so look two days back.
    let day = central_now().date() - Duration::days(2);
    let rows = ctx(
        client
            .fetch_all(&Query::<ActualLoad>::new().operating_day(day, day), 2)
            .await,
        "actual load",
    )?;
    ensure((23..=25).contains(&rows.len()), || {
        format!("{} rows for {day}", rows.len())
    })?;
    for r in &rows {
        ensure((r.zones.sum() - r.total).abs() < 5.0, || {
            format!("zones != total: {r:?}")
        })?;
        ensure((15_000.0..=120_000.0).contains(&r.total), || {
            format!("implausible {r:?}")
        })?;
    }
    let peak = rows
        .iter()
        .max_by(|a, b| a.total.total_cmp(&b.total))
        .expect("non-empty");
    Ok(format!(
        "{day}: peak {:.0} MW at HE{}",
        peak.total,
        peak.hour_ending.get()
    ))
}

pub async fn load_forecast(client: &Client) -> Check {
    let now = central_now();
    let page = ctx(
        client
            .fetch(
                &Query::<LoadForecast>::new()
                    .posted(now - Duration::hours(8), now + Duration::hours(1))
                    .in_use_only()
                    .size(200),
            )
            .await,
        "load forecast",
    )?;
    ensure(!page.records.is_empty(), || {
        "no in-use forecast rows in the last 8h".into()
    })?;
    ensure(page.records.iter().all(|r| r.in_use), || {
        "in_use_only returned unused rows".into()
    })?;
    let r = &page.records[0];
    ensure((10_000.0..=140_000.0).contains(&r.system_total), || {
        format!("implausible {r:?}")
    })?;
    Ok(format!(
        "{} rows | model {} posted {} for {} HE{}: {:.0} MW",
        page.meta.total_records,
        r.model,
        r.posted,
        r.delivery_date,
        r.hour_ending.get(),
        r.system_total
    ))
}

pub async fn wind_5min(client: &Client) -> Check {
    let page = ctx(
        client.fetch(&Query::<WindActual5Min>::new().size(3)).await,
        "wind",
    )?;
    ensure(!page.records.is_empty(), || "no wind rows".into())?;
    let r = &page.records[0];
    ensure((0.0..=40_000.0).contains(&r.gen_system_wide), || {
        format!("implausible {r:?}")
    })?;
    Ok(format!(
        "latest wind {} = {:.0} MW (HSL {:?})",
        r.interval_ending, r.gen_system_wide, r.hsl_system_wide
    ))
}

pub async fn solar_5min(client: &Client) -> Check {
    let page = ctx(
        client.fetch(&Query::<SolarActual5Min>::new().size(3)).await,
        "solar",
    )?;
    ensure(!page.records.is_empty(), || "no solar rows".into())?;
    let r = &page.records[0];
    ensure((0.0..=50_000.0).contains(&r.gen_system_wide), || {
        format!("implausible {r:?}")
    })?;
    Ok(format!(
        "latest solar {} = {:.0} MW (HSL {:?})",
        r.interval_ending, r.gen_system_wide, r.hsl_system_wide
    ))
}

pub async fn system_generation(client: &Client) -> Check {
    let page = ctx(
        client
            .fetch(&Query::<SystemGeneration>::new().size(3))
            .await,
        "generation",
    )?;
    ensure(!page.records.is_empty(), || "no generation rows".into())?;
    let r = &page.records[0];
    ensure((10_000.0..=120_000.0).contains(&r.se_mw), || {
        format!("implausible {r:?}")
    })?;
    Ok(format!(
        "state estimator {} = {:.0} MW",
        r.se_exe_time, r.se_mw
    ))
}

pub async fn fuel_mix(client: &Client) -> Check {
    let mix = ctx(client.fuel_mix().await, "fuel mix")?;
    let latest = mix.latest().ok_or("no snapshots")?;
    for fuel in ["Natural Gas", "Wind", "Solar", "Nuclear"] {
        ensure(latest.generation_mw.contains_key(fuel), || {
            format!("{fuel} missing")
        })?;
    }
    let gas = latest.generation_mw["Natural Gas"];
    let wind = latest.generation_mw["Wind"];
    ensure(gas >= 0.0 && wind >= 0.0, || {
        format!("negative gas/wind in {latest:?}")
    })?;
    Ok(format!(
        "{} snapshots, latest {}: gas {gas:.0} MW, wind {wind:.0} MW, net total {:.0} MW",
        mix.snapshots.len(),
        latest.at,
        latest.net_total_mw()
    ))
}

/// Downloads real archive documents on every call. ERCOT allows 3 downloads of the same report
/// per 12 months (https://www.ercot.com/help/terms/data-portal): do not run this repeatedly.
pub async fn archive(client: &Client) -> Check {
    let now = central_now();
    let listing = ctx(
        client
            .archives(
                "NP6-905-CD",
                &ArchiveQuery::new()
                    .posted_between(now - Duration::hours(6), now + Duration::hours(1))
                    .size(5),
            )
            .await,
        "archive listing",
    )?;
    let newest = listing
        .entries
        .first()
        .ok_or("no archive entries in the last 6h")?;
    let file = ctx(
        client.download_archive("NP6-905-CD", newest.doc_id).await,
        "archive download",
    )?;
    let files = ctx(file.extract(), "extract")?;
    let csv = files
        .iter()
        .find(|f| f.is_csv())
        .ok_or("no csv in archive")?;
    let text = ctx(csv.text(), "utf8")?;
    ensure(text.starts_with("DeliveryDate,"), || {
        format!("unexpected header: {}", &text[..text.len().min(80)])
    })?;
    let bulk = ctx(
        client
            .download_archives(
                "NP6-905-CD",
                &listing
                    .entries
                    .iter()
                    .take(2)
                    .map(|e| e.doc_id)
                    .collect::<Vec<_>>(),
            )
            .await,
        "bulk download",
    )?;
    let bulk_files = ctx(bulk.extract(), "bulk extract")?;
    Ok(format!(
        "{} docs listed; doc {} ({}) -> {} csv lines; bulk of {} -> {} files",
        listing.meta.total_records,
        newest.doc_id,
        newest.friendly_name,
        text.lines().count(),
        listing.entries.len().min(2),
        bulk_files.len()
    ))
}

pub async fn generic_query_all(client: &Client) -> Check {
    let day = yesterday().format("%Y-%m-%d").to_string();
    let merged = ctx(
        client
            .query_all(
                &ReportRequest::new("NP4-190-CD", "dam_stlmnt_pnt_prices")
                    .range("deliveryDate", &day, &day)
                    .filter("settlementPoint", "HB_NORTH")
                    .size(10),
                5,
            )
            .await,
        "generic query_all",
    )?;
    ensure((23..=25).contains(&merged.data.len()), || {
        format!("{} rows over pages of 10", merged.data.len())
    })?;
    Ok(format!(
        "{} rows merged across 3 pages of 10",
        merged.data.len()
    ))
}

pub async fn unknown_param(client: &Client) -> Check {
    let err = client
        .query(&ReportRequest::new("NP4-190-CD", "dam_stlmnt_pnt_prices").filter("notAParam", 1))
        .await
        .err()
        .ok_or("unknown parameter was accepted")?;
    match err {
        Error::Api {
            status: 400,
            unknown_params,
            ..
        } if unknown_params == ["notAParam"] => Ok("400 naming notAParam".into()),
        other => Err(format!("unexpected error: {other}")),
    }
}

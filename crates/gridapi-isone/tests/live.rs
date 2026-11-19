//! Live tests against www.iso-ne.com. They need network access and are skipped by
//! default: `cargo test -p gridapi-isone --test live -- --ignored --nocapture`.
//!
//! The keyless ISO Express tests need no account. The authenticated Web Services tests
//! are behind the `isone-webservices` feature (UNTESTED: there is no account); they print
//! `SKIPPED` without `ISONE_USERNAME` and `ISONE_PASSWORD`.
//!
//! NEVER run these in public CI: they hit ISO-NE's servers, which throttle excessive or invalid requests. Use your own credentials on your own machine.

// Live tests fail loudly with `expect`, and `assert!(x.is_empty())` reads best.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::collections::BTreeSet;

use gridapi_isone::{
    chrono::{Duration, NaiveDate, Utc},
    express::{Express, Market, Revision},
    LocationFilter, INTERNAL_HUB_ID,
};

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

fn express() -> Express {
    Express::builder()
        .user_agent("gridapi-isone-live-tests/0.1")
        .build()
        .expect("client builds")
}

fn hub() -> LocationFilter {
    LocationFilter::id(INTERNAL_HUB_ID)
}

fn yesterday() -> NaiveDate {
    Utc::now().date_naive() - Duration::days(2)
}

#[tokio::test]
#[ignore = "live network"]
async fn day_ahead_hub_2026_11_17() {
    let rows = express()
        .day_ahead_lmp(day(2026, 11, 17))
        .locations(hub())
        .fetch_strict()
        .await
        .expect("fetches");
    assert_eq!(rows.len(), 24);
    let he01 = &rows[0];
    println!("da-lmp 2026-11-17 hub HE01: {he01:?}");
    assert!((he01.energy + he01.congestion + he01.loss - he01.lmp).abs() < 0.011);
    assert_eq!(he01.location_name.as_deref(), Some(".H.INTERNAL_HUB"));
}

#[tokio::test]
#[ignore = "live network"]
async fn fall_back_day_has_25_distinct_hours() {
    let rows = express()
        .day_ahead_lmp(day(2026, 11, 1))
        .locations(hub())
        .fetch_strict()
        .await
        .expect("fetches");
    let instants: BTreeSet<_> = rows
        .iter()
        .map(|r| r.interval_start.with_timezone(&Utc))
        .collect();
    println!(
        "2026-11-01 hub hours: {} labels, {} instants",
        rows.len(),
        instants.len()
    );
    assert_eq!(rows.len(), 25);
    assert_eq!(instants.len(), 25);
    assert!(rows.iter().any(|r| r.hour_ending == "02X"));
}

#[tokio::test]
#[ignore = "live network"]
async fn spring_forward_day_has_23_distinct_hours() {
    let rows = express()
        .day_ahead_lmp(day(2026, 3, 8))
        .locations(hub())
        .fetch_strict()
        .await
        .expect("fetches");
    let instants: BTreeSet<_> = rows
        .iter()
        .map(|r| r.interval_start.with_timezone(&Utc))
        .collect();
    println!("2026-03-08 hub hours: {}", instants.len());
    assert_eq!(instants.len(), 23);
}

#[tokio::test]
#[ignore = "live network"]
async fn real_time_hourly_final_and_prelim() {
    let c = express();
    let d = yesterday();
    let prelim = c
        .real_time_hourly_lmp(d, Revision::Preliminary)
        .locations(LocationFilter::all().with_name(".H.INTERNAL_HUB"))
        .fetch_strict()
        .await
        .expect("prelim");
    println!(
        "rt hourly prelim {d} hub: {} rows, first {:?}",
        prelim.len(),
        prelim.first()
    );
    assert_eq!(prelim.len(), 24);

    let settled = day(2026, 11, 10);
    let fin = c
        .real_time_hourly_lmp(settled, Revision::Final)
        .locations(hub())
        .fetch_strict()
        .await
        .expect("final");
    println!(
        "rt hourly final {settled} hub: {} rows, first {:?}",
        fin.len(),
        fin.first()
    );
    assert_eq!(fin.len(), 24);
}

#[tokio::test]
#[ignore = "live network"]
async fn five_minute_final_hub_on_the_fall_back_day() {
    let rows = express()
        .five_minute_lmp(day(2026, 11, 1), Revision::Final)
        .locations(hub())
        .fetch_strict()
        .await
        .expect("fetches");
    let instants: BTreeSet<_> = rows
        .iter()
        .map(|r| r.timestamp.with_timezone(&Utc))
        .collect();
    println!(
        "2026-11-01 five-minute hub rows: {}, distinct instants {}",
        rows.len(),
        instants.len()
    );
    assert_eq!(rows.len(), 300, "25 hours x 12");
    assert_eq!(instants.len(), 300);
}

#[tokio::test]
#[ignore = "live network"]
async fn the_dynamic_reports_work_without_an_account() {
    let c = express();
    let d = yesterday();

    let latest = c
        .five_minute_lmp_latest(Revision::Preliminary, &hub())
        .await
        .expect("latest 5-min");
    println!("five-minute latest hub: {:?}", latest.last());
    assert!(!latest.is_empty());

    let hourly = c
        .hourly_lmp_latest(Market::RealTime, Revision::Preliminary, &hub())
        .await
        .expect("latest hourly");
    println!("hourly rt prelim latest hub: {:?}", hourly.last());
    assert!(!hourly.is_empty());

    let mix = c.fuel_mix(d).fetch_strict().await.expect("fuel mix");
    println!("fuel mix {d}: {} rows, first {:?}", mix.len(), mix.first());
    assert!(mix.len() > 1000);

    let load = c
        .hourly_system_load(d)
        .fetch_strict()
        .await
        .expect("hourly load");
    println!(
        "hourly system load {d}: {} rows, HE01 {:?}",
        load.len(),
        load.first()
    );
    assert_eq!(load.len(), 24);

    let five = c
        .five_minute_system_load(d)
        .fetch_strict()
        .await
        .expect("5 min load");
    println!(
        "five-minute load {d}: {} rows, first {:?}",
        five.len(),
        five.first()
    );
    assert_eq!(five.len(), 288);

    let da = c
        .day_ahead_demand(d)
        .fetch_strict()
        .await
        .expect("da demand");
    assert_eq!(da.len(), 24);

    let zonal = c.zonal_load(d).fetch_strict().await.expect("zonal");
    println!(
        "zonal load {d}: {} rows, first {:?}",
        zonal.len(),
        zonal.first()
    );
    assert!(zonal.len() >= 288 * 8);

    let rcp = c
        .regulation_prices(d, Revision::Preliminary)
        .fetch_strict()
        .await
        .expect("rcp");
    println!(
        "regulation prices {d}: {} rows, first {:?}",
        rcp.len(),
        rcp.first()
    );
    assert!(rcp.len() >= 280);

    let reserve = c
        .reserve_requirements(d)
        .fetch_strict()
        .await
        .expect("reserve");
    println!(
        "reserve requirements {d}: {} rows, first {:?}",
        reserve.len(),
        reserve.first()
    );
    assert!(reserve.len() >= 24);
}

#[tokio::test]
#[ignore = "live network"]
async fn a_future_day_is_missing_not_an_error() {
    let far = day(2035, 1, 1);
    let series = express()
        .day_ahead_lmp(far)
        .fetch()
        .await
        .expect("not an error");
    assert!(series.rows.is_empty());
    assert_eq!(series.missing, [far]);
}

#[cfg(feature = "isone-webservices")]
mod webservices_live {
    //! The authenticated Web Services route: UNTESTED (no account), behind a feature.
    use super::{day, express, hub};
    use gridapi_isone::{
        express::Revision,
        webservices::{Scope, WebServices, Window},
        Credentials, Error, INTERNAL_HUB_ID,
    };

    #[tokio::test]
    #[ignore = "live network"]
    async fn web_services_reject_bad_credentials_with_401() {
        let ws = WebServices::new(Credentials::new("gridapi-live-test", "not-a-real-password"))
            .expect("client builds");
        let err = ws
            .five_minute_lmp(
                Revision::Preliminary,
                Window::Current,
                Scope::Location(4000),
            )
            .await
            .expect_err("bad credentials");
        println!("web services with bad credentials: {err}");
        assert!(matches!(err, Error::Unauthorized { .. }), "got {err:?}");
    }

    fn credentialed() -> Option<WebServices> {
        match WebServices::from_env() {
            Ok(ws) => Some(ws),
            Err(e) => {
                eprintln!("SKIPPED: {e}; the authenticated API was NOT exercised");
                None
            }
        }
    }

    #[tokio::test]
    #[ignore = "needs ISONE_USERNAME and ISONE_PASSWORD (no account yet)"]
    async fn credentialed_hub_prices_and_locations() {
        let Some(ws) = credentialed() else { return };
        let current = ws
            .five_minute_lmp(
                Revision::Preliminary,
                Window::Current,
                Scope::Location(INTERNAL_HUB_ID),
            )
            .await
            .expect("current hub lmp");
        println!("web services five-minute hub: {current:?}");
        assert!(!current.is_empty());
        assert_eq!(current[0].location.id, INTERNAL_HUB_ID);

        let locations = ws.locations().await.expect("locations");
        println!("web services locations: {}", locations.len());
        assert!(locations.iter().any(|l| l.location_id == INTERNAL_HUB_ID));
    }

    #[tokio::test]
    #[ignore = "needs ISONE_USERNAME and ISONE_PASSWORD (no account yet)"]
    async fn credentialed_day_ahead_matches_the_public_csv() {
        let Some(ws) = credentialed() else { return };
        let d = day(2026, 11, 17);
        let api = ws
            .hourly_lmp(
                gridapi_isone::webservices::HourlyLmpFeed::DayAheadFinal,
                Window::Day(d),
                Scope::Location(INTERNAL_HUB_ID),
            )
            .await
            .expect("api");
        let csv = express()
            .day_ahead_lmp(d)
            .locations(hub())
            .fetch_strict()
            .await
            .expect("csv");
        assert_eq!(api.len(), csv.len());
        for (a, c) in api.iter().zip(&csv) {
            assert_eq!(a.begin_date, c.interval_start);
            assert!((a.lmp_total.expect("price") - c.lmp).abs() < 1e-9);
        }
    }
}

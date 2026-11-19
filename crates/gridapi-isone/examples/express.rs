//! Print rows from the keyless ISO Express reports.
//!
//! ```text
//! cargo run -p gridapi-isone --example express -- da-lmp 2026-11-17 4000
//! cargo run -p gridapi-isone --example express -- fivemin-final 2026-11-10 4000
//! cargo run -p gridapi-isone --example express -- fuelmix 2026-11-17
//! ```
//!
//! Reports: da-lmp, rt-final, rt-prelim, fivemin-final, fivemin-prelim, fuelmix,
//! hourly-load, da-demand, fivemin-load, zonal-load, regulation, reserves.
//! The optional third argument is a location id to keep.

use std::{env, process::ExitCode};

use gridapi_isone::{
    chrono::NaiveDate,
    express::{Express, Revision},
    Error, LocationFilter,
};

const SHOWN_ROWS: usize = 5;

fn show<T: std::fmt::Debug>(name: &str, rows: &[T], missing: &[NaiveDate]) {
    println!(
        "{name}: {} rows, {} missing day(s)",
        rows.len(),
        missing.len()
    );
    rows.iter()
        .take(SHOWN_ROWS)
        .for_each(|r| println!("  {r:?}"));
}

async fn run(report: &str, day: NaiveDate, filter: LocationFilter) -> Result<(), Error> {
    let c = Express::new()?;
    match report {
        "da-lmp" => {
            let s = c.day_ahead_lmp(day).locations(filter).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "rt-final" => {
            let s = c
                .real_time_hourly_lmp(day, Revision::Final)
                .locations(filter)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        "rt-prelim" => {
            let s = c
                .real_time_hourly_lmp(day, Revision::Preliminary)
                .locations(filter)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        "fivemin-final" => {
            let s = c
                .five_minute_lmp(day, Revision::Final)
                .locations(filter)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        "fivemin-prelim" => {
            let s = c
                .five_minute_lmp(day, Revision::Preliminary)
                .locations(filter)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        "fuelmix" => {
            let s = c.fuel_mix(day).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "hourly-load" => {
            let s = c.hourly_system_load(day).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "da-demand" => {
            let s = c.day_ahead_demand(day).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "fivemin-load" => {
            let s = c.five_minute_system_load(day).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "zonal-load" => {
            let s = c.zonal_load(day).locations(filter).fetch().await?;
            show(report, &s.rows, &s.missing);
        }
        "regulation" => {
            let s = c
                .regulation_prices(day, Revision::Preliminary)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        "reserves" => {
            let s = c
                .reserve_requirements(day)
                .locations(filter)
                .fetch()
                .await?;
            show(report, &s.rows, &s.missing);
        }
        other => eprintln!("unknown report {other:?}; see the file header for the list"),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match args.as_slice() {
        [report, date, rest @ ..] => NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|e| format!("bad date {date:?}: {e}"))
            .and_then(|d| {
                match rest {
                    [] => Ok(LocationFilter::all()),
                    [id] => id
                        .parse::<u32>()
                        .map(LocationFilter::id)
                        .map_err(|e| format!("bad location id {id:?}: {e}")),
                    _ => Err("too many arguments".to_owned()),
                }
                .map(|f| (report.clone(), d, f))
            }),
        _ => Err("usage: express <report> <YYYY-MM-DD> [location-id]".to_owned()),
    };
    match parsed {
        Ok((report, day, filter)) => match run(&report, day, filter).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

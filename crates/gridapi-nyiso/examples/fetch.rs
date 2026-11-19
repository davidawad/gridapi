//! Print the first rows of a NYISO dataset.
//!
//! ```text
//! cargo run -p gridapi-nyiso --example fetch -- damlbmp-zone 2026-11-17
//! cargo run -p gridapi-nyiso --example fetch -- fuelmix 2026-11-01 2026-11-03
//! ```
//!
//! Datasets: damlbmp-zone, damlbmp-gen, realtime-zone, realtime-gen, rtlbmp-zone,
//! rtlbmp-gen, pal, palintegrated, isolf, fuelmix, damasp, rtasp, flows.

use std::{env, process::ExitCode};

use gridapi_nyiso::{chrono::NaiveDate, Client, Error, Level};

const SHOWN_ROWS: usize = 5;

fn parse_day(arg: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(arg, "%Y-%m-%d").map_err(|e| format!("bad date {arg:?}: {e}"))
}

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

async fn run(dataset: &str, start: NaiveDate, end: NaiveDate) -> Result<(), Error> {
    let client = Client::new()?;
    match dataset {
        "damlbmp-zone" => {
            let s = client
                .day_ahead_lbmp(Level::Zone, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "damlbmp-gen" => {
            let s = client
                .day_ahead_lbmp(Level::Generator, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "realtime-zone" => {
            let s = client
                .real_time_lbmp(Level::Zone, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "realtime-gen" => {
            let s = client
                .real_time_lbmp(Level::Generator, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "rtlbmp-zone" => {
            let s = client
                .integrated_real_time_lbmp(Level::Zone, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "rtlbmp-gen" => {
            let s = client
                .integrated_real_time_lbmp(Level::Generator, start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "pal" => {
            let s = client.actual_load(start).through(end).fetch().await?;
            show(dataset, &s.rows, &s.missing);
        }
        "palintegrated" => {
            let s = client.integrated_load(start).through(end).fetch().await?;
            show(dataset, &s.rows, &s.missing);
        }
        "isolf" => {
            let s = client.load_forecast(start).through(end).fetch().await?;
            show(dataset, &s.rows, &s.missing);
        }
        "fuelmix" => {
            let s = client.fuel_mix(start).through(end).fetch().await?;
            show(dataset, &s.rows, &s.missing);
        }
        "damasp" => {
            let s = client
                .day_ahead_ancillary(start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "rtasp" => {
            let s = client
                .real_time_ancillary(start)
                .through(end)
                .fetch()
                .await?;
            show(dataset, &s.rows, &s.missing);
        }
        "flows" => {
            let s = client.interface_flows(start).through(end).fetch().await?;
            show(dataset, &s.rows, &s.missing);
        }
        other => eprintln!("unknown dataset {other:?}; see the file header for the list"),
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match args.as_slice() {
        [dataset, start] => parse_day(start).map(|d| (dataset, d, d)),
        [dataset, start, end] => {
            parse_day(start).and_then(|s| parse_day(end).map(|e| (dataset, s, e)))
        }
        _ => Err("usage: fetch <dataset> <YYYY-MM-DD> [<YYYY-MM-DD>]".to_owned()),
    };
    match parsed {
        Ok((dataset, start, end)) => match run(dataset, start, end).await {
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

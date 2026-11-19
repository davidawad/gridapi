//! Prints day-ahead hub LMPs for one Pacific day plus the live fuel mix.
//!
//! `cargo run -p gridapi-caiso --example hub_prices -- 2026-11-10`

use chrono::{Days, NaiveDate, TimeZone};
use chrono_tz::America::Los_Angeles;
use gridapi_caiso::{pivot_lmp, Client, Error, OutlookDataset, TradingHub};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let day: NaiveDate = std::env::args().nth(1).map_or_else(
        || NaiveDate::from_ymd_opt(2026, 11, 10).ok_or("invalid default date".to_owned()),
        |arg| {
            arg.parse::<NaiveDate>()
                .map_err(|e| format!("date {arg:?}: {e}"))
        },
    )?;
    let midnight = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .and_then(|t| Los_Angeles.from_local_datetime(&t).earliest())
            .map(|t| t.with_timezone(&chrono::Utc))
            .ok_or_else(|| Error::InvalidQuery(format!("no Pacific midnight for {d}")))
    };
    let start = midnight(day)?;
    let end = midnight(day.checked_add_days(Days::new(1)).ok_or("date overflow")?)?;

    let client = Client::builder()
        .user_agent("gridapi-caiso-example/0.1")
        .build()?;
    let rows = client
        .day_ahead_lmp(&TradingHub::all_node_ids(), start, end)
        .await?;
    for point in pivot_lmp(rows) {
        println!(
            "{} {:<18} lmp={:>8.2} energy={:>8.2} congestion={:>7.2} loss={:>6.2} ghg={:>6.2}",
            point.interval_start.format("%Y-%m-%dT%H:%MZ"),
            point.node,
            point.lmp.unwrap_or(f64::NAN),
            point.energy.unwrap_or(f64::NAN),
            point.congestion.unwrap_or(f64::NAN),
            point.loss.unwrap_or(f64::NAN),
            point.ghg.unwrap_or(f64::NAN),
        );
    }

    let fuel = client.outlook(OutlookDataset::FuelSource).await?;
    if let Some(now) = fuel.latest() {
        println!(
            "\nfuel mix at {} {} (Pacific), MW:",
            fuel.date, now.local_time
        );
        for (name, value) in fuel.columns.iter().zip(&now.values) {
            println!("  {name:<12} {value:?}");
        }
    }
    Ok(())
}

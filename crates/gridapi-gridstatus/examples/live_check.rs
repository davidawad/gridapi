//! Smoke check against the real API using at most 2 requests.
//!
//! ```text
//! GRIDSTATUS_API_KEY=... cargo run --example live_check
//! ```
//!
//! Mirrors `tests/live.rs` for environments where the test harness cannot run.
//! The client's request budget is 2, so this can never spend more.

use gridapi_gridstatus::{Client, Query};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .api_key_from_env()
        .request_budget(2)
        .build()?;

    let ercot = client
        .ercot_spp_day_ahead_hourly(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T01:00:00Z")
                .location("HB_HOUSTON"),
        )
        .await?;
    for row in &ercot.rows {
        println!(
            "ercot_spp_day_ahead_hourly {} {} {:?}",
            row.interval_start_utc, row.location, row.spp
        );
    }
    println!("ercot response meta: {:?}", ercot.response);

    let caiso = client
        .caiso_lmp_day_ahead_hourly(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T01:00:00Z")
                .limit(3),
        )
        .await?;
    for row in &caiso.rows {
        println!(
            "caiso_lmp_day_ahead_hourly {} {} lmp={:?} energy={:?} congestion={:?} loss={:?} ghg={:?}",
            row.interval_start_utc, row.location, row.lmp, row.energy, row.congestion, row.loss, row.ghg
        );
    }
    println!("requests sent: {}", client.requests_sent());
    Ok(())
}

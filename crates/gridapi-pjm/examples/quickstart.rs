//! Western Hub day-ahead prices for one day, plus the feed count.
//!
//! ```text
//! cargo run -p gridapi-pjm --example quickstart -- 2026-11-17
//! ```
//!
//! Needs `PJM_API_KEY` (register at <https://apiportal.pjm.com/>); there is no
//! keyless default.
//!
//! ```text
//! PJM_API_KEY=... cargo run -p gridapi-pjm --example quickstart
//! ```

use chrono::NaiveDate;
use gridapi_pjm::{Client, LmpRequest};

const WESTERN_HUB: u64 = 51288;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let day = std::env::args()
        .nth(1)
        .map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()?
        .unwrap_or(NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("bad default date")?);

    // Reads PJM_API_KEY; fails with Error::MissingKey when it is unset.
    let client = Client::new()?;
    println!("{} feeds in the catalog", client.feeds().await?.len());

    let rows = client
        .da_hrl_lmps(LmpRequest::new(day).pnode_id(WESTERN_HUB))
        .await?;
    for r in &rows {
        println!(
            "{} {}  {:>8.2} $/MWh  (energy {:.2} + congestion {:.2} + loss {:.2})",
            r.pnode_name,
            r.datetime_beginning_ept,
            r.total_lmp_da,
            r.system_energy_price_da,
            r.congestion_price_da,
            r.marginal_loss_price_da
        );
    }
    Ok(())
}

//! Renders one day of day-ahead zonal prices from a recorded NYISO file, served by a local
//! mock server so it runs offline: `cargo run -p gridapi-nyiso --example showcase`.

use gridapi_nyiso::{chrono::NaiveDate, Client, Level, Source};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = format!(
        "{}/tests/fixtures/damlbmp_zone_20261117.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/damlbmp/20261117damlbmp_zone.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(std::fs::read(fixture)?))
        .mount(&server)
        .await;

    let client = Client::builder().base_url(server.uri()).build()?;
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("bad date")?;
    let rows = client
        .day_ahead_lbmp(Level::Zone, day)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await?;

    println!("NYISO day-ahead LBMP, {day} (Eastern), $/MWh");
    println!(
        "{:<18} {:<8} {:>8} {:>8} {:>10} {:>8}",
        "interval start", "zone", "LBMP", "energy", "congestion", "losses"
    );
    for r in &rows {
        println!(
            "{:<18} {:<8} {:>8.2} {:>8.2} {:>10.2} {:>8.2}",
            r.timestamp.format("%Y-%m-%d %H:%M"),
            r.name,
            r.lbmp,
            r.energy_component(),
            r.marginal_cost_congestion,
            r.marginal_cost_losses,
        );
    }
    Ok(())
}

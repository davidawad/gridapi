# gridapi-spp

Async Rust client for [SPP](https://www.spp.org) (Southwest Power Pool) public market data. It reads
the anonymous file browser on `portal.spp.org` (the successor of `marketplace.spp.org`): LMPs by
settlement location, hubs, load, generation mix and the dashboard chart feeds. Typed responses,
jittered retry, no global state, no account.

## How the portal works

The portal's web UI is a React app over a small Java API. Findings from reading its bundle and
probing it:

- `GET /file-browser-api/?fsName=<source>&path=<folder>&type=folder` lists a folder (JSON array).
- `GET /file-browser-api/download/<source>?path=<file>` downloads a file.
- `<source>` is the page slug from `GET /api/menu/` (`pageType == "file-browser"`);
  `Client::file_browser_pages` returns all of them (about 100 public sources).
- The UI sends an `X-SPP-UI-Token` header, but the token is only populated for signed-in users
  (`/api/principal` reports `unauthenticatedUser: true, uiTokenPresent: false` anonymously).
  The public tree answers anonymous requests with or without the header, so no token or cookie
  handling is needed. A wrong `fsName` answers `404` with an empty body.
- `GET /chart-api/<chart>/asChart` serves the data behind the dashboard charts (`gen-mix`,
  `load-forecast`, `ace`, and the `-swpw` variants of the last two), also anonymously.
  `gen-mix-swpw` answers 404 (the SWPW mix is in the generation-mix files);
  `Chart::GenMixSwpw` is behind the `unverified` feature.

## Usage

```rust,no_run
use gridapi_spp::{at_location, Client};

#[tokio::main]
async fn main() -> Result<(), gridapi_spp::Error> {
    let spp = Client::new()?;

    // Newest real-time five-minute LMP file, filtered to one hub.
    let (file, prices) = spp.latest_rt_lmp_by_location().await?;
    for p in at_location(&prices, "SPPNORTH_HUB") {
        println!("{} {} ${:.2}/MWh", file.name, p.settlement_location, p.lmp);
    }

    // Day-ahead hourly LMPs for an operating day.
    let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
    let da = spp.da_lmp_by_location(date).await?;
    println!("{} rows", da.len());
    Ok(())
}
```

`Client::builder()` takes `base_url`, `user_agent`, `timeout` and `retry` ([`RetryPolicy`]: 4 attempts,
full-jitter exponential backoff on HTTP 429, 5xx and connection errors, `Retry-After` honoured).

## Covered

| Method | Source and path |
| --- | --- |
| `da_lmp_by_location(date)`, `latest_da_lmp_by_location()` | `da-lmp-by-settlement-location` `/YYYY/MM/By_Day/DA-LMP-SL-YYYYMMDD0100.csv`: hourly, every settlement location, hubs included |
| `rt_lmp_by_location(interval_end)`, `latest_rt_lmp_by_location()` | `rtbm-lmp-by-location` `/YYYY/MM/By_Interval/DD/RTBM-LMP-SL-YYYYMMDDHHMM.csv`: five-minute |
| `rt_lmp_daily(date)` | `rtbm-lmp-by-location` `/YYYY/MM/By_Day/RTBM-LMP-DAILY-SL-YYYYMMDD.csv` (about 50 MB, posted a few days late) |
| `hourly_load_daily(date)`, `hourly_load_monthly(y, m)` | `hourly-load` daily (wide) and monthly (long) files |
| `generation_mix(area, window)` | `generation-mix-historical` `/SPP\|SWPW/GenMix{2Hour,YTD,365}_*.csv`, with system load |
| `chart(Chart::{GenMix, LoadForecast, Ace, ...})` | `/chart-api/<chart>/asChart` |
| `list`, `download`, `newest_file`, `file_browser_pages` | the file browser itself, for every other source (binding constraints, MCPs, fuel on margin, estimated prices, TCR, WEIS, modeling reports, ...) |

Hubs: `SPPNORTH_HUB`, `SPPSOUTH_HUB` (`SPP_HUBS`), and `CRSP_HUB` and other `*_HUB` locations in the
`SWPW` (Western Energy Imbalance Service) balancing area; `LocationalPrice::is_hub` tests the suffix.
Intervals are Central Prevailing Time as printed plus a UTC column; both are kept.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **21 of 21 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::list (+ ClientBuilder::user_agent / build, Client::base_url) | file browser listing da-lmp-by-settlement-location / | 15 entries | 11-19 02:28Z | pass |
| Client::file_browser_pages | portal menu: every file-browser page | 119 public file-browser trees | 11-19 02:28Z | pass |
| Client::newest_file | newest file in da-lmp-by-settlement-location By_Day | decoded | 11-19 02:28Z | pass |
| Client::download | raw bytes of the newest day-ahead file | 4275880 bytes | 11-19 02:28Z | pass |
| Client::da_lmp_by_location (+ at_location, SPP_HUBS, LocationalPrice::is_hub) | DA-LMP-SL for 2026-11-17, hubs | 39024 rows | 11-19 02:28Z | pass |
| Client::latest_da_lmp_by_location | newest DA-LMP-SL file in By_Day | 39024 rows | 11-19 02:28Z | pass |
| Client::latest_rt_lmp_by_location | newest RTBM-LMP-SL five-minute file in By_Interval | 1626 rows | 11-19 02:28Z | pass |
| Client::rt_lmp_by_location | RTBM-LMP-SL for the interval ending 2026-11-18 20:30:00 CPT | 1626 rows, same as the newest-file listing | 11-19 02:28Z | pass |
| Client::rt_lmp_daily | RTBM-LMP-DAILY-SL roll-up (about 50 MB), newest posted day among 3..10 days back | 2026-11-13: 468288 rows; SPPNORTH_HUB 288 five-minute intervals | 11-19 02:29Z | pass |
| Client::hourly_load_monthly | HOURLY_LOAD-202610.csv | 20088 rows; 20 zones | 11-19 02:29Z | pass |
| Client::hourly_load_daily | daily HOURLY_LOAD file, newest posted day among 1..10 days back | 672 rows | 11-19 02:29Z | pass |
| Client::generation_mix (+ GenMixSample::total_mw / fuel) | Spp TwoHour | 24 samples | 11-19 02:29Z | pass |
| Client::generation_mix (+ GenMixSample::total_mw / fuel) | Swpw TwoHour | 24 samples | 11-19 02:29Z | pass |
| Client::generation_mix (+ GenMixSample::total_mw / fuel) | Spp YearToDate | 92414 samples | 11-19 02:30Z | pass |
| Client::generation_mix (+ GenMixSample::total_mw / fuel) | Swpw Rolling365 | 66627 samples | 11-19 02:30Z | pass |
| Client::chart (+ ChartData::latest / series_named, Chart::slug) | chart-api/gen-mix/asChart | 24 timestamps | 11-19 02:30Z | pass |
| Client::chart (+ ChartData::latest / series_named, Chart::slug) | chart-api/load-forecast/asChart | 499 timestamps | 11-19 02:30Z | pass |
| Client::chart (+ ChartData::latest / series_named, Chart::slug) | chart-api/load-forecast-swpw/asChart | 499 timestamps | 11-19 02:30Z | pass |
| Client::chart (+ ChartData::latest / series_named, Chart::slug) | chart-api/ace/asChart | 121 timestamps | 11-19 02:30Z | pass |
| Client::chart (+ ChartData::latest / series_named, Chart::slug) | chart-api/ace-swpw/asChart | 121 timestamps | 11-19 02:30Z | pass |
| Client::list (other sources: hourly-load, rtbm-lmp-by-location, generation-mix-historical) | list three more public trees | 331 entries | 11-19 02:30Z | pass |

portal.spp.org is not reachable from the cluster pods (TCP connect timeouts), which is why the
runner is a binary that runs on a workstation.

**Bug found and fixed by this run.** `newest_file` (and so `latest_da_lmp_by_location`,
`latest_rt_lmp_by_location`) picked the portal root's `RePrice` folder as the newest year, because
a plain string sort puts it after `2026`; the first live pass failed with `nothing found under
/RePrice/*/By_Day`. Only all-digit year and month folders count now, with a regression fixture
(`tests/fixtures/list_root_with_reprice.json`, a synthetic root listing with the same shape) and unit tests.

**Gated behind the `unverified` cargo feature:** `Chart::GenMixSwpw`. `chart-api/gen-mix-swpw/asChart`
answers HTTP 404 on the live portal (every other chart slug answers 200). The portal's own page
requests that chart in the echarts shape at `/asChart/line` (and as CSV at `/asFile`), which this
crate does not parse. The same data is served and verified through
`generation_mix(BalancingArea::Swpw, ..)` (rows above), so nothing is lost.

## Development

```text
cargo test -p gridapi-spp                          # offline fixture tests
cargo test -p gridapi-spp -- --ignored --nocapture # live checks against the portal
```

## License

GPL-3.0-or-later; see [LICENSE](../../LICENSE).
Not affiliated with SPP; check SPP's terms of use (the portal's acceptable-use policy) for the data you republish.

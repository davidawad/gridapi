# gridapi-nyiso

Async Rust client for NYISO's public market data. No account and no API key:
NYISO publishes everything as CSV files on `mis.nyiso.com`. This crate hides the
file naming, the switch from daily files to monthly zip archives for older days,
and the Eastern-time quirks (including the repeated hour on the fall-back day),
and returns typed rows.

- Async `reqwest` (rustls), typed request builders, typed rows, `thiserror` errors.
- Retries transport errors, HTTP 429 and 5xx with jittered exponential backoff
  (honours `Retry-After`).
- Configurable base URL, `User-Agent`, timeout and retry policy. No global state.
- Structured `tracing` events; no output unless you install a subscriber.
- License: GPL-3.0-or-later.

## Usage

```rust
use gridapi_nyiso::{chrono::NaiveDate, Client, Level};

#[tokio::main]
async fn main() -> Result<(), gridapi_nyiso::Error> {
    let client = Client::new()?;
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");

    // Day-ahead hourly LBMP by zone.
    let prices = client.day_ahead_lbmp(Level::Zone, day).fetch_strict().await?;
    let capitl = prices.iter().find(|p| p.name == "CAPITL").expect("CAPITL row");
    println!("{} {} ${}/MWh", capitl.timestamp, capitl.name, capitl.lbmp);
    // 2026-11-17T00:00:00-05:00 CAPITL $<price>/MWh

    // A range of days. Days older than about a week come from the monthly
    // archive, one download per month; a day NYISO never published is reported
    // in `missing` instead of failing the request.
    let first = NaiveDate::from_ymd_opt(2026, 11, 1).expect("valid date");
    let mix = client.fuel_mix(first).through(day).fetch().await?;
    println!("{} rows, {} days missing", mix.rows.len(), mix.missing.len());
    Ok(())
}
```

A runnable command-line example is in `examples/fetch.rs`:

```text
cargo run -p gridapi-nyiso --example fetch -- damlbmp-zone 2026-11-17
cargo run -p gridapi-nyiso --example fetch -- fuelmix 2026-11-01 2026-11-03
```

## Datasets

| Method | NYISO dataset (`mis.nyiso.com/public/csv/<dir>/`) | Interval | Row type |
|---|---|---|---|
| `day_ahead_lbmp(Level, day)` | `damlbmp` zone / gen | hourly | `Lbmp` |
| `real_time_lbmp(Level, day)` | `realtime` zone / gen | 5 minutes | `Lbmp` |
| `integrated_real_time_lbmp(Level, day)` | `rtlbmp` zone / gen | hourly | `Lbmp` |
| `actual_load(day)` | `pal` | 5 minutes | `Load` |
| `integrated_load(day)` | `palIntegrated` | hourly | `Load` |
| `load_forecast(day)` | `isolf` | hourly | `LoadForecast` |
| `fuel_mix(day)` | `rtfuelmix` | 5 minutes | `FuelMix` |
| `day_ahead_ancillary(day)` | `damasp` | hourly | `AncillaryPrice` |
| `real_time_ancillary(day)` | `rtasp` | 5 minutes | `AncillaryPrice` |
| `interface_flows(day)` | `ExternalLimitsFlows` | 5 minutes | `InterfaceFlow` |

`Client::raw_csv` returns the untouched CSV bytes of one day for any `Dataset`.

## File layout this crate follows

- Daily file: `<base>/<dir>/<YYYYMMDD><dir>[_zone|_gen].csv`
- Monthly archive: `<base>/<dir>/<YYYYMM01><dir>[_zone|_gen]_csv.zip`, holding one
  daily CSV per day under the daily file's name. LBMP archives carry the `_zone` /
  `_gen` suffix; the other datasets do not.
- NYISO keeps daily files for roughly the last week. `Source::Auto` (the default)
  reads the daily file for recent days and the archive for older ones, and falls
  through to the other route on a 404. The current month's archive exists too,
  so a day that has aged out of the daily window is still reachable. A request
  for several days opens each month's archive once.

## Time

Timestamps are `DateTime<FixedOffset>` in Eastern time: `-05:00` in winter,
`-04:00` in summer. Files with a `Time Zone` column (`pal`, `palIntegrated`,
`rtfuelmix`, `damasp`, `rtasp` from 2016) are read from it. For the others
(`damlbmp`, `realtime`, `rtlbmp`, `isolf`, `ExternalLimitsFlows`) the repeated
hour on the fall-back day is told apart by row order within each series: first
pass EDT, second pass EST. A 25-hour day therefore has 25 distinct instants and
a 23-hour day has 23.

## Known limits

- Ancillary prices (`damasp`, `rtasp`) are decoded in the per-zone layout NYISO
  has used since 2017. Earlier files use regional East/West or SENY columns and
  return `Error::UnsupportedLayout`; use `Client::raw_csv` for those.
- `ExternalLimitsFlows` passes NYISO's "no limit" markers (`-9999`) through as is.
- Old headers are truncated and drift in wording; columns are matched on the
  normalised header text, so 2001-era files decode with the same types.
- Data volume: a day of real-time generator LBMP is about 14 MB of CSV and
  220,000 rows.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **18 of 18 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::new + Client::day_ahead_lbmp(Zone) + Request::fetch_strict + Lbmp::energy_component | damlbmp zone 2026-11-17 | 360 rows | 11-19 02:28Z | pass |
| Client::day_ahead_lbmp(Generator) | damlbmp gen 2026-11-17 | 18024 rows | 11-19 02:28Z | pass |
| Client::real_time_lbmp | realtime Zone 2026-11-17 | 4410 rows | 11-19 02:28Z | pass |
| Client::integrated_real_time_lbmp | rtlbmp Zone 2026-11-17 | 360 rows | 11-19 02:28Z | pass |
| Client::real_time_lbmp | realtime Generator 2026-11-17 | 220794 rows | 11-19 02:28Z | pass |
| Client::integrated_real_time_lbmp | rtlbmp Generator 2026-11-17 | 18024 rows | 11-19 02:28Z | pass |
| Client::actual_load | pal 2026-11-17 | 3234 rows | 11-19 02:28Z | pass |
| Client::integrated_load | palIntegrated 2026-11-17 | 264 rows | 11-19 02:28Z | pass |
| Client::load_forecast | isolf 2026-11-17 | 144 rows; 11 zones | 11-19 02:28Z | pass |
| Client::fuel_mix | rtfuelmix 2026-11-17 | 2058 rows | 11-19 02:28Z | pass |
| Client::day_ahead_ancillary | damasp 2026-11-17 | 264 rows | 11-19 02:28Z | pass |
| Client::real_time_ancillary | rtasp 2026-11-17 | 3234 rows | 11-19 02:28Z | pass |
| Client::interface_flows | ExternalLimitsFlows 2026-11-17 | 5529 rows | 11-19 02:28Z | pass |
| Request::source(ArchiveOnly) on an old day | damlbmp zone 2025-01-15 from the monthly archive | 360 rows | 11-19 02:28Z | pass |
| Request::source(DailyOnly) on a recent day | fuel mix 2026-11-17 from the daily file | 2058 rows from the daily file | 11-19 02:28Z | pass |
| Request::through + Request::fetch (Series::rows / missing), range across a month boundary | rtfuelmix 2025-06-29..2025-07-02 | 8267 rows over 4 days, 0 missing days | 11-19 02:28Z | pass |
| Request::fetch (a day NYISO never published is 'missing', not an error) | damlbmp zone 2035-01-01 | 0 rows, missing = [2035-01-01] | 11-19 02:28Z | pass |
| Client::raw_csv + daily_url + archive_url | raw damlbmp zone CSV for 2026-11-17 | 16845 bytes | 11-19 02:28Z | pass |

Nothing is feature-gated: every method passed. The archive and fall-back-day behaviours (2001
archive, 25-hour fall-back day) are covered by `tests/live.rs`.

## Tests

```text
cargo test -p gridapi-nyiso                                  # fixtures + mock server
cargo test -p gridapi-nyiso --test live -- --ignored --nocapture   # live network
```

Fixtures in `tests/fixtures/` are synthetic: the real response shapes, field names and timestamps with invented values (the upstream terms restrict redistributing their data).

## Not affiliated

This is an independent client for publicly posted files. It is not affiliated with
or endorsed by the New York Independent System Operator. Check NYISO's terms of use
before heavy automated retrieval, and keep the default retry backoff in place.

# gridapi-caiso

Async Rust client for CAISO's public data, no API key needed:

- the **OASIS** `SingleZip` API (`oasis.caiso.com/oasisapi/SingleZip`): day-ahead,
  real-time and 15-minute LMPs, load forecasts, system schedules, ancillary
  service prices, node lists;
- the **Today's Outlook** CSV feeds (`caiso.com/outlook/current/*.csv`): fuel mix,
  demand, net demand, storage, CO2, plus the dated archive.

License: GPL-3.0-or-later.

## Usage

```rust
use chrono::{TimeZone, Utc};
use gridapi_caiso::{pivot_lmp, Client, OutlookDataset, TradingHub};

#[tokio::main]
async fn main() -> Result<(), gridapi_caiso::Error> {
    // Put a contact in the User-Agent; CAISO asks API users to be identifiable.
    let client = Client::builder()
        .user_agent("my-app/1.0 (me@example.com)")
        .build()?;

    // Day-ahead hub prices for one Pacific day (08:00Z = midnight PST).
    let start = Utc.with_ymd_and_hms(2026, 11, 10, 8, 0, 0).single().unwrap_or_default();
    let end = Utc.with_ymd_and_hms(2026, 11, 11, 8, 0, 0).single().unwrap_or_default();
    let rows = client
        .day_ahead_lmp(&TradingHub::all_node_ids(), start, end)
        .await?;
    for p in pivot_lmp(rows) {
        println!("{} {} lmp={:?} cong={:?}", p.interval_start, p.node, p.lmp, p.congestion);
    }

    // Live system state from Today's Outlook (5-minute, Pacific time).
    let fuel = client.outlook(OutlookDataset::FuelSource).await?;
    if let Some(now) = fuel.latest_in("Natural Gas") {
        println!("gas {:?} MW at {}", fuel.value(now, "Natural Gas"), now.local_time);
    }
    Ok(())
}
```

Anything without a typed helper goes through the generic builder:

```rust
use gridapi_caiso::{Client, Query, QueryName, MarketRun};

# async fn run(client: Client, start: chrono::DateTime<chrono::Utc>, end: chrono::DateTime<chrono::Utc>)
# -> Result<(), gridapi_caiso::Error> {
let query = Query::builder(QueryName::Other("SLD_REN_FCST".into()), start, end)
    .version(1)
    .market_run(MarketRun::Dam)
    .build()?;
let rows = client.fetch_all(&query).await?;       // Vec<Record>: column map per row
println!("{:?}", rows.first().and_then(|r| r.get("MW")));
# Ok(()) }
```

Implement `FromRecord` for your own row type to get `client.fetch::<T>(&query)`.

## Endpoints covered

| Helper | OASIS report | Resolution | Notes |
| --- | --- | --- | --- |
| `day_ahead_lmp` | `PRC_LMP` v12, `DAM` | hourly | LMP, congestion, energy, loss, GHG rows per node |
| `real_time_lmp` | `PRC_INTVL_LMP` v3, `RTM` | 5 min | |
| `rtpd_lmp` | `PRC_RTPD_LMP` v3, `RTPD` | 15 min | |
| `load_forecast` | `SLD_FCST` v1 | hourly | per TAC area; `DAM`, `2DA`, `7DA`, `RTM` runs |
| `system_schedules` | `ENE_SLRS` v1 | hourly | `Caiso_Totals` generation, imports, exports |
| `ancillary_prices` | `PRC_AS` v1 | hourly | RU / RD / SR / NR by region; `ALL` handled via the group-download redirect |
| `pnodes` / `apnodes` | `ATL_PNODE` / `ATL_APNODE` v1 | n/a | node lists (the PNode list is about 3 MB zipped) |
| `outlook` / `outlook_on` | `current/*.csv`, `history/<date>/*.csv` | 5 min | `fuelsource`, `demand`, `netdemand`, `storage`, `co2` |
| `fetch`, `fetch_all`, `fetch_records` | any `queryname` | | generic builder, chunking, typed or raw rows |

Trading hubs: `TradingHub::{Np15, Sp15, Zp26}` map to `TH_NP15_GEN-APND`,
`TH_SP15_GEN-APND`, `TH_ZP26_GEN-APND`.

## OASIS behaviour handled

- **Errors arrive as HTTP 200.** A failed query returns a zip holding an XML
  `ERROR` document (`1000` no data, `1004` period over 31 days). It surfaces as
  `Error::Oasis { code, description }`; `Error::is_no_data()` tests for 1000.
- **Pacing.** OASIS allows about one request per 5 s and answers 429 beyond
  that. Every `Client` spaces its own requests (default 5.25 s,
  `ClientBuilder::min_interval`). The pacer is a lock-free per-client atomic: no
  global lock, and two clients do not share a budget.
- **Retries.** Transport errors and timeouts, 429, 5xx and non-zip bodies are
  retried with exponential backoff and jitter (`RetryPolicy`, default 5 attempts,
  2 s base, 60 s cap). A 429 also pushes the client's pacer out. OASIS error
  documents and 4xx are not retried. Timeouts default to 180 s because OASIS is
  slow and some cloud egress ranges stall.
- **31-day limit.** `fetch_all` and all typed helpers split `[start, end)` into
  contiguous windows of at most 30 days. Windows that return "no data" are
  skipped; if every window is empty the "no data" error is returned (a mistyped
  node name also produces it).
- **Group downloads.** `anc_type=ALL` or `anc_region=ALL` makes `SingleZip`
  redirect to `GroupZip`, which returns the whole Pacific day for the
  `startdatetime`. `ancillary_prices` follows it one Pacific day at a time and
  filters back to your window.
- **Column drift.** The value column is `MW` in `PRC_LMP`, `VALUE` in
  `PRC_INTVL_LMP` and `PRC` in `PRC_RTPD_LMP`; typed rows hide that. CSV and XML
  (`resultformat=1`) both decode.
- **Zip safety.** Decompressed output is capped at 512 MiB and response bodies at
  256 MiB by default.

Timestamps in requests and responses are UTC; the CAISO operating date and the
Outlook feeds are Pacific time.

## Tests

```
cargo test -p gridapi-caiso                 # fixtures + local scripted-server tests, no network
cargo test -p gridapi-caiso --test live -- --ignored --nocapture   # live: oasis.caiso.com and caiso.com (paced, ~1-2 min)
```

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **20 of 20 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::day_ahead_lmp + pivot_lmp + TradingHub::all_node_ids / node_id | PRC_LMP DAM, the three hubs, 2026-11-10 Pacific day | 360 component rows; 72 hub | 11-19 02:28Z | pass |
| Client::real_time_lmp | PRC_INTVL_LMP RTM, NP15, first hour of 2026-11-10 | 60 component rows | 11-19 02:28Z | pass |
| Client::rtpd_lmp | PRC_RTPD_LMP RTPD, SP15, first two hours of 2026-11-10 | 40 component rows | 11-19 02:28Z | pass |
| Client::load_forecast | SLD_FCST DAM, CA ISO-TAC, 2026-11-10 | decoded | 11-19 02:28Z | pass |
| Client::system_schedules | ENE_SLRS DAM, all zones, first two hours of 2026-11-10 | 60 rows | 11-19 02:28Z | pass |
| Client::ancillary_prices (single product and region) | PRC_AS DAM, RU, AS_CAISO_EXP, 2026-11-10 | decoded | 11-19 02:28Z | pass |
| Client::ancillary_prices (AncillaryType::All, the grouped redirect form) | PRC_AS DAM, ALL products, ALL regions, 2026-11-10 | 624 rows across regions {"AS_CAISO", "AS_CAISO_EXP", "AS_NP26", "AS_NP26_EXP", "AS_SP26", "AS_SP26_EXP"} | 11-19 02:28Z | pass |
| Client::apnodes | ATL_APNODE as of 2026-11-10 | decoded | 11-19 02:29Z | pass |
| Client::pnodes | ATL_PNODE as of 2026-11-10 (3 MB CSV in a zip) | decoded | 11-19 02:29Z | pass |
| Client::fetch_records (raw, Query::builder / QueryBuilder market_run / node / build) | PRC_LMP DAM TH_NP15_GEN-APND, 2026-11-10 hour 1 | decoded | 11-19 02:29Z | pass |
| Client::fetch_all (chunking across the 31-day limit) | PRC_LMP DAM NP15, 45 days from 2026-11-10 minus 45 days: two windows | 5400 records; 45 days; 2 requests | 11-19 02:29Z | pass |
| Client::fetch<T: FromRecord> (typed) | PRC_LMP DAM TH_SP15_GEN-APND, 2026-11-10 hour 1, decoded as LmpRow | decoded | 11-19 02:29Z | pass |
| Error::Oasis no-data (OASIS error documents, Error::is_no_data) | PRC_LMP DAM node NOT_A_NODE | decoded | 11-19 02:29Z | pass |
| Client::outlook (+ OutlookSeries::latest) | caiso.com Today's Outlook current/fuelsource.csv | 13 columns; 195 rows | 11-19 02:29Z | pass |
| Client::outlook (+ OutlookSeries::latest) | caiso.com Today's Outlook current/demand.csv | 4 columns; 289 rows | 11-19 02:29Z | pass |
| Client::outlook (+ OutlookSeries::latest) | caiso.com Today's Outlook current/netdemand.csv | 5 columns; 289 rows | 11-19 02:29Z | pass |
| Client::outlook (+ OutlookSeries::latest) | caiso.com Today's Outlook current/storage.csv | 3 columns; 195 rows | 11-19 02:29Z | pass |
| Client::outlook (+ OutlookSeries::latest) | caiso.com Today's Outlook current/co2.csv | 6 columns; 195 rows | 11-19 02:29Z | pass |
| OutlookSeries::latest_in / value / series / column_index | current/demand.csv, column `Current demand` | decoded | 11-19 02:29Z | pass |
| Client::outlook_on | history/20261110/demand.csv | 2026-11-10: 289 rows, 289 with measured demand | 11-19 02:29Z | pass |

OASIS allows one request per five seconds and the client paces itself to it. oasis.caiso.com is
not reachable from the cluster pods (connect timeouts), which is why the runner is a binary that
runs on a workstation. Earlier curl-only observations that still hold:

* OASIS error documents: an unknown node is HTTP 200 plus a zip holding `ERR_CODE 1000 "No data
  returned for the specified selection"`; a 61-day window gets `1004 "Data can be requested for
  period of 31 days only"`; a second request inside 5 s gets HTTP 429 or an "Acceptable Use Policy
  Violation. Please retry your request after 5 seconds." body. All covered by tests.
* `anc_type=ALL`: `SingleZip` answers `302` to `GroupZip?groupid=DAM_PRC_AS_GRP&...` (a whole-day
  CSV); handled (the `AncillaryType::All` row).
* `batteries.csv` returns 404; `storage.csv` carries the battery data.

Offline suites (pod): 42 unit + 18 client (scripted local server: pacing, 429/5xx/timeout retries,
chunking, redirect) + 10 fixture + 4 doctests. Nothing is feature-gated: every method passed.

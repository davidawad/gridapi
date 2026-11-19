# gridapi-ieso

Async Rust client for the public reports site of the
[IESO](https://www.ieso.ca) (Independent Electricity System Operator,
Ontario). No API key. Typed decoders for the price, demand, generation,
intertie and adequacy reports, plus raw file access to every other report.

Ontario moved to a locational marginal price (LMP) market in 2025. The old
`PUB_RealtimeOntarioZonalPrice.csv` is gone (404); the zonal prices are now XML
and the nodal LMPs are CSV. This crate targets the post-LMP layout.

No existing Rust client was found (crates.io and GitHub searched 2026-11-18),
so this one is written from the live files.

## Usage

```rust,no_run
use chrono::NaiveDate;
use gridapi_ieso::{Client, FileSpec, Report, Stamp, Version};

#[tokio::main]
async fn main() -> gridapi_ieso::Result<()> {
    let client = Client::new()?;
    let date = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");

    // Day-ahead Ontario zonal price, hour ending 1, CAD/MWh.
    let da = client.da_ontario_zonal_prices(date).await?;
    println!("{:?}", da.body.hour(1).and_then(|h| h.zonal_price)); // Some(<price>)

    // Nodal day-ahead LMP; the Ontario zonal price is the pseudo-node ONTARIO_ZONAL_PRICE.
    let lmp = client.da_lmp(date).await?;
    let zonal = lmp.location(gridapi_ieso::data::lmp::ONTARIO_ZONAL_PRICE).next();
    println!("{:?}", zonal.map(|r| r.lmp));

    // Five-minute real-time zonal price, newest published hour.
    let rt = client.realtime_ontario_zonal_prices_current().await?;
    for (start_utc, interval) in rt.body.published() {
        println!("{start_utc} {:?}", interval.lmp);
    }

    // Pin the newest immutable snapshot instead of the moving alias file.
    let spec = FileSpec::of(Report::DaHourlyOntarioZonalPrice, Stamp::Day(date))
        .version(Version::HighestListed);
    let fetched = client.fetch(&spec).await?;
    println!("read revision {:?} from {}", fetched.revision, fetched.url);
    Ok(())
}
```

## What is covered

| Client method | Report (`reports-public.ieso.ca/public/<Report>/`) | Format | Stamp |
| --- | --- | --- | --- |
| `da_ontario_zonal_prices(date)` | `DAHourlyOntarioZonalPrice` | XML | `YYYYMMDD` |
| `predispatch_ontario_zonal_prices(date)` | `PredispHourlyOntarioZonalPrice` | XML | `YYYYMMDD` |
| `realtime_ontario_zonal_prices(date, hour)` / `_current()` | `RealtimeOntarioZonalPrice` | XML, 12 x 5 min | `YYYYMMDDHH` |
| `realtime_virtual_zonal_prices(date, hour)` / `_current()` | `RealtimeZonalEnergyPrices` | XML, per virtual zone | `YYYYMMDDHH` |
| `da_lmp(date)` | `DAHourlyEnergyLMP` (about 1,070 nodes) | CSV | `YYYYMMDD` |
| `predispatch_lmp(date)` | `PredispHourlyEnergyLMP` | CSV | `YYYYMMDD` |
| `realtime_lmp(date, hour)` / `_current()` | `RealtimeEnergyLMP` | CSV, 5 min | `YYYYMMDDHH` |
| `demand(year)` | `Demand` (market and Ontario demand, MW) | CSV | `YYYY` |
| `generator_output_capability(date)` | `GenOutputCapability` (188 units: output, capability, available capacity) | XML | `YYYYMMDD` |
| `generation_by_fuel(year)` | `GenOutputbyFuelHourly` | XML | `YYYY` |
| `intertie_schedule_flow(date)` | `IntertieScheduleFlow` (schedules plus 5 min actual flows) | XML | `YYYYMMDD` |
| `adequacy(date)` | `Adequacy3` (supply, demand, reserves outlook; headline series typed) | XML | `YYYYMMDD` |
| `list_reports()` / `list_files(report)` | Apache directory listings | HTML | |
| `fetch(&FileSpec)` | any report, returned as text | any | any |

Every decoder is also a pure function in `gridapi_ieso::data::*` (text in,
value out), so you can decode files you cached or fetched yourself.

## Behaviour worth knowing

* **Versions.** Each file exists as an alias (`PUB_X_20261117.xml`, always the
  newest content) and as immutable revisions (`PUB_X_20261117_v3.xml`).
  `Version::Newest` (default) reads the alias in one request.
  `Version::HighestListed` parses the directory listing and reads the highest
  `_vN` (numeric order, so `v10` beats `v9`); the five-minute report listings
  are several MB. `Version::Exact(n)` reads one revision. The alias can be
  newer than the highest `_vN`.
* **Time.** IESO stamps everything in Eastern Standard Time all year (UTC-5, no
  daylight saving) and hours are hour-ending. `gridapi_ieso::time` converts
  `(date, hour[, interval])` to the UTC start instant.
* **Empty cells** (unpublished intervals) decode to `None`, never to zero.
* **Retries.** `429`, `408` and `5xx` plus connect/timeout failures retry with
  exponential backoff and full jitter (default 4 attempts, 0.5 s base, 20 s
  cap), honouring `Retry-After`. `404` is `Error::NotFound` and not retried
  (files appear on a schedule). No global state; a `Client` is cheap to clone.
* **Configuration.** `Client::builder()` sets `base_url`, `user_agent`
  (default `gridapi-ieso/<version>`), timeouts, retry policy and the default
  `Version`.

## Tests

```sh
cargo test -p gridapi-ieso                                        # fixtures + scripted local HTTP server
cargo test -p gridapi-ieso --test live -- --ignored --nocapture   # live, no key needed
```

`tests/fixtures/` are synthetic: the real response shapes, field names and timestamps with invented values (the upstream terms restrict redistributing their data).

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **19 of 19 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::da_ontario_zonal_prices (+ OntarioZonalHourly::hour) | DAHourlyOntarioZonalPrice 2026-11-17 | 24 hours | 11-19 02:28Z | pass |
| Client::predispatch_ontario_zonal_prices | PredispHourlyOntarioZonalPrice, newest of today / yesterday / tomorrow | 24 hours | 11-19 02:28Z | pass |
| Client::realtime_ontario_zonal_prices (date + hour) | RealtimeOntarioZonalPrice hour ending 12, newest day among 1..4 days back | decoded | 11-19 02:28Z | pass |
| Client::realtime_ontario_zonal_prices_current | RealtimeOntarioZonalPrice current alias | decoded | 11-19 02:28Z | pass |
| Client::realtime_virtual_zonal_prices (date + hour) | RealtimeZonalEnergyPrices hour ending 12, newest day among 1..4 days back | 2026-11-17 HE12: 9 virtual zones | 11-19 02:28Z | pass |
| Client::realtime_virtual_zonal_prices_current | RealtimeZonalEnergyPrices current alias | 9 virtual zones | 11-19 02:28Z | pass |
| Client::da_lmp (+ LmpFile::location) | DAHourlyEnergyLMP 2026-11-17 | 25632 rows; 1068 locations | 11-19 02:28Z | pass |
| Client::predispatch_lmp | PredispHourlyEnergyLMP, newest of today / yesterday / tomorrow | 25536 rows | 11-19 02:28Z | pass |
| Client::realtime_lmp (date + hour) | RealtimeEnergyLMP hour ending 12, newest day among 1..4 days back | 12768 rows | 11-19 02:28Z | pass |
| Client::realtime_lmp_current | RealtimeEnergyLMP current alias | 6384 rows | 11-19 02:28Z | pass |
| Client::demand | Demand 2026 (yearly CSV) | 7705 rows | 11-19 02:28Z | pass |
| Client::generator_output_capability (+ output_by_fuel) | GenOutputCapability 2026-11-17 | 188 generators | 11-19 02:28Z | pass |
| Client::generation_by_fuel | GenOutputbyFuelHourly 2026 (yearly XML) | 321 days | 11-19 02:28Z | pass |
| Client::intertie_schedule_flow (+ IntertieScheduleFlow::zone) | IntertieScheduleFlow 2026-11-17 | 14 intertie zones | 11-19 02:28Z | pass |
| Client::adequacy | Adequacy3 2026-11-17 | 24 capacity hours | 11-19 02:28Z | pass |
| Client::list_reports | report directory index | 139 report directories | 11-19 02:28Z | pass |
| Client::list_files | directory listing of DAHourlyOntarioZonalPrice | 185 files | 11-19 02:28Z | pass |
| Client::fetch (FileSpec::of / version(HighestListed), Version, Stamp, Report) | DAHourlyOntarioZonalPrice 2026-11-17, highest listed revision | 6083 bytes | 11-19 02:28Z | pass |
| Client::load (arbitrary decoder) | DAHourlyOntarioZonalPrice 2026-11-17 through parse_ontario_zonal_hourly | 24 hours | 11-19 02:28Z | pass |

Nothing is feature-gated: every method passed. The first pass had one transient transport error on
`intertie_schedule_flow` (right after the 11 s `generation_by_fuel` download); the same call passed
on the next pass and with curl.

## Gaps

* `Adequacy3` types the headline series and internal resources by fuel; zonal
  imports/exports, area demand, dispatchable load and reserve holdbacks are in
  the file but not modelled (use `Client::fetch` and your own serde types).
* The other ~125 report directories (shadow prices, constraints, uplift,
  capacity auction, ...) are reachable through `FileSpec`/`fetch` and
  `list_files` but have no typed decoder.
* Only data on the public site; no authenticated IESO market-participant APIs.

## Licence

GPL-3.0-or-later. Not affiliated with or endorsed by the IESO. Check IESO's
terms of use for your intended use of the data.

# gridapi-gridstatus

Async Rust client for the [Grid Status](https://www.gridstatus.io) hosted API
(`https://api.gridstatus.io/v1`, `x-api-key` header). No other Rust client for
this API existed on crates.io or GitHub when this was written (2026-11-18), so
this is a from-scratch implementation against the published OpenAPI document
(`https://api.gridstatus.io/openapi.json`, v1.3.0) and synthetic fixtures with the real response shapes.

Licensed under GPL-3.0-or-later. No dependency on any private crate.

**A pure optional adapter: bring your own plan.** This crate is a thin client for
Grid Status's documented, public API and nothing more. It is independent of the
other `gridapi-*` crates (none of them depends on it, and it can be left out
without affecting them). It needs your own Grid Status account, API key and
plan; the key is yours alone and must not be shared with anyone (Grid Status
Terms of Use, section 2.2), and your use of the service is governed by the
[Grid Status Terms of Use](https://gridstatus.io/legal/terms_of_use.pdf).
Data provided by Grid Status (<https://www.gridstatus.io/>).

## Usage

```rust,no_run
use gridapi_gridstatus::{Client, Iso, Query, Resample, ResampleFrequency, ResampleFunction};

#[tokio::main]
async fn main() -> Result<(), gridapi_gridstatus::Error> {
    // Reads GRIDSTATUS_API_KEY. Defaults: 1 request per 1.5 s per client,
    // 3 retries with jittered backoff on 429/5xx.
    let client = Client::from_env()?;

    // Typed helper: ERCOT day-ahead settlement point price at HB_HOUSTON.
    let page = client
        .ercot_spp_day_ahead_hourly(
            &Query::new()
                .start_time("2026-11-17T00:00:00Z")
                .end_time("2026-11-17T03:00:00Z")
                .location("HB_HOUSTON"),
        )
        .await?;
    for row in &page.rows {
        println!("{} {:?}", row.interval_start_utc, row.spp);
    }

    // Any dataset, dynamic records, server-side resampling.
    let daily_max = client
        .query_records(
            "ercot_load",
            &Query::new()
                .start_time("2026-11-01")
                .end_time("2026-11-08")
                .resample(Resample::new(ResampleFrequency::Day1).function(ResampleFunction::Max))
                .timezone("market"),
        )
        .await?;
    for r in &daily_max.rows {
        println!("{:?} {:?}", r.timestamp("interval_start_utc"), r.f64("load"));
    }

    // Fuel mix and load for any ISO; catalog; quota.
    let mix = client.fuel_mix(Iso::Pjm, &Query::new().time("latest")).await?;
    println!("{:?}", mix.rows.first().map(|r| r.megawatts()));
    let catalog = client.list_datasets().await?;
    println!("{} datasets", catalog.datasets.len());
    let usage = client.usage().await?;
    println!("{} requests left this period", usage.remaining_requests());
    Ok(())
}
```

## Coverage

| Area | API | Rust |
| --- | --- | --- |
| Catalog | `GET /datasets` | `Client::list_datasets` |
| Dataset metadata | `GET /datasets/{id}` | `Client::dataset` |
| Generic query | `GET /datasets/{id}/query` | `Client::query<R>`, `query_records`, `query_all` (cursor/page walk with explicit `max_pages`) |
| Query params | `start_time end_time time filter_column filter_value filter_operator columns resample_frequency resample_function resample_by timezone order limit page page_size cursor` (`time_comparison`, `publish_time`, `publish_time_start`, `publish_time_end` need the `unverified` feature) | `Query` builder, validated before sending |
| Quota | `GET /api_usage` | `Client::usage` (`remaining_requests`, `remaining_rows`) |
| Update log | `GET /dataset-updates/{id}` | `Client::dataset_updates` |
| Anything else (`/pricing_locations`, `/constraints`, `/outages/...`, `/reports/daily_peak/{iso}`, `/csv-exports/...`) | | `Client::get_json` untyped escape hatch with the same throttle, retry and budget |
| Typed rows | `ercot_spp_day_ahead_hourly`, `ercot_spp_real_time_15_min`, `pjm_lmp_day_ahead_hourly`, `caiso_lmp_day_ahead_hourly`, `{iso}_lmp_day_ahead_hourly`, `{iso}_fuel_mix`, `{iso}_load` for ERCOT, PJM, CAISO, MISO, NYISO, ISONE, SPP | `SppRow`, `LmpRow`, `FuelMixRow`, `LoadRow` and `Client::{ercot_spp_*, pjm_lmp_*, caiso_lmp_*, lmp_day_ahead_hourly, fuel_mix, load}` |

Note the real-time dataset id is `ercot_spp_real_time_15_min` (the catalog
has no `..._15_minute`).

## Throttling, retry, quota

- Throttle: per client instance, lock-free (an atomic slot reservation, no mutex
  and no queue). Default one request per 1.5 s (the free tier's 1 per second is enforced as a window: exactly 1 s apart drew 429). Clones of a `Client` share
  it; separate clients do not. `ClientBuilder::min_interval` changes it.
- Retry: 429, 500, 502, 503, 504 and transient transport errors, exponential
  backoff with jitter (default 3 retries, 500 ms base, 30 s cap). `Retry-After`
  is honoured as a floor; if it exceeds the cap the call fails instead of sleeping.
- `ClientBuilder::request_budget(n)`: hard cap on HTTP requests (retries
  included) from one client. The free tier is 250 requests and 500k rows per
  month, so tests and one-off scripts should set it.
- Quota headers: `ResponseMeta::rate_limit` parses `X-RateLimit-*` /
  `RateLimit-*` / `Retry-After` when present. On the free tier none are sent
  (observed 2026-11-18); the server does send `x-request-id`, `x-page`,
  `x-page-size`, `x-limit`, and (non-streamed) `x-rows-in-response` and
  `x-has-next-page`. Use `Client::usage()` for the real quota.

Free-tier limits as returned by `/api_usage`: 250 requests/month, 500 000
rows/month, 50 000 rows per response, 1/s, 30/min, 600/h.

## Tests

- `cargo test -p gridapi-gridstatus`: unit tests plus `tests/client.rs`, which runs
  the real client against a scripted local HTTP server serving the real
  responses in `tests/fixtures/` (query rows, catalog subset, metadata, usage).
- `tests/live.rs`: `#[ignore]` tests that call the real API; each sets a request
  budget equal to what it needs.
  `GRIDSTATUS_API_KEY=... cargo test -p gridapi-gridstatus --test live -- --ignored <name>`
- `examples/live_check.rs`: a 2-request smoke check usable where the test harness
  is unavailable.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **14 of 14 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::usage (+ ClientBuilder::api_key_from_env / request_budget / build) | GET /api_usage with request_budget(15) | plan Free: 14 of 250 requests used this period (period 2026-11-01 .. 2026-12-01); 236 left | 11-19 02:28Z | pass |
| Client::list_datasets | GET /datasets | 554 datasets; 130 start with ercot_; standard fuel-mix datasets present for all 7 ISOs | 11-19 03:43Z | pass |
| Client::dataset | GET /datasets/ercot_spp_day_ahead_hourly | time index Some("interval_start_utc"); 6 columns; frequency Some("1_HOUR"); latest row Some(2026-11-20T05:00:00Z) | 11-19 02:28Z | pass |
| Client::dataset_updates | GET /dataset-updates/ercot_spp_day_ahead_hourly order=desc limit=3 | 3 updates; 0 rows | 11-19 02:28Z | pass |
| Client::get_json | GET /datasets/ercot_load untyped | 23 top-level keys | 11-19 02:28Z | pass |
| Client::query_records + Query resample (Resample, ResampleFrequency, ResampleFunction, by) | ercot_spp_real_time_15_min HB_HOUSTON 2026-11-17T00Z..01Z resampled to 1 hour, max, by location: expect one row, the maximum of ... | decoded | 11-19 02:28Z | pass |
| Client::query_all (+ Client::query, the call it makes per page) with Query page_size | ercot_spp_real_time_15_min HB_HOUSTON 2026-11-17T00Z..01Z page_size 2, max_pages 2: expect 4 rows over 2 pages | 4 rows; 2 pages | 11-19 02:28Z | pass |
| Client::ercot_spp_day_ahead_hourly + Query filter / filter_operator(In) / columns / order / limit | ercot_spp_day_ahead_hourly 2026-11-17T00Z..01Z location in (HB_HOUSTON, HB_NORTH), 4 columns, order desc, limit 4 | 2 rows | 11-19 02:28Z | pass |
| Client::ercot_spp_real_time_15_minute | HB_HOUSTON 2026-11-17T00Z..01Z | 4 rows | 11-19 02:28Z | pass |
| Client::pjm_lmp_day_ahead_hourly | 2026-11-17T00Z..01Z limit 3 | 3 rows | 11-19 02:28Z | pass |
| Client::caiso_lmp_day_ahead_hourly | 2026-11-17T00Z..01Z limit 3 (the CAISO query that never had a live run) | 3 rows | 11-19 04:00Z | pass |
| Client::lmp_day_ahead_hourly | lmp_day_ahead_hourly(Miso) 2026-11-17T00Z..01Z limit 2 (iso chosen from the catalog) | 2 rows | 11-19 03:43Z | pass |
| Client::fuel_mix + Query::time("latest") | fuel_mix(Ercot) time=latest | decoded | 11-19 04:01Z | pass |
| Client::load + Query::timezone("market") | load(Ercot) 2026-11-17T00:00Z..00:15Z, timezone=market | 3 rows | 11-19 03:43Z | pass |

**All 14 request-making calls pass**, in two sessions on a 250-request monthly quota: 15 requests
(ten calls passed, then the cap was hit) and an approved 8 more (five calls, two reruns after 429s;
retries off, so each failure showed its real status). The rows above are the passing call of each
method; the CAISO query now has its live run (`caiso_lmp_day_ahead_hourly`, 2026-11-17 00:00Z).

**Bugs found by this run and fixed**, each with a regression test:

* `list_datasets` failed to decode the live catalog (`invalid value: integer -1, expected u64`).
  `Dataset::number_of_rows_approximate` and `popularity_rank` are signed now; the fixture is a real
  dataset record with those fields set to -1. The fixed decoder then parsed the live 554-dataset catalog.
* The default spacing of 1 s drew `429 Too Many Requests. Limit: 1 per 1 second`, and each retry
  spent a budgeted request (4 of the first session's 15). The server's window is measured around the
  response: a call that took 1.7 s was followed immediately after its response by one that got a 429.
  The throttle now also waits one interval from each response, and the default is 1.5 s.

**Gated behind the `unverified` cargo feature:** only the query setters `time_comparison`,
`publish_time`, `publish_time_start`, `publish_time_end`: no dataset in the budget has a
publish-time column. Verified query knobs: `time("latest")`, `timezone("market")`, `resample`, the `in`
filter, `columns`, `order`, `limit`, `page_size`, `start_time`/`end_time`, `location`. Fixtures (real shapes, invented values) exist for: `ercot_fuel_mix`, `ercot_load`, `pjm_lmp_day_ahead_hourly`,
`GET /datasets`, `GET /api_usage`.

## License

GPL-3.0-or-later.

## Terms, attribution and notices

Data provided by Grid Status (https://www.gridstatus.io/). Requires your own Grid Status API key and plan; subject to the Grid Status Terms of Use (https://gridstatus.io/legal/terms_of_use.pdf).

This crate is a pure client for Grid Status's documented API and an optional adapter: it is independent of the other `gridapi-*` crates, and it is not described or offered as an alternative to Grid Status. Your Grid Status key must not be shared with any third party.

Live tests (`tests/live.rs`, and any live example) are `#[ignore]`d and are for your own machine with your own credentials. **Never run them in public CI**: they spend your Grid Status quota and a Grid Status key must not be shared or exposed.

gridapi is an independent, unofficial client library. It is not affiliated with, endorsed by, or sponsored by ERCOT, PJM Interconnection, California ISO, NYISO, ISO New England, MISO, Southwest Power Pool, IESO, AESO, the U.S. Energy Information Administration or Grid Status Inc. All trademarks and names belong to their owners and are used only to identify the services the library talks to. No upstream data is redistributed by this repository; fixtures are synthetic except EIA fixtures, which are U.S. government public-domain data. Users must obtain their own credentials, follow each upstream's terms, and not share credentials.

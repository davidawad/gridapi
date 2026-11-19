# gridapi-isone

Async Rust client for ISO New England market data, over two routes:

- **`express`: keyless.** The public ISO Express CSV reports on `www.iso-ne.com`
  (day-ahead and real-time LMP, five-minute LMP, fuel mix, system and zonal load,
  regulation prices, reserve requirements). No account. The crate follows the
  redirect the static reports use and obtains the session cookie the dynamic
  reports require.
- **`webservices`: authenticated, behind the non-default `isone-webservices` cargo
  feature. Untested: no account.** The Web Services API v1.1
  (`webservices.iso-ne.com/api/v1.1`), HTTP basic auth with an ISO Express account,
  credentials from `ISONE_USERNAME` and `ISONE_PASSWORD`. The default build exposes only
  the live-verified `express` route.

Async `reqwest` (rustls), typed requests and rows, `thiserror` errors, retries on
transport errors, HTTP 429 and 5xx with jittered exponential backoff (honours
`Retry-After`), configurable base URL, `User-Agent`, timeout and retry policy, no
global state, structured `tracing` events. License: GPL-3.0-or-later.

## Usage

```rust
use gridapi_isone::{
    chrono::NaiveDate,
    express::{Express, Revision},
    LocationFilter, INTERNAL_HUB_ID,
};

#[tokio::main]
async fn main() -> Result<(), gridapi_isone::Error> {
    let express = Express::new()?;
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
    let hub = LocationFilter::id(INTERNAL_HUB_ID);

    // Day-ahead hourly LMP at the Internal Hub (4000).
    let da = express.day_ahead_lmp(day).locations(hub.clone()).fetch_strict().await?;
    println!("{} HE{} ${}/MWh", da[0].interval_start, da[0].hour_ending, da[0].lmp);
    // 2026-11-17T00:00:00-05:00 HE01 $<price>/MWh

    // Five-minute final real-time prices for the hub on a settled day.
    let settled = NaiveDate::from_ymd_opt(2026, 11, 10).expect("valid date");
    let rt = express.five_minute_lmp(settled, Revision::Final).locations(hub).fetch().await?;
    println!("{} rows, {} missing days", rt.rows.len(), rt.missing.len());
    Ok(())
}
```

With an account (needs `--features isone-webservices`; untested, no account):

```rust
use gridapi_isone::{
    express::Revision,
    webservices::{Scope, WebServices, Window},
};

# async fn demo() -> Result<(), gridapi_isone::Error> {
let ws = WebServices::from_env()?; // ISONE_USERNAME, ISONE_PASSWORD
let hub = ws
    .five_minute_lmp(Revision::Preliminary, Window::Current, Scope::Location(4000))
    .await?;
# Ok(())
# }
```

Examples: `cargo run -p gridapi-isone --example express -- da-lmp 2026-11-17 4000` and
`cargo run -p gridapi-isone --features isone-webservices --example webservices`.

## What the keyless route needs

Findings from probing `www.iso-ne.com` on 2026-11-18. The earlier 302 on a public
CSV was this redirect, not a block.

| Path | Behaviour | What the client does |
|---|---|---|
| `/static-transform/csv/histRpts/<dir>/<file>` | HTTP 302 with `Location: /histRpts/<dir>/<file>`, then 200 with the CSV. No cookie, no special `User-Agent`. | Follows redirects (the default). |
| `/transform/csv/<report>?...` | HTTP **403** (S3-style body) unless the request carries the `isox_token` cookie. Any page under `/isoexpress/web/reports/` sets it. A browser `User-Agent` and a `Referer` are not enough on their own, and not needed once the cookie is present. | Fetches `/isoexpress/web/reports/` once per client, keeps the cookie in the client's jar, and on a 403 refreshes it and retries once; a second 403 is `Error::SessionRejected`. |
| The token's `User-Agent` | `isox_token` is bound to the `User-Agent` that fetched it: the same cookie jar with a different agent gets 403, with the same agent gets 200 (30 of 30 requests). The token lasts about 11 hours. | One `User-Agent` per client for both the session page and every report. If you change it, build a new client. |
| A day with no file | HTTP **200** whose body is `No data exists for this period.` | Reported as a missing day, not an error. |
| A dynamic report with no rows | HTTP 200 with a `0 lines` trailer | Reported as a missing day. |
| `hourlysystemdemand?start=D` alone | HTTP 500; `start` and `end` are both required | The client always sends both. |

Every report line is tagged `C` (comment), `H` (names, then units), `D` (data) or
`T` (trailer, `N lines`). The client checks the trailer against the rows received
and returns `Error::Truncated` on a mismatch.

## Reports

| Method | ISO Express report | Interval | Row type |
|---|---|---|---|
| `day_ahead_lmp(day)` | `da-lmp/WW_DALMP_ISO_<day>.csv` | hourly, ~29,000 rows/day | `HourlyLmp` |
| `real_time_hourly_lmp(day, Final)` | `rt-lmp/lmp_rt_final_<day>.csv` | hourly | `HourlyLmp` |
| `real_time_hourly_lmp(day, Preliminary)` | `rt-lmp/lmp_rt_prelim_<day>.csv` (names only) | hourly | `HourlyLmp` |
| `five_minute_lmp(day, rev)` | `5min-rt-{final,prelim}/lmp_5min_<day>_<HH-HH>.csv` (six files) | 5 minutes, ~350,000 rows/day | `FiveMinuteLmp` |
| `five_minute_lmp_latest(rev, filter)`, `five_minute_lmp_rolling(filter)` | `fiveminlmp/current`, `currentrollinginterval` | 5 minutes | `FiveMinuteLmp` |
| `hourly_lmp_latest(market, rev, filter)` (day-ahead final or real-time preliminary only), `hourly_lmp_rolling(...)` (real-time preliminary only; other pairs are `Error::UnsupportedQuery`, ISO-NE answers them with HTTP 500) | `hourlylmp/current`, `currentrollinginterval` | hourly (ids only) | `HourlyLmp` |
| `fuel_mix(day)` | `genfuelmix` | ~5 minutes | `FuelMix` |
| `hourly_system_load(day)` | `hourlysystemdemand` | hourly | `HourlyDemand` |
| `day_ahead_demand(day)` | `hourlydayaheaddemand` | hourly | `HourlyDemand` |
| `five_minute_system_load(day)` | `fiveminutesystemload` | 5 minutes | `FiveMinuteSystemLoad` |
| `zonal_load(day)` | `fiveminuteestimatedzonalload` | 5 minutes | `ZonalLoad` |
| `regulation_prices(day, rev)` | `fiveminrcp?type={prelim,final}` | 5 minutes | `RegulationPrice` |
| `reserve_requirements(day)` | `hourlyrequirements` | hourly | `ReserveRequirement` |

`Express::raw_csv(path)` returns any other report's bytes with the same redirect,
session and no-data handling.

Locations: `INTERNAL_HUB_ID` = 4000 (`.H.INTERNAL_HUB`); load zones
`ZONE_MAINE` 4001, `ZONE_NEW_HAMPSHIRE` 4002, `ZONE_VERMONT` 4003,
`ZONE_CONNECTICUT` 4004, `ZONE_RHODE_ISLAND` 4005, `ZONE_SEMASS` 4006,
`ZONE_WCMASS` 4007, `ZONE_NEMASS_BOSTON` 4008. Use `LocationFilter` to keep only the
ones you need while decoding; a full five-minute day is large.

## Web Services endpoints

Behind the `isone-webservices` feature (off by default). **Untested: no account.**

`five_minute_lmp`, `hourly_lmp` (day-ahead final, real-time preliminary, real-time
final), `fuel_mix`, `five_minute_system_load`, `hourly_system_load`,
`day_ahead_hourly_demand`, `regulation_prices`, `reserve_prices`, `load_forecast`,
`external_flows`, `locations`, plus `raw_records(path)` for any other resource in the
[API documentation](https://webservices.iso-ne.com/docs/v1.1/). Each takes a
`Window` (`Current` or `Day`) and, where the API allows it, a `Scope` (`All` or one
location id). Combinations the API does not serve return
`Error::UnsupportedQuery` without sending a request.

## Time

Timestamps are `DateTime<FixedOffset>` in Eastern time (`-05:00` winter, `-04:00`
summer). Hourly reports label rows "hour ending": `01`..`24`, with `02X` for the
repeated hour on the fall-back day, and no `03` on the spring-forward day. The crate
converts each label to the interval **start**. Reports with local clock times
(five-minute LMP, fuel mix, load) resolve the repeated hour by row order within each
location or fuel series. A 25-hour day therefore has 25 distinct instants and a
23-hour day has 23.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **20 of 20 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Express::new + Express::day_ahead_lmp + Request::locations + Request::fetch_strict | static da-lmp report 2026-11-17, Internal Hub | 24 rows | 11-19 02:28Z | pass |
| Request::fetch (fall-back day, '02X' hour)  | day-ahead LMP 2026-11-01 hub | 25 hub rows, 25 distinct instants, `02X` present | 11-19 02:28Z | pass |
| Express::real_time_hourly_lmp (Revision::Preliminary, LocationFilter::all().with_name) | hourly RT prelim 2026-11-16, hub by name | 24 rows | 11-19 02:28Z | pass |
| Express::real_time_hourly_lmp (Revision::Final) | hourly RT final 2026-11-10, hub | 24 rows | 11-19 02:28Z | pass |
| Express::five_minute_lmp (Revision::Final) | five-minute final 2026-11-01 (fall-back day), hub | 300 rows; 25 hours | 11-19 02:28Z | pass |
| Express::five_minute_lmp (Revision::Preliminary) | five-minute prelim 2026-11-16, hub | 288 rows | 11-19 02:28Z | pass |
| Express::five_minute_lmp_latest | dynamic fiveminlmp/current (session cookie), hub, preliminary | 1 rows | 11-19 02:28Z | pass |
| Express::five_minute_lmp_rolling | dynamic fiveminlmp/currentrollinginterval, hub | 48 rows; first 2026-11-18 17:25:00 -05:00 last 2026-11-18 21:20:00 -05:00 | 11-19 02:28Z | pass |
| Express::hourly_lmp_latest (Market::RealTime, Market::DayAhead) | dynamic hourlylmp/current: real-time preliminary and day-ahead final, hub; then the pairs ISO-NE answers 500 for are refused locally | 1 rows | 11-19 02:28Z | pass |
| Express::hourly_lmp_rolling | dynamic hourlylmp/currentrollinginterval rt, hub, preliminary | 3 rows; first 2026-11-18 18:00:00 -05:00 last 2026-11-18 20:00:00 -05:00 | 11-19 02:28Z | pass |
| Express::fuel_mix | genfuelmix 2026-11-16 | 2445 rows | 11-19 02:28Z | pass |
| Express::hourly_system_load | hourly system load 2026-11-16 | 24 rows | 11-19 02:28Z | pass |
| Express::day_ahead_demand | day-ahead hourly demand 2026-11-16 | 24 rows | 11-19 02:28Z | pass |
| Express::five_minute_system_load | five-minute system load 2026-11-16 | 288 rows | 11-19 02:28Z | pass |
| Express::zonal_load | zonal load 2026-11-16 | 2304 rows | 11-19 02:28Z | pass |
| Express::regulation_prices (Revision::Preliminary) | regulation clearing prices 2026-11-16 | 288 rows | 11-19 02:28Z | pass |
| Express::reserve_requirements | hourly reserve requirements 2026-11-16 | 96 rows | 11-19 02:28Z | pass |
| Express::raw_csv | raw CSV of the static da-lmp report for 2026-11-17 | 2453291 bytes of CSV | 11-19 02:28Z | pass |
| Request::through + Series::missing (a day ISO-NE never published is 'missing', not an error) | day-ahead LMP 2035-01-01 | 0 rows, missing = [2035-01-01] | 11-19 02:28Z | pass |
| Request::through (multi-day range) | day-ahead LMP 2026-11-15..2026-11-17, hub | 72 rows over 3 days, 0 missing | 11-19 02:28Z | pass |

**Bug found and fixed by this run.** `Express::hourly_lmp_latest(Market::DayAhead,
Revision::Preliminary, ..)` answered HTTP 500. ISO-NE serves the latest hourly report as
day-ahead `Final` and real-time `Preliminary` only, and the rolling window as real-time
`Preliminary` only; every other pair is HTTP 500. Those pairs are now refused locally with
`Error::UnsupportedQuery` before a request is sent, and `(DayAhead, Final)` is covered by a
synthetic fixture (`tests/fixtures/hourly_current_da_final.csv`) and a no-request test.

**Gated behind the non-default `isone-webservices` cargo feature: untested, no account.** The
authenticated Web Services API v1.1 (`webservices` module, `Credentials`) needs an ISO Express
account (sign-up asks for a phone number and company), which was not available. What is known: the
endpoint answers HTTP 401 with `WWW-Authenticate: Basic realm="DataSourceRealm"` without
credentials and bad credentials surface as `Error::Unauthorized`. The request paths and JSON record
shapes come from ISO-NE's published documentation and schema, and the fixtures in
`tests/fixtures/ws/` are authored, not captured. The default build does not expose it.

How far back the keyless files go (probe of 15 March in each year, HTTP 200 with data versus the
no-data body): day-ahead LMP from 2016 (absent 2014), final real-time hourly from 2018 (absent
2016), final five-minute LMP from 2022 (absent 2020), fuel mix from 2020 (empty 2018), hourly
system load from 2020 (empty 2018).

## Tests

```text
cargo test -p gridapi-isone                                          # fixtures + mock server
cargo test -p gridapi-isone --test live -- --ignored --nocapture     # live network
```

Fixtures in `tests/fixtures/` (CSV) are synthetic: the real response shapes, field names and timestamps with invented values (the upstream terms restrict redistributing their data).

## Not affiliated

This is an independent client for publicly posted files and a documented API. It is
not affiliated with or endorsed by ISO New England. Check ISO-NE's terms of use before
heavy automated retrieval, and keep the default retry backoff in place.

## Terms, attribution and notices

Data from ISO New England. Content is protected by copyright; non-personal use may require permission (https://www.iso-ne.com/legal-privacy).

Respect ISO-NE's throttling policy: excessive concurrent or invalid-URL requests can be throttled. A day-range query requests one file per day and reports days ISO-NE has no file for as missing, so ask only for dates you expect to exist. Non-personal use of ISO-NE content may require permission: contact info@iso-ne.com.

Live tests (`tests/live.rs`, and any live example) are `#[ignore]`d and are for your own machine with your own credentials. **Never run them in public CI**: they hit ISO-NE's servers, which throttle excessive or invalid requests.

gridapi is an independent, unofficial client library. It is not affiliated with, endorsed by, or sponsored by ERCOT, PJM Interconnection, California ISO, NYISO, ISO New England, MISO, Southwest Power Pool, IESO, AESO, the U.S. Energy Information Administration or Grid Status Inc. All trademarks and names belong to their owners and are used only to identify the services the library talks to. No upstream data is redistributed by this repository; fixtures are synthetic except EIA fixtures, which are U.S. government public-domain data. Users must obtain their own credentials, follow each upstream's terms, and not share credentials.

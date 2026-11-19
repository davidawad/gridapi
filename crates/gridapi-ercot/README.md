# gridapi-ercot

Async Rust client for the [ERCOT Public API](https://apiexplorer.ercot.com) (`api.ercot.com/api/public-reports`).

- Handles ERCOT's Azure B2C login for you: password grant, `id_token` cached for its hour, refreshed ahead of expiry with the refresh token (falls back to a fresh login), and re-acquired once after an HTTP 401.
- Report catalog (EMIL products and their data endpoints), a generic paged query builder, and typed helpers for prices, LMPs, load, forecasts, wind, solar and generation.
- Archive listing, single and bulk download, with zip extraction (including the zip-of-zips bulk format).
- Live generation fuel mix from ERCOT's dashboard feed (unofficial; see below).
- Retries 429, 5xx, transport errors and CDN bot-check pages with jittered exponential backoff, honouring `Retry-After`. Cookie jar and redirect following are on, with a browser-like `User-Agent`, because the CDN in front of `api.ercot.com` sometimes answers with a 302 "Loading" page.
- `rustls`, no OpenSSL. No global state; `Client` is `Clone + Send + Sync`.

## Credentials

Register at <https://apiexplorer.ercot.com> to get an account (username and password) and a subscription key. Then:

```sh
export ERCOT_USERNAME=you@example.com
export ERCOT_PASSWORD=...
export ERCOT_SUBSCRIPTION_KEY=...
```

`Credentials::new(user, password, key)` takes explicit values; `Debug` redacts all three.

## Usage

```rust
use chrono::NaiveDate;
use gridapi_ercot::{Client, Credentials, DayAheadSpp, Query, RealTimeSpp};

#[tokio::main]
async fn main() -> Result<(), gridapi_ercot::Error> {
    let client = Client::new(Credentials::from_env()?)?;

    // Newest real-time (15-minute) settlement point price at a hub.
    let today = NaiveDate::from_ymd_opt(2026, 11, 18).expect("valid date");
    if let Some(p) = client.latest_real_time_spp("HB_NORTH", today).await? {
        println!("HB_NORTH {} h{} i{}: {:.2} $/MWh", p.delivery_date, p.delivery_hour, p.delivery_interval, p.price);
    }

    // Day-ahead hourly prices for one hub, all pages (at most 10).
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
    let dam = client
        .fetch_all(&Query::<DayAheadSpp>::new().delivery_date(day, day).settlement_point("HB_HOUSTON"), 10)
        .await?;
    for r in dam {
        println!("HE{:02} {:.2}", r.hour_ending.get(), r.price);
    }

    // Anything else: the generic query takes any filter the API Explorer shows.
    let page = client
        .query(
            &gridapi_ercot::ReportRequest::new("NP6-905-CD", "spp_node_zone_hub")
                .range("deliveryDate", "2026-11-17", "2026-11-17")
                .filter("settlementPointType", "LZ")
                .size(100),
        )
        .await?;
    println!("{} rows, columns: {:?}", page.meta.total_records, page.fields.iter().map(|f| &f.name).collect::<Vec<_>>());
    Ok(())
}
```

## Coverage

| Area | EMIL id | Artifact | Rust |
|---|---|---|---|
| Catalog | all | `GET /` | `Client::products`, `Client::product`, `Client::search_products` |
| Any report | any | any | `ReportRequest`, `Client::query`, `Client::query_all` |
| RT settlement point prices (15 min) | NP6-905-CD | `spp_node_zone_hub` | `Query::<RealTimeSpp>`, `Client::latest_real_time_spp` |
| DAM settlement point prices | NP4-190-CD | `dam_stlmnt_pnt_prices` | `Query::<DayAheadSpp>` |
| RT LMPs by node, zone, hub (SCED) | NP6-788-CD | `lmp_node_zone_hub` | `Query::<RealTimeLmp>` |
| Actual load by weather zone | NP6-345-CD | `act_sys_load_by_wzn` | `Query::<ActualLoad>` |
| 7-day load forecast by model and weather zone | NP3-565-CD | `lf_by_model_weather_zone` | `Query::<LoadForecast>` |
| Wind actual, 5 min | NP4-733-CD | `wpp_actual_5min_avg_values` | `Query::<WindActual5Min>` |
| Solar actual, 5 min | NP4-738-CD | `spp_actual_5min_avg_values` | `Query::<SolarActual5Min>` |
| State-estimator total generation | NP6-625-CD | `se_ld_rpt_ercot_gen` | `Query::<SystemGeneration>` |
| Fuel mix | (dashboard feed) | `fuel-mix.json` | `Client::fuel_mix` |
| Archive | any | `archive/{emil}` | `Client::archives`, `archives_all`, `download_archive`, `download_archives`, `ArchiveFile::extract` |

Adding another typed report is one marker type, one record type implementing `FromRow`, and a few filter methods; see `src/reports/`.

## Things worth knowing

- **Times are Central Prevailing Time with no offset.** Records use `chrono::NaiveDateTime` (`gridapi_ercot::chrono` re-exports the crate); `dst_flag` marks the repeated hour at the autumn clock change.
- **Hour-ending columns support exact match only** (`hourEnding=07:00`), not ranges. Real-time price hours (`deliveryHour`) are integers and do support ranges. The API answers HTTP 400 and lists the offending names for any parameter a report does not have (`Error::Api { unknown_params, .. }`).
- **Default sort is newest day first, then hour and interval ascending.** `size(1)` on NP6-905-CD therefore returns the first interval of the latest day, not the newest. Use `latest_real_time_spp`, or constrain the hour range. The API allows `dir` only when sorting by a single field; multi-field sorts (`sort_ascending_by`) are ascending.
- **NP6-345-CD (actual load) posts a finished day the next morning**, so yesterday is usually empty.
- **Page size:** the server accepted 100 000 rows per page. `query_all` / `fetch_all` default to 50 000 and take a `max_pages` budget, failing with `Error::TooManyPages` before fetching the rest rather than truncating silently.
- **Fuel mix is not in the Public API.** `Client::fuel_mix` reads the JSON feed behind ERCOT's website dashboard (`https://www.ercot.com/api/1/services/read/dashboards/fuel-mix.json`, no credentials, 5-minute snapshots). It is undocumented and may change; a changed shape surfaces as `Error::Decode`.
- **Bot check.** An HTML body where data was expected is `Error::BotCheck` and is retried; redirects are followed with cookies kept.
- `Client::builder(creds)` lets you change the base URL, token URL, `User-Agent`, timeout and `RetryPolicy`.

## Tests

```sh
cargo test                                   # fixtures + mock server, no network
ERCOT_USERNAME=... ERCOT_PASSWORD=... ERCOT_SUBSCRIPTION_KEY=... \
  cargo run --example live_smoke             # every live check, prints sample values
ERCOT_USERNAME=... ERCOT_PASSWORD=... ERCOT_SUBSCRIPTION_KEY=... \
  cargo test --test live -- --ignored --nocapture --test-threads=1
```

Fixtures under `tests/fixtures/` are synthetic: the real response shapes, field names and timestamps with invented values (the upstream terms restrict redistributing their data).

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **17 of 17 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Credentials::from_env + Client::new + Client::products | ROPC login, GET /api/public-reports (catalog) | 121 products (98 queryable) | 11-19 02:28Z | pass |
| Client::product | GET /np6-905-cd | NP6-905-CD \| Settlement Point Prices at Resource Nodes, Hubs and Load Zones \| last post Some("2026-11-18T20:17:01") | 11-19 02:28Z | pass |
| Client::search_products | catalog filtered by 'settlement point price' | 4 products match | 11-19 02:28Z | pass |
| Client::latest_real_time_spp + Client::fetch (Query<RealTimeSpp>: delivery_date, delivery_hour, settlement_point_type, size) | NP6-905-CD HB_NORTH today, then hubs in delivery hour 1 | 5 hubs | 11-19 02:28Z | pass |
| Client::fetch_all + Client::fetch (Query<DayAheadSpp>: delivery_date, settlement_point, hour_ending) | NP4-190-CD HB_HOUSTON yesterday, then exact HE07 | 24 hours | 11-19 02:28Z | pass |
| Client::fetch (Query<RealTimeLmp>: sced_timestamp, settlement_point, size) | NP6-788-CD HB_NORTH, last 3 hours | decoded | 11-19 02:28Z | pass |
| Client::fetch_all (Query<ActualLoad>: operating_day) | NP6-345-CD two days back | decoded | 11-19 02:28Z | pass |
| Client::fetch (Query<LoadForecast>: posted, in_use_only, size) | NP3-565-CD last 8 hours, in-use model only | 1536 rows | 11-19 02:28Z | pass |
| Client::fetch (Query<WindActual5Min>) | NP4-733-CD latest 3 | decoded | 11-19 02:28Z | pass |
| Client::fetch (Query<SolarActual5Min>) | NP4-738-CD latest 3 | decoded | 11-19 02:28Z | pass |
| Client::fetch (Query<SystemGeneration>) | NP6-625-CD latest 3 | decoded | 11-19 02:28Z | pass |
| Client::fuel_mix | ercot.com dashboard fuel-mix.json | 534 snapshots | 11-19 02:28Z | pass |
| Client::archives + Client::download_archive + Client::download_archives + ArchiveFile::extract | NP6-905-CD archive, last 6 hours: list, download newest, bulk-download 2 | 24 docs; 2 files | 11-19 02:28Z | pass |
| Client::archives_all | NP6-905-CD archive posted in the last 2 hours, size 50, max 3 pages | 8 documents| 11-19 02:28Z | pass |
| Client::query_all (ReportRequest range/filter/size) | NP4-190-CD dam_stlmnt_pnt_prices HB_NORTH yesterday, size 10, max 5 pages | 24 rows merged across 3 pages of 10 | 11-19 02:28Z | pass |
| Client::query (success path; ReportRequest sort) | NP4-190-CD dam_stlmnt_pnt_prices HB_NORTH yesterday sorted by hourEnding desc | 2026-11-17: 24 rows of 24 total; first row hour ending Some(String("24:00")), 5 columns | 11-19 02:28Z | pass |
| Client::query (error path: unknown filter) | NP4-190-CD with filter notAParam | 400 naming notAParam | 11-19 02:28Z | pass |

The per-report checks are the crate's own `tests/live/checks.rs`, included unchanged in the
runner, so the runner and `examples/live_smoke` cannot drift. Nothing is feature-gated: every
method passed.

## License

GPL-3.0-or-later.

## Terms, attribution and notices

ERCOT API use is subject to ERCOT's Terms of Use for the ERCOT API (https://www.ercot.com/help/terms/data-portal). Register your own account; do not share credentials. 'ERCOT' is a trademark of ERCOT.

Download cap: ERCOT's Data Access Portal terms say "You may not download the same report from the Data Access Portal more than three times within a 12-month period". `Client::download_archive` and `Client::download_archives` do not enforce it: download each report once and keep it. The archive live check downloads real documents on every run, so do not run it repeatedly.

Create your ERCOT account yourself with your real name (ERCOT bars bot-registered accounts and misrepresenting your identity) and never share the credentials. ERCOT's name and logo are used only descriptively; no logo is used here.

Live tests (`tests/live.rs`, and any live example) are `#[ignore]`d and are for your own machine with your own credentials. **Never run them in public CI**: they use your own ERCOT credentials, which must not be shared, and ERCOT caps downloads of the same report at 3 per 12 months.

gridapi is an independent, unofficial client library. It is not affiliated with, endorsed by, or sponsored by ERCOT, PJM Interconnection, California ISO, NYISO, ISO New England, MISO, Southwest Power Pool, IESO, AESO, the U.S. Energy Information Administration or Grid Status Inc. All trademarks and names belong to their owners and are used only to identify the services the library talks to. No upstream data is redistributed by this repository; fixtures are synthetic except EIA fixtures, which are U.S. government public-domain data. Users must obtain their own credentials, follow each upstream's terms, and not share credentials.

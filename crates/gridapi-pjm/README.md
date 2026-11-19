# gridapi-pjm

Async Rust client for the [PJM Data Miner 2 API](https://api.pjm.com/api/v1)
(`api.pjm.com/api/v1`): the feed catalog and field definitions, a generic
feed query builder with filters / paging / `fields` / `sort`, and typed
helpers for the feeds most people want.

No Rust client for this API existed on crates.io or GitHub when this was
written (searches for `pjm`, `pjm-api`, `pjm-dataminer`; the only GitHub
hits were Python wrappers), hence this crate.

Licensed under GPL-3.0-or-later.

## Usage

```rust,no_run
use chrono::NaiveDate;
use gridapi_pjm::{Client, LmpRequest};

#[tokio::main]
async fn main() -> Result<(), gridapi_pjm::Error> {
    // PJM_API_KEY if set, otherwise the public key the Data Miner 2 web app ships.
    let client = Client::new()?;

    // Day-ahead LMPs at Western Hub (pnode 51288) for one day.
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
    let rows = client.da_hrl_lmps(LmpRequest::new(day).pnode_id(51288)).await?;
    println!("{} {} ${:.2}/MWh", rows[0].pnode_name, rows[0].datetime_beginning_utc, rows[0].total_lmp_da);
    // WESTERN HUB 2026-11-17 05:00:00 $101.50/MWh
    Ok(())
}
```

Any feed, with your own types or untyped rows:

```rust,no_run
use gridapi_pjm::{Client, Order, Query, Row};

# async fn run(client: Client) -> Result<(), gridapi_pjm::Error> {
let md = client.feed_metadata("da_hrl_lmps").await?;           // field definitions
let filterable: Vec<_> = md.filterable().map(|c| &c.field_name).collect();

let q = Query::new()
    .row_count(1_000)                                          // page size, max 50,000
    .fields(["datetime_beginning_utc", "pnode_id", "total_lmp_da"])
    .sort("datetime_beginning_ept", Order::Asc)
    .pnode_ids([51288, 1])                                     // pnode_id=51288;1
    .filter("datetime_beginning_ept", "11/17/2026 00:00to11/17/2026 23:59");
let page = client.query::<Row>("da_hrl_lmps", &q).await?;      // one page + totalRows
let all: Vec<Row> = client.query_all("da_hrl_lmps", &q).await?; // follows startRow
# Ok(()) }
```

`cargo run -p gridapi-pjm --example quickstart -- 2026-11-17` prints a day of
Western Hub prices.

## Endpoints covered

| Call | Feed | Rows |
| --- | --- | --- |
| `feeds()` | `GET /api/v1/` | `FeedSummary` for all 119 feeds |
| `feed_metadata(feed)` | `GET /api/v1/{feed}/metadata` | `FeedMetadata` with `Column`s (type, filterable, sortable) |
| `query(feed, &Query)` / `query_all` | any `GET /api/v1/{feed}` | your type or `Row` |
| `da_hrl_lmps(LmpRequest)` | `da_hrl_lmps` | `DaHourlyLmp` |
| `rt_hrl_lmps(LmpRequest)` | `rt_hrl_lmps` (verified) | `RtLmp` |
| `rt_fivemin_hrl_lmps(LmpRequest)` | `rt_fivemin_hrl_lmps` (verified) | `RtLmp` |
| `hrl_load_metered(LoadRequest)` | `hrl_load_metered` | `HourlyLoadMetered` |
| `load_frcstd_7_day(ForecastRequest)` | `load_frcstd_7_day` | `LoadForecast7Day` |
| `very_short_load_frcst(..)` | `very_short_load_frcst` | `FiveMinuteLoadForecast` |
| `gen_by_fuel(date filter)` | `gen_by_fuel` | `GenByFuel` |
| `pnodes(PnodeRequest)` / `pnode(id)` | `pnode` | `Pnode` |

Datetime filters use PJM's `MM/DD/YYYY HH:MMtoMM/DD/YYYY HH:MM` format;
`DateRange`, `NaiveDate` (whole day), `RelativeDate` (`Today`, `CurrentHour`,
...) and exact instants all render to it. LMP feeds require a datetime filter
(PJM answers 400 otherwise); `LmpRequest` always sets one and defaults to
`row_is_current=true` (latest version).

Datetimes in typed rows are `chrono::NaiveDateTime` exactly as PJM sends them:
`*_ept` is Eastern prevailing time, `*_utc` is UTC, and the stamp is the
*start* of the interval.

## Keys

| `KeyMode` | Key sent | On 401 |
| --- | --- | --- |
| `Auto` (default) | `PJM_API_KEY` if set and non-empty, else the public key | as below |
| `User(ApiKey)` | the key you provisioned at <https://apiportal.pjm.com/> | `Error::Unauthorized` |
| `PublicWebApp` | `subscriptionKey` from <https://dataminer2.pjm.com/config/settings.json>, fetched on first use | refetch `settings.json` once and retry; then `Error::Unauthorized` |

The key travels in the `Ocp-Apim-Subscription-Key` header and is never logged
(`ApiKey`'s `Debug` is redacted).

The public key is the one PJM's own web app ships, so it works without
registration and rotates without notice. PJM's non-member terms limit use of
Data Miner data to internal business purposes, and commercial use or
republication needs PJM membership; get your own key for anything beyond
experimentation.

## Rate limits

PJM's Data Miner 2 page states: members may not exceed **600** data
connections per minute, non-members may not exceed **6**
(<https://www.pjm.com/markets-and-operations/etools/data-miner-2>).

Each `Client` throttles itself to `RateLimit::NON_MEMBER` (6 per minute) with a
strict sliding window, so a burst of 6 goes out immediately and the 7th waits
until the first is a minute old. Use `ClientBuilder::rate_limit(RateLimit::MEMBER)`
with a member key, `unthrottled()` to leave it to the server. The window belongs
to the client and its clones: there is no global state, and separate clients do
not coordinate. Each page of `query_all` and each retry counts as a request.

Retries (default 3) cover HTTP 429, 5xx and transient transport errors, with
full-jitter exponential back-off and `Retry-After` honoured (capped at 120 s).
Validation errors (400) are never retried; they come back as `Error::Rejected`
carrying PJM's field messages.

## PJM behaviour worth knowing

* `rowCount` and `startRow` are mandatory once any parameter is given; the
  client always sends both. Maximum `rowCount` is 50,000; date ranges must be
  within 366 days.
* Hourly LMPs older than 731 days and five-minute LMPs older than 186 days are
  *archived*: queries need to sit in one calendar year (UTC), cannot sort, and
  can filter only on dates, `type`, `row_is_current` and `version_nbr`.
* LMPs are versioned. Verified real-time prices post the next business day, so
  yesterday's `rt_hrl_lmps` can be empty.
* `very_short_load_frcst` holds 30 days of history (200k+ rows); filter it,
  for example `evaluated_at_ept=CurrentHour`.
* Not wrapped: `download=true` / `format=csv|xml` responses, the unverified
  (`rt_unverified_*`) feeds' special relative filters beyond `RelativeDate`,
  and PJM's `syndication` API. Use `query` with your own row type for any feed
  without a typed helper.

## Tests

* Unit and fixture tests: synthetic responses under `tests/fixtures` (real shapes, invented values).
* `tests/mock_server.rs`: a local `wiremock` server covering auth modes, the
  401 refetch, retry/back-off, paging, throttling and error mapping.
* Live tests, ignored by default (they honour the 6-per-minute limit, so they
  take a couple of minutes):

  ```text
  cargo test -p gridapi-pjm --test live -- --ignored --nocapture --test-threads=1
  ```

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **15 of 15 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::builder / key_mode(PublicWebApp) / build + Client::feeds | GET /api/v1/ (feed catalog) with the web-app public key | 119 feeds; the seven typed-helper feeds are all listed | 11-19 02:28Z | pass |
| Client::new + Client::feed_metadata | new() then GET /api/v1/da_hrl_lmps/metadata | 14 columns, archive cutoff Some(731) days; column(pnode_id) present: true | 11-19 02:28Z | pass |
| fetch_public_key + ApiKey::new + KeyMode::User + ClientBuilder::api_key | settings.json -> user-key client -> Client::pnode(51288); then a wrong user key | decoded | 11-19 02:28Z | pass |
| Client::da_hrl_lmps (LmpRequest::new / pnode_id) | da_hrl_lmps Western Hub 2026-11-17 | 24 rows | 11-19 02:28Z | pass |
| Client::rt_hrl_lmps | rt_hrl_lmps Western Hub, 4 days back | 24 rows | 11-19 02:28Z | pass |
| Client::rt_fivemin_hrl_lmps | rt_fivemin_hrl_lmps Western Hub, 4 days back | 288 rows | 11-19 02:28Z | pass |
| Client::hrl_load_metered (LoadRequest::new / zone) | hrl_load_metered zone AE, 5 days back | 48 rows | 11-19 02:28Z | pass |
| Client::load_frcstd_7_day (ForecastRequest::new / area) | load_frcstd_7_day RTO_COMBINED | 168 rows | 11-19 02:28Z | pass |
| Client::very_short_load_frcst | very_short_load_frcst evaluated_at_ept=CurrentHour, RTO_COMBINED | 144 rows | 11-19 02:29Z | pass |
| Client::gen_by_fuel (DateFilter::Relative / RelativeDate) | gen_by_fuel Yesterday | 240 rows | 11-19 02:29Z | pass |
| Client::pnodes (PnodeRequest::new / name) + Client::pnode | pnodes name=WESTERN HUB; pnode(51288) | decoded | 11-19 02:29Z | pass |
| Client::query<T> (typed, one page) + Query datetime_beginning_utc / pnode_ids / fields / sort / row_count | da_hrl_lmps Western Hub, exact 2026-11-17 05:00 UTC, sort by datetime_beginning_ept asc, row_count 5 | 1 row | 11-19 02:29Z | pass |
| Client::query_rows (untyped) + Query::fields / filter | da_hrl_lmps Western Hub 2026-11-17, fields datetime_beginning_utc,total_lmp_da, row_count 3 | 24 rows | 11-19 02:29Z | pass |
| Client::query_all (paging) | load_frcstd_7_day RTO_COMBINED, row_count 50: expect >= 160 rows over >= 4 pages | 168 rows over 50-row pages | 11-19 02:30Z | pass |
| Error::Rejected (no datetime filter on an LMP feed) | da_hrl_lmps with only pnode_ids, no datetime | typed rejection as designed: PJM rejected the request (HTTP 400): Filters: A datetime is missing. Please input values for datetime_beginning_ept or datetime_beginning_utc \| API_1041 \| A datetime is missing. Please input values for datetime_beginning_ept or datetime_beginning_utc | 11-19 02:30Z | pass |

PJM's non-member limit is 6 requests a minute and the client paces itself to it, so the run takes
about four minutes. Mock-server (21 tests) and fixture/unit suites pass offline; fixtures are synthetic
(real shapes, invented values). Observed, not documented by PJM: sixteen back-to-back requests
with the public key in 7 seconds all returned 200, so PJM does not appear to enforce the stated
figure at that volume; the client still paces itself to it by default because the terms state it.
Nothing is feature-gated: every method passed. A call that PJM rejects (`Sort field is not
sortable` on `datetime_beginning_utc`) came from the runner's own query, not a client defect; the
metadata marks only `datetime_beginning_ept` sortable.

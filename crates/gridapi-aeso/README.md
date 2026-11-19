# gridapi-aeso

Async Rust client for the public API of the
[AESO](https://www.aeso.ca) (Alberta Electric System Operator): pool price,
system marginal price, Alberta internal load, live supply and demand,
intertie capability, generation capacity and outages, merit order and reference
data.

No existing Rust client was found (crates.io and GitHub searched 2026-11-18),
so this one is written from the live API.

## Getting a key

The gateway (`https://apimgw.aeso.ca/public`) needs an `API-KEY` header. Register
at <https://developer-apim.aeso.ca/>, subscribe to the "AESO Public API"
product, and copy the primary key from your profile. The gateway answers `404`
(not `401`) for a missing or invalid key; this crate says so in the error text.
AESO's terms limit the data to non-commercial, personal or educational use
unless you have its written permission.

## Usage

```rust,no_run
use chrono::NaiveDate;
use gridapi_aeso::{Client, DateRange};

#[tokio::main]
async fn main() -> gridapi_aeso::Result<()> {
    // Reads AESO_API_KEY. Or: Client::builder().api_key("...").build()?
    let client = Client::from_env()?;
    let day = NaiveDate::from_ymd_opt(2026, 11, 16).expect("valid date");

    let prices = client.pool_price(DateRange::day(day)).await?;
    println!("{}: {:?} CAD/MWh", prices[0].begin_mpt, prices[0].pool_price); // 0.92

    let now = client.current_supply_demand().await?;
    println!("load {:?} MW, BC flow {:?} MW", now.alberta_internal_load, now.flow("British Columbia"));
    Ok(())
}
```

## What is covered

| Client method | Endpoint under `/public` | Notes |
| --- | --- | --- |
| `pool_price(range)` | `poolprice-api/v1.1/price/poolPrice` | hourly, actual + forecast + 30-day average; max 366 days |
| `system_marginal_price(range)` | `systemmarginalprice-api/v1.1/price/systemMarginalPrice` | irregular intervals with volume |
| `system_marginal_price_current()` | `.../systemMarginalPrice/current` | |
| `alberta_internal_load(range)` | `actualforecast-api/v1/load/albertaInternalLoad` | hourly actual + forecast MW |
| `current_supply_demand()` | `currentsupplydemand-api/v2/csd/summary/current` | totals, generation by fuel, reserves, interchange by path |
| `generation_assets_current()` | `currentsupplydemand-api/v1/csd/generation/assets/current` | 231 assets: max capability, net generation |
| `interchange_capability(&InterchangeQuery)` | `itc/v1/interchange` | ATC/TTC/TRM/gross offer per intertie and flowgate; max 100 days |
| `intertie_outages(&OutageQuery)` | `itc/v1/outage` | outages that cut transfer capability |
| `aies_gen_capacity(range)` | `aiesgencapacity-api/v1/AIESGenCapacity` | MC/outage/available by fuel; max 31 days |
| `load_outage_forecast(range)` | `loadoutageforecast-api/v1/loadOutageReport` | |
| `energy_merit_order(date)` | `energymeritorder-api/v1/meritOrder/energy` | every offer block per hour; **60-day lag** |
| `operating_reserve_offers(date)` | `operatingreserveoffercontrol-api/v1/operatingReserveOfferControl` | 60-day lag |
| `metered_volume(&MeteredVolumeQuery)` | `meteredvolume-api/v1/meteredvolume/details` | |
| `unit_commitment(range)` | `unitcommitmentdata-api/v2/unitCommitment` | from 2024-07-01 |
| `asset_list(&AssetListQuery)` | `assetlist-api/v1/assetlist` | |
| `pool_participants()` | `poolparticipant-api/v1/poolparticipantlist` | |
| `get_text` / `get_json` | any path | for anything without a typed method |

Every decoder is also a pure function in `gridapi_aeso::decode`. `DateRange::chunks(n)`
splits a long span into legal requests (`POOL_PRICE_MAX_DAYS`,
`AIES_CAPACITY_MAX_DAYS`).

## Behaviour worth knowing

* **Errors.** AESO's HTTP 400 bodies (`{"message": "No Data available for this
  day.", ...}`) become `Error::Api`; `Error::is_no_data()` recognises the merit
  order inside its 60-day lag (`merit_order_latest_date(today)` gives the newest
  available date).
* **Empty values.** Prices not yet settled arrive as `""` and decode to `None`.
* **Time.** Every record carries a UTC instant and the same instant in Mountain
  prevailing time (zone-less, as AESO writes it).
* **Retries.** `429`, `408`, `5xx` and connect/timeout failures retry with
  exponential backoff and full jitter (default 4 attempts, 0.5 s base, 20 s cap),
  honouring `Retry-After`. `4xx` is never retried.
* **Key hygiene.** The key lives in a sensitive default header; `Debug` output
  and error text never contain it.
* **Configuration.** `Client::builder()` sets `api_key`, `base_url`, `user_agent`
  (default `gridapi-aeso/<version>`), timeouts and the retry policy. No global state.

## Tests

```sh
cargo test -p gridapi-aeso                                   # fixtures + scripted local HTTP server
AESO_API_KEY=... cargo test -p gridapi-aeso --test live -- --ignored --nocapture
```

`tests/fixtures/` are synthetic: the real response shapes, field names and timestamps with invented values (the upstream terms restrict redistributing their data).

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **18 of 18 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::from_env + Client::pool_price | poolprice-api/v.../price/poolPrice 2026-11-16 | 24 hours | 11-19 02:28Z | pass |
| Client::system_marginal_price | systemMarginalPrice 2026-11-16 | 122 intervals | 11-19 02:28Z | pass |
| Client::system_marginal_price_current | systemMarginalPrice/current | decoded | 11-19 02:28Z | pass |
| Client::alberta_internal_load | albertaInternalLoad 2026-11-16 | 24 hours | 11-19 02:28Z | pass |
| Client::current_supply_demand | csd/summary/current | 9 fuel groups | 11-19 02:28Z | pass |
| Client::generation_assets_current | csd/generation/assets/current | 231 assets | 11-19 02:28Z | pass |
| Client::aies_gen_capacity | AIESGenCapacity 2026-11-16 | 9 fuel groups | 11-19 02:28Z | pass |
| Client::load_outage_forecast | loadOutageReport 2026-11-16 | 24 hourly rows | 11-19 02:28Z | pass |
| Client::interchange_capability (InterchangeQuery::new / hours) | itc/v1/interchange 2026-11-16 hours 1..3 | BC import HE1 2026-11-16: TTC 800 ATC 750; 100 allocations | 11-19 02:28Z | pass |
| Client::intertie_outages (OutageQuery::new / intertie) | itc/v1/outage BC 2026-09-01..2026-11-17 | 7 intertie outages affecting BC | 11-19 02:28Z | pass |
| Client::energy_merit_order + merit_order_latest_date + MeritOrderHour::marginal_block | meritOrder/energy 2026-09-01, then a date inside the 60-day lag | 24 hours; 308 blocks | 11-19 02:28Z | pass |
| Client::operating_reserve_offers | operatingReserveOfferControl 2026-09-01 (outside the 60-day lag) | 24 hours; hour 1 (2026-09-01 00:00:00 MPT) has 47 reserve blocks | 11-19 02:28Z | pass |
| Client::metered_volume (MeteredVolumeQuery::new / assets) | meteredvolume/details one asset, 2026-09-01 | 1 participant, 1 asset; participant decoded | 11-19 02:28Z | pass |
| Client::unit_commitment | unitCommitment 2026-11-01..2026-11-17 | 3 directives | 11-19 02:28Z | pass |
| Client::asset_list (AssetListQuery::all / asset_id) | assetlist asset_ID=<id> | 1 asset | 11-19 02:28Z | pass |
| Client::pool_participants | poolparticipantlist | 411 participants | 11-19 02:28Z | pass |
| Client::get_text | raw text of poolparticipant-api/v1/poolparticipantlist | 127963 bytes of JSON text (object) | 11-19 02:28Z | pass |
| Client::get_json | systemMarginalPrice/current as untyped JSON | JSON object | 11-19 02:28Z | pass |

The key is `AESO_API_KEY`; it cannot go into a pod Job spec, which is why the runner is a binary
that runs on a workstation with the key in its environment (the key is read at run time and
redacted from all output). Nothing is feature-gated: every method passed.

## Gaps

* No typed model for `meteredvolume` and `operatingreserveoffercontrol` beyond
  what the fixtures cover (both decode, neither was seen with a large result).
* Dates for the ITC API are `yyyyMMdd`; everything else is `yyyy-MM-dd`. The
  query types hide this, `get_text` does not.
* Rate limits are not documented by AESO; the retry policy is conservative.

## Licence

GPL-3.0-or-later. Not affiliated with or endorsed by the AESO. Respect AESO's
terms of use (non-commercial, personal or educational use unless authorised).

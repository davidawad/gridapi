# gridapi-miso

Async Rust client for [MISO](https://www.misoenergy.org) (Midcontinent Independent System Operator)
market and real-time data. Typed responses, jittered retry, no global state.

> **Warning: MISO's website terms bar automated access to misoenergy.org.**
> MISO's terms (<https://www.misoenergy.org/meet-miso/legal-and-privacy/>) say: "You agree not to
> access or use this Website or the App by any means other than through the interface that is
> provided by MISO for use in accessing or using this Website or the App. You agree not use any
> automated means, including, without limitation, agents, robots, scripts, or spiders, to access,
> monitor, or copy any part of this Website or the App." Whether `public-api.misoenergy.org` and
> `docs.misoenergy.org` count as "the Website" is arguable, so every route that scripts them is
> behind the cargo feature **`miso-public-web`, which is off by default**. The route MISO provides
> for programmatic access is the **Data Exchange API**
> (<https://data-exchange.misoenergy.org/>), used with your own key. Enable `miso-public-web` only
> with MISO's written permission or at your own risk.

| Source | Host | Auth | Cargo feature (all off by default) | Status |
| --- | --- | --- | --- | --- |
| **Data Exchange API** (the supported route) | `apim.misoenergy.org` | your own `MISO_API_KEY` | `miso-dataexchange` | **untested: no account** |
| Public API (`/api/...`) | `public-api.misoenergy.org` | none | `miso-public-web` | verified live |
| Market report files | `docs.misoenergy.org/marketreports` | none | `miso-public-web` | verified live |
| Legacy RT data broker | `api.misoenergy.org/MISORTWDDataBroker` | none | `miso-public-web` | retired by MISO, returns `no data` |

With no features enabled the crate has no request-making method: pick `miso-dataexchange` (with
your own key) or, at your own risk, `miso-public-web`.

## Usage

The examples below use the website routes, so they need
`gridapi-miso = { version = "0.1", features = ["miso-public-web"] }` (see the warning above).

```rust,no_run
use gridapi_miso::{Client, FuelMixView, LmpReportKind};
use chrono::NaiveDate;

#[tokio::main]
async fn main() -> Result<(), gridapi_miso::Error> {
    let miso = Client::new()?;

    // Latest five-minute ex-ante LMP at the hubs.
    let prices = miso.ex_ante_lmp().await?;
    if let Some(hub) = prices.hub("ARKANSAS.HUB") {
        println!("{}: {} ${:.2}/MWh", prices.ref_id, hub.name, hub.lmp);
    }

    // Generation by fuel.
    let mix = miso.fuel_mix(FuelMixView::Latest).await?;
    println!("total {} MW", mix.total_mw);

    // A published hourly LMP report (nodes x HE1..24, with MCC and MLC rows).
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("valid date");
    let report = miso.lmp_report(LmpReportKind::DayAheadExPost, date).await?;
    println!("{:?}", report.lmp("ARKANSAS.HUB", 1));
    Ok(())
}
```

Configuration: `Client::builder()` takes endpoints (`Endpoints::all_at` points every host at one
test server), `user_agent`, `timeout`, `retry` ([`RetryPolicy`]: 4 attempts, full-jitter exponential
backoff on HTTP 429, 5xx and connection errors, `Retry-After` honoured) and, with the
`miso-dataexchange` feature, the Data Exchange key (`api_key`, or `api_key_from_env` which reads
`MISO_API_KEY`).

MISO asks that real-time endpoints be polled no more than once a minute.

## Endpoints covered

Public API (feature `miso-public-web`), all typed unless noted:

| Method | Path |
| --- | --- |
| `ex_ante_lmp` | `/api/MarketPricing/GetExAnteLmp` |
| `lmp_consolidated_table` | `/api/MarketPricing/GetLmpConsolidatedTable` (five-minute, hourly-integrated, DA ex-ante and ex-post, with ancillary MCPs) |
| `five_minute_ex_post_lmp(Current / Previous / Rolling)` | `/api/MarketPricing/GetRealTimeFiveMinExPost/*` |
| `ancillary_services_mcp` | `/api/MarketPricing/GetAncillaryServicesMcp` |
| `fuel_mix(Latest / Today / Yesterday)` | `/api/FuelMix[/Today\|/Yesterday]` |
| `total_load` | `/api/RealTimeTotalLoad` (DA cleared, forecast, five-minute actual) |
| `forecast_vs_actual`, `renewable_forecast`, `renewable_actual` (wind, solar) | `/api/WindSolar/*` |
| `net_actual_interchange`, `net_scheduled_interchange` | `/api/Interchange/GetNai`, `GetNsi` |
| `binding_constraints` | `/api/BindingConstraints/RealTime` |
| `ace`, `snapshot` | `/api/Ace`, `/api/Snapshot` |
| `public_api_value(path)` | any other `/api/*` path as raw JSON (`CsatSupplyDemand`, `RegionalDirectionalTransfer`, ...) |

Market report files (feature `miso-public-web`): `lmp_report(kind, date)` for `YYYYMMDD_da_expost_lmp.csv`, `_da_exante_lmp.csv`,
`_rt_lmp_prelim.csv`, `_rt_lmp_final.csv` (typed: node, node type, LMP/MCC/MLC by hour ending), and
`market_report(file_name)` for any other file's bytes. Report files are EST (UTC-5, no DST). Real-time
final files appear only after settlement; prelim files cover about the last week.

Data Exchange (`pricing_page`, `data_exchange_page`, `data_exchange_all_pages`) is implemented
for the pricing paths below and returns rows as raw JSON, behind the `miso-dataexchange` feature
(off by default). **Untested: no account.** See Verified.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) on 2026-11-18 by an internal live-matrix runner that is not part of this repository: **26 of 26 calls pass**. Values the upstream returned are not reproduced: each row records the call, the row or item counts and the result. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::ex_ante_lmp (+ ExAnteLmp::hub / interval_utc) | public-api api/MarketPricing/GetExAnteLmp | 9 hubs | 11-19 02:28Z | pass |
| Client::lmp_consolidated_table (+ PriceBlock::node) | api/MarketPricing/GetLmpConsolidatedTable | 318 nodes | 11-19 02:28Z | pass |
| Client::five_minute_ex_post_lmp (Current, Previous, Rolling) | api/MarketPricing/GetRealTimeFiveMinExPost/{Current,Previous,Rolling} | 2628 rows; 751608 rows; 675396 rows | 11-19 02:28Z | pass |
| Client::ancillary_services_mcp | api/MarketPricing/GetAncillaryServicesMcp | 8 reserve zones | 11-19 02:28Z | pass |
| Client::fuel_mix (+ FuelMix::latest) | api/FuelMix view Latest | 8 entries; 8 latest entries | 11-19 02:28Z | pass |
| Client::fuel_mix (+ FuelMix::latest) | api/FuelMix view Today | 2056 entries; 8 latest entries | 11-19 02:28Z | pass |
| Client::fuel_mix (+ FuelMix::latest) | api/FuelMix view Yesterday | 2091 entries; 7 latest entries | 11-19 02:28Z | pass |
| Client::total_load | api/RealTimeTotalLoad | decoded | 11-19 02:28Z | pass |
| Client::forecast_vs_actual | api/WindSolar/getWind | 48 hours | 11-19 02:28Z | pass |
| Client::renewable_forecast | api/WindSolar/getWindforecast | 48 hours | 11-19 02:28Z | pass |
| Client::renewable_actual | api/WindSolar/getWindactual | 21 hours | 11-19 02:28Z | pass |
| Client::forecast_vs_actual | api/WindSolar/getSolar | 48 hours | 11-19 02:28Z | pass |
| Client::renewable_forecast | api/WindSolar/getSolarforecast | 48 hours | 11-19 02:28Z | pass |
| Client::renewable_actual | api/WindSolar/getSolaractual | 21 hours | 11-19 02:28Z | pass |
| Client::net_actual_interchange | api/Interchange/GetNai | decoded | 11-19 02:28Z | pass |
| Client::net_scheduled_interchange | api/Interchange/GetNsi | 10 neighbours | 11-19 02:28Z | pass |
| Client::snapshot | api/Snapshot | 4 dashboard items | 11-19 02:28Z | pass |
| Client::binding_constraints | api/BindingConstraints/RealTime | decoded | 11-19 02:28Z | pass |
| Client::ace | api/Ace | decoded | 11-19 02:28Z | pass |
| Client::public_api_value | api/CsatSupplyDemand untyped | JSON array with 94 entries | 11-19 02:28Z | pass |
| Client::lmp_report (+ LmpReport::lmp / series / lmp_rows, NodeType, Component, LmpReportKind::file_name) | docs.misoenergy.org/marketreports 20261116_da_expost_lmp.csv | 7884 rows; 424 hub | 11-19 02:28Z | pass |
| Client::lmp_report (+ LmpReport::lmp / series / lmp_rows, NodeType, Component, LmpReportKind::file_name) | docs.misoenergy.org/marketreports 20261116_da_exante_lmp.csv | 7884 rows; 424 hub | 11-19 02:28Z | pass |
| Client::lmp_report (+ LmpReport::lmp / series / lmp_rows, NodeType, Component, LmpReportKind::file_name) | docs.misoenergy.org/marketreports 20261116_rt_lmp_prelim.csv | 7884 rows; 424 hub | 11-19 02:28Z | pass |
| Client::lmp_report (RealTimeFinal) | docs.misoenergy.org/marketreports 20260601_rt_lmp_final.csv | 7836 rows | 11-19 02:28Z | pass |
| Client::market_report | raw bytes of the day-ahead ex-ante report | 1234937 bytes | 11-19 02:28Z | pass |
| Client::data_broker (retired upstream 2025-12-12; typed Error::NoData) | MISORTWDDataBroker getfuelmix | retired endpoint answers as documented: https://api.misoenergy.org/MISORTWDDataBroker/DataBrokerServices.asmx returned no data (feed retired or not yet published) | 11-19 02:28Z | pass |

Every row above ran with `miso-public-web` enabled (the website routes; opt-in, see the warning at
the top). **Gated behind the non-default `miso-dataexchange` cargo feature: untested, no account.** The Data
Exchange API (`apim.misoenergy.org`: `data_exchange_page`, `data_exchange_all_pages`,
`pricing_page`, `ClientBuilder::api_key`, `Error::MissingApiKey`) needs a MISO account (sign-up asks
for a phone number and company), which was not available, so the default build does not expose it.
Verified without a key earlier: `pricing/v1/day-ahead/{date}/lmp-expost`, `.../lmp-exante` and
`pricing/v1/real-time/{date}/lmp-exante`, `.../lmp-expost` answer `401 Access denied due to missing
subscription key`, other made-up paths answer `404`; the key header is `Ocp-Apim-Subscription-Key`.
The response envelope and query parameter names are assumed. `data_broker` hits the broker MISO
retired on 2025-12-12 and is verified to fail the documented way (`Error::NoData`).

## Development

```text
cargo test -p gridapi-miso --all-features        # offline fixture tests (default build: gating test only)
cargo test -p gridapi-miso --features miso-public-web -- --ignored --nocapture   # live checks against MISO
```

The live checks hit MISO's website hosts, whose terms bar automated access: run them only on your
own machine with MISO's permission or at your own risk, and **never in public CI**.

## License

GPL-3.0-or-later; see [LICENSE](../../LICENSE).
Not affiliated with MISO; check MISO's terms of use for the data you republish.

## Terms, attribution and notices

MISO's website terms prohibit automated access to misoenergy.org; use the Data Exchange API (https://data-exchange.misoenergy.org/) with your own key.

MISO's website terms (https://www.misoenergy.org/meet-miso/legal-and-privacy/) say: "You agree not use any automated means, including, without limitation, agents, robots, scripts, or spiders, to access, monitor, or copy any part of this Website or the App." The routes that script `public-api.misoenergy.org` and `docs.misoenergy.org` are therefore behind the off-by-default cargo feature `miso-public-web`. The route MISO provides for programmatic access is the Data Exchange API (https://data-exchange.misoenergy.org/), behind the `miso-dataexchange` feature, with your own key.

Live tests (`tests/live.rs`, and any live example) are `#[ignore]`d and are for your own machine with your own credentials. **Never run them in public CI**: they hit MISO's website hosts, whose terms bar automated access.

gridapi is an independent, unofficial client library. It is not affiliated with, endorsed by, or sponsored by ERCOT, PJM Interconnection, California ISO, NYISO, ISO New England, MISO, Southwest Power Pool, IESO, AESO, the U.S. Energy Information Administration or Grid Status Inc. All trademarks and names belong to their owners and are used only to identify the services the library talks to. No upstream data is redistributed by this repository; fixtures are synthetic except EIA fixtures, which are U.S. government public-domain data. Users must obtain their own credentials, follow each upstream's terms, and not share credentials.

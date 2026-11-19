# gridapi

Typed, async Rust clients for eleven North American power-market data sources (ERCOT, PJM, CAISO, NYISO, ISO-NE, MISO, SPP, IESO, AESO, EIA and Grid Status), so you can pull prices, load and generation mix without writing a CSV scraper per grid operator.

![NYISO day-ahead LBMP by zone: eleven zone lines with morning and evening peaks, downstate zones highest in the evening](docs/showcase/nyiso-zones.png)

The chart above is drawn from rows decoded by `gridapi-nyiso`: the client fetched a day-ahead zonal file from a local mock server, parsed the Eastern-time stamps and split each LBMP into energy, congestion and losses. The file has NYISO's real layout; the prices are invented, so the repository carries no upstream data. Reproduce every image with `docs/showcase/make-screenshots.sh` (see [docs/showcase.md](docs/showcase.md)).

The same decode as terminal output:

![Terminal output of the gridapi-nyiso showcase example: a table of interval, zone, LBMP, energy, congestion and losses](docs/showcase/nyiso-day-ahead-terminal.png)

## Install

These crates are not on crates.io. Add the one you need as a git dependency:

```toml
[dependencies]
gridapi-nyiso = { git = "https://github.com/davidawad/gridapi", package = "gridapi-nyiso" }
```

Pin `rev = "<commit>"` or `tag = "<tag>"` for reproducible builds. Every crate is async (`tokio` plus `reqwest` with rustls, no OpenSSL), has no global state, and retries 429, 5xx and transport errors with jittered backoff.

Build everything: `cargo build --workspace`. The offline tests need no network or keys: `cargo test --workspace`.

## Crates

| Crate | Upstream | Auth | Usage and details |
| --- | --- | --- | --- |
| `gridapi-ercot` | ERCOT Public API | username, password and subscription key from `ERCOT_*` env vars | [README](crates/gridapi-ercot/README.md) |
| `gridapi-pjm` | PJM Data Miner 2 | `PJM_API_KEY`, or the public key the Data Miner web app ships | [README](crates/gridapi-pjm/README.md) |
| `gridapi-caiso` | CAISO OASIS and Today's Outlook | none | [README](crates/gridapi-caiso/README.md) |
| `gridapi-nyiso` | NYISO `mis.nyiso.com` CSV files | none | [README](crates/gridapi-nyiso/README.md) |
| `gridapi-isone` | ISO-NE ISO Express, and Web Services v1.1 | none for ISO Express; `ISONE_USERNAME` and `ISONE_PASSWORD` for Web Services | [README](crates/gridapi-isone/README.md) |
| `gridapi-miso` | MISO public API and market reports | none (`MISO_API_KEY` for the Data Exchange API) | [README](crates/gridapi-miso/README.md) |
| `gridapi-spp` | SPP portal file browser | none | [README](crates/gridapi-spp/README.md) |
| `gridapi-ieso` | IESO (Ontario) public reports | none | [README](crates/gridapi-ieso/README.md) |
| `gridapi-aeso` | AESO (Alberta) API | `AESO_API_KEY` | [README](crates/gridapi-aeso/README.md) |
| `gridapi-eia` | EIA Open Data API v2, plus EIA-860M and EIA-930 bulk files | `EIA_API_KEY` (bulk files need none) | [README](crates/gridapi-eia/README.md) |
| `gridapi-gridstatus` | Grid Status hosted API | `GRIDSTATUS_API_KEY` | [README](crates/gridapi-gridstatus/README.md) |

Keys are read from environment variables only; nothing in this repository embeds one. Minimal use, as in the render above:

```rust
use gridapi_nyiso::{chrono::NaiveDate, Client, Level};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("bad date")?;
    let rows = Client::new()?.day_ahead_lbmp(Level::Zone, day).fetch_strict().await?;
    for r in &rows {
        println!("{} {} {:.2}", r.timestamp, r.name, r.lbmp);
    }
    Ok(())
}
```

## Verified

Each crate was run against its live upstream on 2026-11-18 and its decoders are tested offline against small synthetic responses in `tests/fixtures` (real shapes, invented values). The per-crate README has the full table of live checks and their values. Offline suite across the workspace: 671 passed, 0 failed, 69 ignored with default features; 702 passed, 0 failed, 73 ignored with `--all-features` (the ignored tests are the live ones and need network, and for some, keys).

| Crate | Live-verified | Not verified (stated in its README) |
| --- | --- | --- |
| [`gridapi-ercot`](crates/gridapi-ercot/README.md#verified) | 14 of 14 live checks | none listed |
| [`gridapi-pjm`](crates/gridapi-pjm/README.md#verified) | 3 live tests with the public key | member-key behaviour; `download=true` and csv/xml responses are not wrapped |
| [`gridapi-caiso`](crates/gridapi-caiso/README.md#verified) | OASIS LMPs and the Today's Outlook feeds | the `#[ignore]` OASIS live suite end to end |
| [`gridapi-nyiso`](crates/gridapi-nyiso/README.md#verified) | the published datasets, 2026-11-17 | none listed |
| [`gridapi-isone`](crates/gridapi-isone/README.md#verified) | keyless ISO Express route, 8 of 10 live tests | Web Services route (no account yet; 2 tests skipped) |
| [`gridapi-miso`](crates/gridapi-miso/README.md#verified) | public API and market reports, 5 live tests | Data Exchange API (no account) |
| [`gridapi-spp`](crates/gridapi-spp/README.md#verified) | portal listing and downloads, checked with curl | the Rust live tests could not run in that session |
| [`gridapi-ieso`](crates/gridapi-ieso/README.md#verified) | 6 of 6 live tests | none listed |
| [`gridapi-aeso`](crates/gridapi-aeso/README.md#verified) | live key, 2026-11-18 | rate limits are undocumented by AESO |
| [`gridapi-eia`](crates/gridapi-eia/README.md#verified) | 7 of 7 live tests | none listed |
| [`gridapi-gridstatus`](crates/gridapi-gridstatus/README.md#verified) | free-tier key, 10 requests spent | the CAISO day-ahead query through this client |

### Feature gates for unverified methods

Anything that could not get a live pass is off by default, so the default build exposes only methods checked against the real service:

| Crate | Feature | Gates | Why unverified |
| --- | --- | --- | --- |
| `gridapi-isone` | `isone-webservices` | the `webservices` module and `Credentials` (Web Services API v1.1) | untested: no ISO Express account |
| `gridapi-miso` | `miso-dataexchange` | the keyed Data Exchange API (`data_exchange_page`, `pricing_page`, `ClientBuilder::api_key`) | untested: no MISO account |
| `gridapi-gridstatus` | `unverified` | the `time_comparison` and `publish_time*` query setters | no live request reached a dataset with a publish-time column |
| `gridapi-spp` | `unverified` | `Chart::GenMixSwpw` | the endpoint answered HTTP 404 on 2026-11-18 |

Enable one with, for example, `gridapi-isone = { git = "https://github.com/davidawad/gridapi", package = "gridapi-isone", features = ["isone-webservices"] }`.

## Terms of the upstream data

This is a client library; it grants you no rights to anyone's data. Each upstream sets its own terms, and several are restrictive. Read them before you use or republish what these crates return. What each upstream's published terms say, as read on 2026-11-18:

| Upstream | What the terms say | Source |
| --- | --- | --- |
| PJM | Data Miner data is for internal use only, and redistribution of data contained in or derived from it is prohibited without a PJM membership. Non-member accounts may use the data for internal business only; commercial use, including publishing or making derivatives, requires at least an Associate Membership. The public key the web app ships is not a licence to do more. | [Data Miner API guide (PDF)](https://www.pjm.com/-/media/DotCom/etools/data-miner-2/data-miner-2-api-guide.pdf) |
| AESO | Material may be used and copied for non-commercial, personal or educational purposes if unmodified with copyright notices kept. Any other use needs the AESO's written permission. | [AESO legal](https://www.aeso.ca/legal/) |
| IESO | A limited, non-exclusive, non-sublicensable licence to use and reproduce the content, provided the IESO copyright line is included on reproductions; other uses need written permission. | [IESO Terms of Use](https://www.ieso.ca/Terms-of-Use) |
| SPP | Permission to copy and distribute with citation, except in a commercial publication; any commercial use needs prior written authorisation from SPP. | [SPP terms and conditions](https://www.spp.org/terms-conditions/) |
| CAISO | The CAISO API licence is revocable and limited to use under its terms: no excessive load on production systems, no use for monitoring or benchmarking CAISO systems. CAISO states it owns CAISO Data. The website terms also bar copying or republishing except as authorised. | [CAISO privacy and terms of use](https://www.caiso.com/privacy-terms-of-use) |
| MISO | The website terms bar modifying, publishing or reproducing content, and bar automated means (agents, robots, scripts, spiders) to access or copy the website. MISO also publishes a public API and market-report files; I did not find a separate data licence for them. Check with MISO before automated use at scale. | [MISO legal and privacy](https://www.misoenergy.org/meet-miso/legal-and-privacy/) |
| ISO-NE | Content is protected by copyright; duplication or non-personal use may violate copyright. Permission to use a graph or image is requested by email. The Web Services API needs an account and is subject to its own terms, which I have not read because I have no account. | [ISO-NE legal and privacy](https://www.iso-ne.com/legal-privacy) |
| NYISO | The site reserves all rights and prohibits downloading or republishing its images and video as stand-alone files. I found no separate data licence for the `mis.nyiso.com` CSV files. | [NYISO legal notice](https://www.nyiso.com/legal-notice) |
| EIA | U.S. government publications are public domain; you may use and distribute the data, and EIA asks for an acknowledgment that includes the publication date. | [EIA copyrights and reuse](https://www.eia.gov/about/copyrights_reuse.php) |
| ERCOT | The Public API requires registration and acceptance of terms in the API Explorer. I could not find a fetchable terms page for the API, so its terms are unread. | [ERCOT API Explorer](https://apiexplorer.ercot.com) |
| Grid Status | A hosted third-party aggregator with a per-account API key and plan quotas. The terms page returns 403 to non-browser fetches, so I could not read them. | [Grid Status](https://www.gridstatus.io) |

The test fixtures (`crates/*/tests/fixtures`) are synthetic: they keep each upstream's real response shapes, field names, timestamps and station names, with the numeric values replaced by invented ones, so this repository carries none of the upstream data. The exception is `gridapi-eia`, whose fixtures are real EIA responses (U.S. government work, public domain). The ISO-NE Web Services JSON fixtures were authored from its published documentation. The "Verified" sections in the crate READMEs record the method, date, pass/fail and row counts of the live runs, and deliberately omit the prices, loads and other values the upstreams returned. The same goes for inline test samples and the live tests, which assert shapes rather than upstream values.

These crates are not affiliated with or endorsed by any of the organisations above.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE). Copyright (C) 2026 David Awad.

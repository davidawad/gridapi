# gridapi-eia

Async Rust client for the U.S. Energy Information Administration's
[Open Data API v2](https://www.eia.gov/opendata/documentation.php)
(`https://api.eia.gov/v2`), plus helpers for the keyless EIA-860M and EIA-930
bulk files.

* Route browsing and metadata (frequencies, facets, data columns, period range).
* A typed `DataQuery` builder for any route: frequency, facets, start/end,
  sort, offset/length. `data_all` / `data_stream` page past EIA's 5000-row cap
  for you.
* Typed rows and query constructors for:
  * `electricity/rto/*` (EIA-930 Hourly Grid Monitor): region data, fuel-type
    data, interchange, subregion demand, and the daily variants;
  * `electricity/operating-generator-capacity` (EIA-860 / 860M inventory);
  * `electricity/retail-sales`;
  * `natural-gas/pri/fut` (Henry Hub spot and NYMEX futures 1-4).
* Bulk files (cargo feature `bulk`, on by default), no API key needed:
  * the monthly **EIA-860M** xlsx, including the **Planned**, **Retired** and
    **Canceled or Postponed** sheets that the API does not serve;
  * the **EIA-930 six-month CSVs** (BALANCE, SUBREGION, INTERCHANGE) with the
    imputed and adjusted series.
* `reqwest` + `rustls`, retry with jittered exponential backoff on HTTP 429,
  5xx and transport errors (honours `Retry-After`), configurable base URL and
  `User-Agent`, no global state. The API key is never written to errors or
  `Debug` output.

## Usage

```rust
use gridapi_eia::datasets::rto::{self, RegionDataRow, RegionMetric};
use gridapi_eia::{Client, Direction};

#[tokio::main]
async fn main() -> Result<(), gridapi_eia::Error> {
    let client = Client::from_env()?; // reads EIA_API_KEY

    // Latest 24 hours of ERCOT demand.
    let q = rto::region_data_for("ERCO", RegionMetric::Demand)
        .sort("period", Direction::Desc)
        .length(24);
    let page = client.data_page::<RegionDataRow>(&q).await?;
    println!("{} rows in series, latest {:?}", page.total, page.rows[0]);

    // A full year, across as many 5000-row pages as it takes.
    let year = rto::region_data_for("ERCO", RegionMetric::Demand)
        .start("2025-01-01T00")
        .end("2025-12-31T23");
    let rows: Vec<RegionDataRow> = client.data_all(&year).await?;
    println!("{} hourly rows", rows.len());

    // Any other route, untyped.
    let q = gridapi_eia::DataQuery::new("petroleum/pri/spt")
        .frequency(gridapi_eia::Frequency::Daily)
        .data_column("value")
        .length(5);
    let rows: Vec<gridapi_eia::Row> = client.data_all(&q).await?;
    println!("{rows:?}");
    Ok(())
}
```

Browsing:

```rust
let root = client.root().await?;                      // top-level routes
let meta = client.route("electricity/rto/region-data").await?;
let bas  = client.facet_values("electricity/rto/region-data", "respondent").await?;
let all  = client.datasets_under("electricity/rto").await?; // (path, metadata)
```

Bulk files:

```rust
use gridapi_eia::bulk::eia860m::{self, Release, Sheet};

let dest = std::path::Path::new("eia860m.xlsx");
let release = client.download_latest_eia860m(Release::current_utc(), dest).await?;
let planned = eia860m::read_sheet(dest, Sheet::Planned)?; // blocking: spawn_blocking in async code
```

```rust
use gridapi_eia::bulk::eia930::{BalanceReader, Dataset, HalfYear, SixMonthFile};

let file = SixMonthFile { dataset: Dataset::Balance, year: 2026, half: HalfYear::JulDec };
client.download_eia930(file, std::path::Path::new("balance.csv")).await?;
for row in BalanceReader::open(std::path::Path::new("balance.csv"))? {
    let row = row?;
    println!("{} {} {:?}", row.balancing_authority, row.utc_end, row.demand_mw);
}
```

## Notes on EIA behaviour this crate handles

* Almost every number arrives as a JSON string (`"value":"49812"`); typed rows
  parse them, and `null`/blank become `None`.
* A reply to any query matching more than 5000 rows carries the warning
  `incomplete return` even when you asked for a small `length`. It is exposed
  as `DataPage::warnings`, not an error.
* Offsets only address a stable order when the query is sorted, so paged
  fetches add `sort[0]=period asc` unless you set a sort
  (`without_auto_sort()` opts out).
* Errors come in two shapes (`{"error":"msg","code":400}` and
  `{"error":{"code":"API_KEY_INVALID","message":...}}`); both map to
  `Error::Api { status, code, message }`. An unknown `frequency` for a route is
  a 400 listing the valid ones.
* Unpublished 860M months answer with a 301 to an HTML page. Redirects are not
  followed; they surface as `Error::NotPublished`, and
  `download_latest_eia860m` walks back month by month. Recent months live under
  `/xls/`, older under `/archive/xls/`; both are tried.
* EIA-930 hourly API periods are UTC unless you use `Frequency::LocalHourly`.

## Tests

```text
cargo test -p gridapi-eia                      # offline: fixtures + local mock server
EIA_API_KEY=... cargo test -p gridapi-eia --test live -- --ignored --nocapture --test-threads=1
```

Offline tests use small real responses recorded under `tests/fixtures` and a
scripted local HTTP server (`tests/mock_api.rs`, `tests/mock_bulk.rs`) for
retry, paging, error mapping and download behaviour. The `#[ignore]` live
tests hit the real API and www.eia.gov and need a free key from
<https://www.eia.gov/opendata/register.php>.

## Verified

Every request-making public method of this crate was called through the crate's own `Client` (not curl) by the [`gridapi-live`](../gridapi-live) matrix runner against the live service: **26 of 26 calls pass**. The runner is a macOS binary cross-built in a pod (`gridapi-live` at `4af2164281f`) and run on a workstation; times below are UTC. Reproduce with `gridapi-live eia` (`run.sh` supplies keys from the secrets store at run time). The full matrix as JSON is `../gridapi-live/results/2026-11-18.json`. Config-only builders and pure parsers are covered by the unit and fixture tests; the builder calls each live call used are named in its row.

| Method | Live call made | Rows / value (data timestamp) | Called (UTC) | Result |
| --- | --- | --- | --- | --- |
| Client::builder / api_key_from_env / user_agent / timeout / build; Client::root | builder().api_key_from_env().user_agent().timeout(120s).build() then GET /v2/ | 14 root routes incl. electricity, natural-gas | 11-19 02:28Z | pass |
| Client::from_env | from_env() then GET /v2/electricity | 6 child routes under electricity | 11-19 02:28Z | pass |
| Client::new | new("not-a-real-key") then GET /v2/ expects typed API_KEY_INVALID | typed error as designed: EIA API error (HTTP 403, API_KEY_INVALID): An invalid api_key was supplied. Get one at https://api.eia.gov:443 | 11-19 02:28Z | pass |
| Client::route | GET /v2/electricity/rto/region-data | frequencies ["hourly", "local-hourly"]; facets ["respondent", "type"]; period Some("2019-01-01T00")..Some("2026-11-19T08") | 11-19 02:28Z | pass |
| Client::facet_values | GET region-data/facet/respondent | 83 respondent values incl. ERCO, PJM | 11-19 02:28Z | pass |
| Client::datasets_under | walk electricity/rto | 8 datasets: ["electricity/rto/daily-interchange-data", "electricity/rto/daily-fuel-type-data", "electricity/rto/daily-region-sub-ba-data", "electricity/rto/daily-region-data", "electricity/rto/interchange-data", "electricity/rto/region-sub-ba-data", "electricity/rto/fuel-type-data", "electricity/rto/region-data"] | 11-19 02:28Z | pass |
| Client::data_page + rto::region_data / region_data_for | region_data().facet(respondent=PJM,type=D) desc length 3; region_data_for(ERCO, Demand) desc length 24 | PJM demand 2026-11-19T01 = 99458; ERCO demand 2026-11-19T01 = 53275 megawatthours (24 rows; 69097 in series) | 11-19 02:28Z | pass |
| rto::fuel_type_data / fuel_type_data_for | fuel_type_data().facet(ERCO, WND) length 3; fuel_type_data_for(ERCO, Some(NaturalGas)) length 3; fuel_type_data_for(ERCO, None) length 20 | ERCO wind 2026-11-18T05 = 19907; gas 2026-11-18T05 = 21572; 20 all-fuel rows | 11-19 02:28Z | pass |
| rto::interchange_data / interchange_from | interchange_data().facet(fromba=ERCO,toba=SWPP) length 3; interchange_from(ERCO) length 5 | ERCO->SWPP 2026-11-17T06 = -463; 5 rows out of ERCO, latest 2026-11-17T06 to CEN = -1 | 11-19 02:28Z | pass |
| rto::region_sub_ba_data | region_sub_ba_data() desc length 5 | 5 rows; 2026-11-18T08 Some("CISO")/Some("PGAE") = 11200 | 11-19 02:28Z | pass |
| rto::daily_region_data / daily_fuel_type_data / daily_interchange_data | daily_* desc length 3 each (ERCO where faceted) | daily ERCO demand 2026-11-17 = 1206928; daily fuel 2026-11-17 BAT = -18613; daily interchange 2026-11-16 ERCO->CEN = -68 | 11-19 02:28Z | pass |
| Client::data_all + DataQuery start/end/length paging | region_data_for(ERCO, Demand) 2026-11-10T00..T23 with length(10): expect 24 rows over 3 pages | 24 rows, 24 unique periods 2026-11-10T00..2026-11-10T23 | 11-19 02:28Z | pass |
| Client::data_all (past the 5000-row cap) | region_data_for(ERCO, Demand) 2025-01-01T00..2025-12-31T23 | 8760 rows (two pages) 2025-01-01T00..2025-12-31T23 | 11-19 02:28Z | pass |
| Client::data_stream | region_data_for(ERCO, Demand) length(7) desc: first stream item | first page 7 rows, latest 2026-11-19T01 | 11-19 02:28Z | pass |
| DataQuery frequency_raw / data_columns / offset / without_auto_sort / sort (untyped Row) | region-data hourly ERCO/D asc 2026-11-10T00..T05 offset 2 length 2 without_auto_sort, vs the same query at offset 0 | offset 2 starts at Some(String("2026-11-10T02")), matches third row of the full six | 11-19 02:28Z | pass |
| retail::retail_sales / monthly_for | retail_sales(Annual).facet(US, ALL) desc length 3; monthly_for(TX, Commercial) desc length 3 | US all-sector 2025 price 13.56 sales 4079909.35378; TX commercial 2026-08 price 8.96 cents/kWh sales 18258.30758 | 11-19 02:28Z | pass |
| natural_gas::prices / henry_hub_spot_daily | prices(Contract1, Daily) desc length 3; henry_hub_spot_daily() desc length 5 | NYMEX contract 1 2024-04-05 = 1.785; Henry Hub spot 2026-11-17 = 3.1 $/MMBtu | 11-19 02:28Z | pass |
| generators::operating_generator_capacity | latest period from route metadata; stateid=VA, energy_source_code=NG | 2026-08: 103 VA natural-gas generators, 15102.2 MW nameplate; first Some("Clinch River") Some("1") | 11-19 02:28Z | pass |
| Client::data_page (untyped Row, any route) | petroleum/pri/spt daily value length 5 | 5 rows of 92587 in series; latest period Some(String("2026-11-17")) | 11-19 02:28Z | pass |
| Client::download_latest_eia860m | walk back from the current month until a release is published | release 2026-09 (september_generator2026.xlsx), 14013970 bytes | 11-19 02:28Z | pass |
| Client::download_eia860m | the release just found, to a second file | release 2026-09, 14013970 bytes | 11-19 02:28Z | pass |
| eia860m::read_sheet (Operating, Planned, Retired, CanceledOrPostponed) | parse all four sheets of the downloaded workbook | release 2026-09: operating 28484 (1417017 MW), planned 2358 (290242 MW), retired 7321, canceled/postponed 1745; first operating Sand Point / 1 0.9 MW (SB) Standby/Backup: available for service but not normally used | 11-19 02:28Z | pass |
| eia860m::parse_sheet | parse the Planned sheet from an open file handle | 2358 planned rows; status codes {"L", "P", "T", "TS", "U", "V"} | 11-19 02:29Z | pass |
| Client::download_eia930 + BalanceReader::open (BALANCE) | current half-year BALANCE file, read every row | 2026 JulDec: 36826500 bytes, 202923 rows, 0 parse errors, 60 BAs, 55 numeric columns, last row Some(("YAD", 2026-11-18T05:00:00, None)) | 11-19 02:29Z | pass |
| Client::download_eia930 + SubregionReader::open (SUBREGION) | current half-year SUBREGION file, read every row | 21539394 bytes, 299114 rows, last row Some(("SWPW", "WAUW", 2026-11-18T06:00:00, Some(83.0))) | 11-19 02:29Z | pass |
| Client::download_eia930 + InterchangeReader::open (INTERCHANGE) | current half-year INTERCHANGE file, read every row | 78724071 bytes, 999055 rows, first row Some(("AECI", "MISO", Some(646.0))) | 11-19 02:29Z | pass |

Observed, not a defect: `natural_gas::prices(Contract1, Daily)` returns observations only up to
2024-04-05, so EIA no longer updates the NYMEX futures series; Henry Hub spot (`RNGWHHD`) is
current. Nothing is feature-gated: every method passed.

## Not covered

* Typed rows exist only for the datasets above; every other route works
  through `DataQuery` with `Row` (a JSON map) or your own `Deserialize` struct.
* `csv`/`xml` output formats and the `json` `length` > 5000 limit are not
  supported; paging is the supported route to more rows.
* Bulk readers return strings/numbers as published and do not reconcile 860M
  with 860 annual; the status/technology code tables are EIA's, not embedded.

## License

GPL-3.0-or-later.

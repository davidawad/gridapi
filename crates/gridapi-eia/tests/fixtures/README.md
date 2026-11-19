Source: U.S. Energy Information Administration (retrieved 2026-11-18). Not endorsed by EIA. The EIA logo is not used.

# Test fixtures: EIA excerpts

Every file in this directory is an **excerpt** of a response from the EIA Open
Data API v2 (https://www.eia.gov/opendata/) or of an EIA-930 bulk file, trimmed
to a few rows or records so the offline tests stay small. None of them is a
complete EIA dataset, and none should be read or cited as "EIA data": the
publisher's full, current data is at https://www.eia.gov/.

| File | What it is |
| --- | --- |
| `data_empty.json`, `data_fuel_type.json`, `data_generator_capacity.json`, `data_henry_hub.json`, `data_interchange.json`, `data_region.json`, `data_retail_sales.json` | excerpt: the first rows of an API v2 `data` response (the `total` field still reports the full count) |
| `facet_type.json`, `meta_region_data.json`, `meta_root.json`, `meta_rto.json` | excerpt: route and facet metadata responses |
| `error_bad_frequency.json`, `error_invalid_key.json` | excerpt: short API error bodies |
| `eia930_balance_sample.csv` | excerpt: the header and a few rows of an EIA-930 balance file |

U.S. government publications are in the public domain; EIA asks that
reproductions carry an acknowledgement such as the source line above, and EIA's
terms bar modifying content and still claiming EIA as the source, which is why
these files are labelled excerpts. EIA's terms: https://www.eia.gov/opendata/terms-of-service.php and
https://www.eia.gov/about/copyrights_reuse.php.

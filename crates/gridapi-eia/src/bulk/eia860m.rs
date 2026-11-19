//! EIA-860M: the monthly "Preliminary Monthly Electric Generator Inventory"
//! workbook, `<month>_generator<year>.xlsx`. Unlike the API it includes the
//! **Planned**, **Retired** and **Canceled or Postponed** sheets.
//!
//! ```no_run
//! use gridapi_eia::bulk::eia860m::{self, Release, Sheet};
//! use gridapi_eia::Client;
//!
//! # async fn demo() -> Result<(), gridapi_eia::Error> {
//! let client = Client::from_env()?;
//! let dest = std::path::Path::new("eia860m.xlsx");
//! let release = client.download_latest_eia860m(Release::current_utc(), dest).await?;
//! let planned = eia860m::read_sheet(dest, Sheet::Planned)?;
//! println!("{} planned generators in the {release} release", planned.len());
//! # Ok(()) }
//! ```

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use calamine::{Data, Reader, Xlsx};
use chrono::Datelike;

use crate::client::Client;
use crate::error::{Error, Result};

const XLSX_MAGIC: &[u8] = b"PK";
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// A monthly release of the workbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Release {
    /// Calendar year.
    pub year: i32,
    /// Month, 1-12.
    pub month: u32,
}

impl Release {
    /// A release, validating the month.
    pub fn new(year: i32, month: u32) -> Result<Self> {
        if (1..=12).contains(&month) {
            Ok(Self { year, month })
        } else {
            Err(Error::InvalidRequest(format!(
                "month must be 1..=12, got {month}"
            )))
        }
    }

    /// The current UTC month. EIA publishes a release for month M late in
    /// month M or early M+1, so this one is often not out yet; use
    /// [`Client::download_latest_eia860m`] to walk back to the newest.
    #[must_use]
    pub fn current_utc() -> Self {
        let now = chrono::Utc::now();
        Self {
            year: now.year(),
            month: now.month(),
        }
    }

    /// The previous calendar month.
    #[must_use]
    pub fn previous(self) -> Self {
        if self.month == 1 {
            Self {
                year: self.year - 1,
                month: 12,
            }
        } else {
            Self {
                month: self.month - 1,
                ..self
            }
        }
    }

    /// File name, for example `september_generator2026.xlsx`.
    #[must_use]
    pub fn file_name(self) -> String {
        let idx = (self.month.clamp(1, 12) - 1) as usize;
        format!("{}_generator{}.xlsx", MONTHS[idx], self.year)
    }

    /// URLs the file may live at under `base` (default host
    /// `https://www.eia.gov`): EIA keeps recent months in `xls/` and moves
    /// older ones to `archive/xls/`, so both are tried, `xls/` first.
    #[must_use]
    pub fn urls(self, base: &str) -> [String; 2] {
        let f = self.file_name();
        let base = base.trim_end_matches('/');
        [
            format!("{base}/electricity/data/eia860m/xls/{f}"),
            format!("{base}/electricity/data/eia860m/archive/xls/{f}"),
        ]
    }
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year, self.month)
    }
}

/// A sheet of the workbook that this crate can parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sheet {
    /// Operating generators (and standby, out-of-service ones).
    Operating,
    /// Planned generators (permitted through under construction).
    Planned,
    /// Retired generators.
    Retired,
    /// Canceled or indefinitely postponed generators.
    CanceledOrPostponed,
}

impl Sheet {
    /// The worksheet name inside the workbook.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Operating => "Operating",
            Self::Planned => "Planned",
            Self::Retired => "Retired",
            Self::CanceledOrPostponed => "Canceled or Postponed",
        }
    }
}

/// A calendar month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct YearMonth {
    /// Year.
    pub year: u16,
    /// Month, 1-12.
    pub month: u8,
}

/// One generator row. Columns not modelled below (and any EIA adds later) are
/// kept as strings in [`GeneratorRow::extra`], keyed by their header.
///
/// Which fields are populated depends on the sheet: `operating` is set on
/// Operating and Retired, `planned_operation` on Planned, `retirement` on
/// Retired, `planned_retirement` on Operating.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratorRow {
    /// Owner/operator entity id.
    pub entity_id: Option<u32>,
    /// Owner/operator name.
    pub entity_name: String,
    /// Plant id.
    pub plant_id: u32,
    /// Plant name.
    pub plant_name: String,
    /// Plant state postal code.
    pub state: String,
    /// County.
    pub county: String,
    /// Balancing authority code (blank for some small plants).
    pub balancing_authority: String,
    /// Sector, for example `IPP Non-CHP`.
    pub sector: String,
    /// Generator id within the plant.
    pub generator_id: String,
    /// Unit code.
    pub unit_code: String,
    /// Nameplate capacity, MW.
    pub nameplate_mw: Option<f64>,
    /// Net summer capacity, MW.
    pub net_summer_mw: Option<f64>,
    /// Net winter capacity, MW.
    pub net_winter_mw: Option<f64>,
    /// Technology.
    pub technology: String,
    /// Energy source code, for example `SUN`.
    pub energy_source_code: String,
    /// Prime mover code, for example `PV`.
    pub prime_mover_code: String,
    /// Status text as published, for example `(OP) Operating`.
    pub status: String,
    /// Latitude.
    pub latitude: Option<f64>,
    /// Longitude.
    pub longitude: Option<f64>,
    /// Commercial operation month (Operating, Retired).
    pub operating: Option<YearMonth>,
    /// Planned operation month (Planned).
    pub planned_operation: Option<YearMonth>,
    /// Planned retirement month (Operating).
    pub planned_retirement: Option<YearMonth>,
    /// Actual retirement month (Retired).
    pub retirement: Option<YearMonth>,
    /// Every other column as published, header to trimmed text (empty cells
    /// omitted).
    pub extra: BTreeMap<String, String>,
}

impl GeneratorRow {
    /// The status code inside the leading parentheses, `OP` for
    /// `(OP) Operating`.
    #[must_use]
    pub fn status_code(&self) -> Option<&str> {
        let rest = self.status.strip_prefix('(')?;
        rest.split_once(')').map(|(code, _)| code)
    }
}

impl Client {
    /// Download one release of the workbook to `dest` (tries `xls/` then
    /// `archive/xls/`). [`Error::NotPublished`] if neither has it.
    pub async fn download_eia860m(&self, release: Release, dest: &Path) -> Result<u64> {
        let mut last = Error::NotPublished(release.file_name());
        for url in release.urls(self.bulk_base_url()) {
            match self.download_to(&url, dest, Some(XLSX_MAGIC)).await {
                Err(e @ Error::NotPublished(_)) => last = e,
                other => return other,
            }
        }
        Err(last)
    }

    /// Download the newest published release at or before `from`, looking
    /// back up to 6 months (EIA lags month end by days to weeks). Returns
    /// the release that was saved.
    pub async fn download_latest_eia860m(&self, from: Release, dest: &Path) -> Result<Release> {
        let mut release = from;
        for _ in 0..6 {
            match self.download_eia860m(release, dest).await {
                Ok(_) => return Ok(release),
                Err(Error::NotPublished(_)) => release = release.previous(),
                Err(e) => return Err(e),
            }
        }
        Err(Error::NotPublished(format!(
            "no EIA-860M release found in the 6 months up to {from}"
        )))
    }
}

/// Parse one sheet of a downloaded workbook. Blocking; the workbook is
/// 14 MB so expect around a second.
pub fn read_sheet(path: &Path, sheet: Sheet) -> Result<Vec<GeneratorRow>> {
    let file = std::fs::File::open(path)?;
    parse_sheet(BufReader::new(file), sheet)
}

/// Parse one sheet from any seekable reader (for example a `Cursor` over
/// downloaded bytes).
pub fn parse_sheet<RS: Read + Seek>(reader: RS, sheet: Sheet) -> Result<Vec<GeneratorRow>> {
    let mut wb: Xlsx<RS> =
        Xlsx::new(reader).map_err(|e| Error::Bulk(format!("open workbook: {e}")))?;
    let range = wb
        .worksheet_range(sheet.name())
        .map_err(|e| Error::Bulk(format!("sheet {:?}: {e}", sheet.name())))?;
    let rows: Vec<Vec<String>> = range
        .rows()
        .map(|r| r.iter().map(cell_text).collect())
        .collect();
    rows_to_generators(&rows)
}

/// Locate the header row (first cell `Entity ID`) and map every data row
/// below it. Rows without a numeric Plant ID (blank spacer, `NOTES:`
/// footer) are skipped. Pure.
pub(crate) fn rows_to_generators(rows: &[Vec<String>]) -> Result<Vec<GeneratorRow>> {
    let (header_at, header) = rows
        .iter()
        .enumerate()
        .find(|(_, r)| r.first().is_some_and(|c| c == "Entity ID"))
        .ok_or_else(|| Error::Bulk("no header row starting with `Entity ID`".into()))?;
    let index: HashMap<&str, usize> = header
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty())
        .map(|(i, h)| (h.as_str(), i))
        .collect();
    Ok(rows[header_at + 1..]
        .iter()
        .filter_map(|r| to_generator(header, &index, r))
        .collect())
}

const MODELLED: &[&str] = &[
    "Entity ID",
    "Entity Name",
    "Plant ID",
    "Plant Name",
    "Google Map",
    "Bing Map",
    "Plant State",
    "County",
    "Balancing Authority Code",
    "Sector",
    "Generator ID",
    "Unit Code",
    "Nameplate Capacity (MW)",
    "Net Summer Capacity (MW)",
    "Net Winter Capacity (MW)",
    "Technology",
    "Energy Source Code",
    "Prime Mover Code",
    "Status",
    "Latitude",
    "Longitude",
    "Operating Month",
    "Operating Year",
    "Planned Operation Month",
    "Planned Operation Year",
    "Planned Retirement Month",
    "Planned Retirement Year",
    "Retirement Month",
    "Retirement Year",
];

fn to_generator(
    header: &[String],
    index: &HashMap<&str, usize>,
    row: &[String],
) -> Option<GeneratorRow> {
    let text = |name: &str| -> String {
        index
            .get(name)
            .and_then(|&i| row.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let num = |name: &str| text(name).parse::<f64>().ok();
    let ym = |month: &str, year: &str| -> Option<YearMonth> {
        let m = text(month)
            .parse::<u8>()
            .ok()
            .filter(|m| (1..=12).contains(m))?;
        let y = text(year).parse::<u16>().ok()?;
        Some(YearMonth { year: y, month: m })
    };
    let plant_id = text("Plant ID").parse::<u32>().ok()?;
    let extra = header
        .iter()
        .enumerate()
        .filter(|(_, h)| !h.is_empty() && !MODELLED.contains(&h.as_str()))
        .filter_map(|(i, h)| {
            let v = row.get(i)?.trim();
            (!v.is_empty()).then(|| (h.clone(), v.to_owned()))
        })
        .collect();
    Some(GeneratorRow {
        entity_id: text("Entity ID").parse().ok(),
        entity_name: text("Entity Name"),
        plant_id,
        plant_name: text("Plant Name"),
        state: text("Plant State"),
        county: text("County"),
        balancing_authority: text("Balancing Authority Code"),
        sector: text("Sector"),
        generator_id: text("Generator ID"),
        unit_code: text("Unit Code"),
        nameplate_mw: num("Nameplate Capacity (MW)"),
        net_summer_mw: num("Net Summer Capacity (MW)"),
        net_winter_mw: num("Net Winter Capacity (MW)"),
        technology: text("Technology"),
        energy_source_code: text("Energy Source Code"),
        prime_mover_code: text("Prime Mover Code"),
        status: text("Status"),
        latitude: num("Latitude"),
        longitude: num("Longitude"),
        operating: ym("Operating Month", "Operating Year"),
        planned_operation: ym("Planned Operation Month", "Planned Operation Year"),
        planned_retirement: ym("Planned Retirement Month", "Planned Retirement Year"),
        retirement: ym("Retirement Month", "Retirement Year"),
        extra,
    })
}

/// Render a cell the way a human reads it: whole-number floats without `.0`.
fn cell_text(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_owned(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{f:.0}"),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn release_file_names_and_urls() {
        let r = Release::new(2026, 9).unwrap();
        assert_eq!(r.file_name(), "september_generator2026.xlsx");
        let [a, b] = r.urls("https://www.eia.gov/");
        assert_eq!(
            a,
            "https://www.eia.gov/electricity/data/eia860m/xls/september_generator2026.xlsx"
        );
        assert_eq!(
            b,
            "https://www.eia.gov/electricity/data/eia860m/archive/xls/september_generator2026.xlsx"
        );
        assert_eq!(r.to_string(), "2026-09");
        assert!(Release::new(2026, 13).is_err());
        assert!(Release::new(2026, 0).is_err());
    }

    #[test]
    fn previous_wraps_year() {
        assert_eq!(
            Release {
                year: 2026,
                month: 1
            }
            .previous(),
            Release {
                year: 2025,
                month: 12
            }
        );
        assert_eq!(
            Release {
                year: 2026,
                month: 5
            }
            .previous(),
            Release {
                year: 2026,
                month: 4
            }
        );
        assert!(
            Release {
                year: 2025,
                month: 12
            } < Release {
                year: 2026,
                month: 1
            }
        );
    }

    #[test]
    fn cell_text_normalises_numbers() {
        assert_eq!(cell_text(&Data::Float(63560.0)), "63560");
        assert_eq!(cell_text(&Data::Float(0.9)), "0.9");
        assert_eq!(cell_text(&Data::Int(12)), "12");
        assert_eq!(cell_text(&Data::String("  x ".into())), "x");
        assert_eq!(cell_text(&Data::Empty), "");
    }

    fn operating_rows() -> Vec<Vec<String>> {
        vec![
            s(&["Inventory of Operating Generators as of September 2026"]),
            s(&["", "", ""]),
            s(&[
                "Entity ID",
                "Entity Name",
                "Plant ID",
                "Plant Name",
                "Google Map",
                "Bing Map",
                "Plant State",
                "County",
                "Balancing Authority Code",
                "Sector",
                "Generator ID",
                "Unit Code",
                "Nameplate Capacity (MW)",
                "Net Summer Capacity (MW)",
                "Net Winter Capacity (MW)",
                "Technology",
                "Energy Source Code",
                "Prime Mover Code",
                "Operating Month",
                "Operating Year",
                "Planned Retirement Month",
                "Planned Retirement Year",
                "Status",
                "Nameplate Energy Capacity (MWh)",
                "Latitude",
                "Longitude",
            ]),
            s(&[
                "61227",
                "Nautilus Solar Solutions",
                "70578",
                "Carroll County MD Solar LLC",
                "Map",
                "Map",
                "MD",
                "Carroll",
                "PJM",
                "IPP Non-CHP",
                "19132",
                "",
                "5",
                "5",
                "5",
                "Solar Photovoltaic",
                "SUN",
                "PV",
                "9",
                "2025",
                "",
                "",
                "(OP) Operating",
                "12.5",
                "39.58",
                "-76.84",
            ]),
            s(&["", "", ""]),
            s(&["NOTES:\nCapacity from facilities"]),
        ]
    }

    #[test]
    fn operating_rows_map_to_typed_generators_and_skip_footer() {
        let gens = rows_to_generators(&operating_rows()).unwrap();
        assert_eq!(gens.len(), 1, "title, spacer and NOTES rows are skipped");
        let g = &gens[0];
        assert_eq!(g.plant_id, 70578);
        assert_eq!(g.entity_id, Some(61227));
        assert_eq!(g.state, "MD");
        assert_eq!(g.balancing_authority, "PJM");
        assert_eq!(g.nameplate_mw, Some(5.0));
        assert_eq!(
            g.operating,
            Some(YearMonth {
                year: 2025,
                month: 9
            })
        );
        assert_eq!(g.planned_retirement, None);
        assert_eq!(g.status_code(), Some("OP"));
        assert_eq!(g.latitude, Some(39.58));
        assert_eq!(
            g.extra
                .get("Nameplate Energy Capacity (MWh)")
                .map(String::as_str),
            Some("12.5")
        );
        assert!(!g.extra.contains_key("Google Map"));
    }

    #[test]
    fn planned_sheet_columns_map_to_planned_operation() {
        let rows = vec![
            s(&["Inventory of Planned Generators as of September 2026"]),
            s(&[
                "Entity ID",
                "Entity Name",
                "Plant ID",
                "Plant Name",
                "Plant State",
                "Planned Operation Month",
                "Planned Operation Year",
                "Status",
                "Nameplate Capacity (MW)",
            ]),
            s(&[
                "61222",
                "174 Power Global",
                "63798",
                "Atlas",
                "AZ",
                "9",
                "2026",
                "(V) Under construction",
                "300",
            ]),
        ];
        let gens = rows_to_generators(&rows).unwrap();
        assert_eq!(
            gens[0].planned_operation,
            Some(YearMonth {
                year: 2026,
                month: 9
            })
        );
        assert_eq!(gens[0].operating, None);
        assert_eq!(gens[0].status_code(), Some("V"));
    }

    #[test]
    fn missing_header_is_an_error() {
        let rows = vec![s(&["just", "text"])];
        assert!(matches!(rows_to_generators(&rows), Err(Error::Bulk(_))));
    }

    #[test]
    fn out_of_range_month_is_none_not_a_panic() {
        let rows = vec![
            s(&["Entity ID", "Plant ID", "Operating Month", "Operating Year"]),
            s(&["1", "2", "13", "2020"]),
        ];
        assert_eq!(rows_to_generators(&rows).unwrap()[0].operating, None);
    }
}

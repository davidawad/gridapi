//! EIA-930 six-month bulk CSVs, `EIA930_<DATASET>_<year>_<Jan_Jun|Jul_Dec>.csv`
//! under `https://www.eia.gov/electricity/gridmonitor/sixMonthFiles/`.
//!
//! They hold what the API's `rto` routes do not: imputed and adjusted series,
//! the `Sum(Valid DIBAs)` check column and a long history. Files are large
//! (balance about 45 MB, interchange about 100 MB per half year), so
//! [`Reader`]s iterate rows lazily.
//!
//! ```no_run
//! use gridapi_eia::bulk::eia930::{BalanceReader, Dataset, HalfYear, SixMonthFile};
//! use gridapi_eia::Client;
//!
//! # async fn demo() -> Result<(), gridapi_eia::Error> {
//! let client = Client::from_env()?;
//! let file = SixMonthFile { dataset: Dataset::Balance, year: 2026, half: HalfYear::JulDec };
//! let dest = std::path::Path::new("balance.csv");
//! client.download_eia930(file, dest).await?;
//! for row in BalanceReader::open(dest)?.take(3) {
//!     let row = row?;
//!     println!("{} {} {:?}", row.balancing_authority, row.utc_end, row.demand_mw);
//! }
//! # Ok(()) }
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime};

use crate::client::Client;
use crate::error::{Error, Result};

/// Which of the three published files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dataset {
    /// `BALANCE`: demand, forecast, generation by fuel, total interchange per BA.
    Balance,
    /// `SUBREGION`: demand per BA subregion.
    Subregion,
    /// `INTERCHANGE`: flows between directly interconnected BAs.
    Interchange,
}

impl Dataset {
    const fn token(self) -> &'static str {
        match self {
            Self::Balance => "BALANCE",
            Self::Subregion => "SUBREGION",
            Self::Interchange => "INTERCHANGE",
        }
    }
}

/// First or second half of a calendar year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HalfYear {
    /// January to June.
    JanJun,
    /// July to December.
    JulDec,
}

impl HalfYear {
    const fn token(self) -> &'static str {
        match self {
            Self::JanJun => "Jan_Jun",
            Self::JulDec => "Jul_Dec",
        }
    }
}

/// One six-month file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SixMonthFile {
    /// Which dataset.
    pub dataset: Dataset,
    /// Calendar year.
    pub year: i32,
    /// Which half.
    pub half: HalfYear,
}

impl SixMonthFile {
    /// File name, for example `EIA930_BALANCE_2026_Jul_Dec.csv`.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!(
            "EIA930_{}_{}_{}.csv",
            self.dataset.token(),
            self.year,
            self.half.token()
        )
    }

    /// Download URL under `base` (default host `https://www.eia.gov`).
    #[must_use]
    pub fn url(&self, base: &str) -> String {
        format!(
            "{}/electricity/gridmonitor/sixMonthFiles/{}",
            base.trim_end_matches('/'),
            self.file_name()
        )
    }
}

impl Client {
    /// Download one six-month file to `dest`. The current half year's file
    /// is rewritten by EIA as data arrives. [`Error::NotPublished`] for a
    /// half that does not exist yet.
    pub async fn download_eia930(&self, file: SixMonthFile, dest: &Path) -> Result<u64> {
        self.download_to(&file.url(self.bulk_base_url()), dest, None)
            .await
    }
}

/// A raw CSV record addressed by header name.
struct Fields<'a> {
    index: &'a HashMap<String, usize>,
    record: &'a csv::StringRecord,
}

impl Fields<'_> {
    fn text(&self, name: &str) -> &str {
        self.index
            .get(name)
            .and_then(|&i| self.record.get(i))
            .map_or("", str::trim)
    }

    fn required(&self, name: &str) -> Result<&str> {
        let v = self.text(name);
        if v.is_empty() {
            Err(Error::Bulk(format!("missing value in column {name:?}")))
        } else {
            Ok(v)
        }
    }

    fn num(&self, name: &str) -> Option<f64> {
        parse_num(self.text(name))
    }

    fn date(&self, name: &str) -> Result<NaiveDate> {
        let v = self.required(name)?;
        NaiveDate::parse_from_str(v, "%m/%d/%Y")
            .map_err(|e| Error::Bulk(format!("column {name:?}: bad date {v:?}: {e}")))
    }

    fn timestamp(&self, name: &str) -> Result<NaiveDateTime> {
        let v = self.required(name)?;
        parse_timestamp(v)
            .map_err(|e| Error::Bulk(format!("column {name:?}: bad timestamp {v:?}: {e}")))
    }

    fn hour(&self) -> Result<u8> {
        let v = self.required("Hour Number")?;
        v.parse()
            .map_err(|_| Error::Bulk(format!("bad Hour Number {v:?}")))
    }
}

/// `1,234` (thousands separators) and blanks are tolerated.
fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    s.replace(',', "").parse().ok()
}

/// `07/01/2026 7:00:00 AM` (no zero padding on the hour).
fn parse_timestamp(s: &str) -> std::result::Result<NaiveDateTime, chrono::ParseError> {
    NaiveDateTime::parse_from_str(s, "%m/%d/%Y %I:%M:%S %p")
}

/// Lazily iterates a six-month CSV, mapping each record with `map`.
pub struct Reader<R: Read, T> {
    records: csv::StringRecordsIntoIter<R>,
    index: HashMap<String, usize>,
    /// Headers outside the typed set, in file order.
    extra: Arc<[String]>,
    map: fn(&Fields<'_>, &Arc<[String]>) -> Result<T>,
}

impl<R: Read, T> Reader<R, T> {
    fn new(
        rdr: R,
        typed: &[&str],
        map: fn(&Fields<'_>, &Arc<[String]>) -> Result<T>,
    ) -> Result<Self> {
        let mut csv = csv::ReaderBuilder::new().flexible(true).from_reader(rdr);
        let headers: Vec<String> = csv
            .headers()
            .map_err(|e| Error::Bulk(format!("csv header: {e}")))?
            .iter()
            .map(|h| h.trim().to_owned())
            .collect();
        let index = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.clone(), i))
            .collect();
        let extra: Arc<[String]> = headers
            .iter()
            .filter(|h| !typed.contains(&h.as_str()))
            .cloned()
            .collect();
        Ok(Self {
            records: csv.into_records(),
            index,
            extra,
            map,
        })
    }
}

impl<R: Read, T> Iterator for Reader<R, T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let record = match self.records.next()? {
            Ok(r) => r,
            Err(e) => return Some(Err(Error::Bulk(format!("csv record: {e}")))),
        };
        let fields = Fields {
            index: &self.index,
            record: &record,
        };
        Some((self.map)(&fields, &self.extra))
    }
}

/// One BA-hour of the BALANCE file.
///
/// Raw (as reported) demand, forecast, generation and interchange are typed
/// fields. The other numeric columns (fuel breakdown, imputed, adjusted,
/// `Sum(Valid DIBAs) (MW)`) are available by header through
/// [`BalanceRow::value`].
#[derive(Debug, Clone, PartialEq)]
pub struct BalanceRow {
    /// BA code.
    pub balancing_authority: String,
    /// Local data date.
    pub data_date: NaiveDate,
    /// Hour number within the local day, 1-24 (23 or 25 on DST changes).
    pub hour_number: u8,
    /// End of hour, local clock (naive).
    pub local_end: NaiveDateTime,
    /// End of hour, UTC (naive).
    pub utc_end: NaiveDateTime,
    /// Day-ahead demand forecast, MW.
    pub demand_forecast_mw: Option<f64>,
    /// Demand, MW.
    pub demand_mw: Option<f64>,
    /// Net generation, MW.
    pub net_generation_mw: Option<f64>,
    /// Total interchange, MW.
    pub total_interchange_mw: Option<f64>,
    /// Region code (last column).
    pub region: Option<String>,
    names: Arc<[String]>,
    values: Vec<Option<f64>>,
}

impl BalanceRow {
    /// Any numeric column by exact header, for example
    /// `Net Generation (MW) from Natural Gas` or
    /// `Demand (MW) (Adjusted)`. `None` when the column is absent or blank.
    #[must_use]
    pub fn value(&self, header: &str) -> Option<f64> {
        let i = self.names.iter().position(|n| n == header)?;
        self.values.get(i).copied().flatten()
    }

    /// Headers of all numeric columns carried by [`BalanceRow::value`].
    #[must_use]
    pub fn value_headers(&self) -> &[String] {
        &self.names
    }
}

/// Iterator over a BALANCE file.
pub type BalanceReader<R = File> = Reader<R, BalanceRow>;

const BALANCE_TYPED: [&str; 10] = [
    "Balancing Authority",
    "Data Date",
    "Hour Number",
    "Local Time at End of Hour",
    "UTC Time at End of Hour",
    "Demand Forecast (MW)",
    "Demand (MW)",
    "Net Generation (MW)",
    "Total Interchange (MW)",
    "Region",
];

fn balance_row(f: &Fields<'_>, numeric: &Arc<[String]>) -> Result<BalanceRow> {
    Ok(BalanceRow {
        balancing_authority: f.required("Balancing Authority")?.to_owned(),
        data_date: f.date("Data Date")?,
        hour_number: f.hour()?,
        local_end: f.timestamp("Local Time at End of Hour")?,
        utc_end: f.timestamp("UTC Time at End of Hour")?,
        demand_forecast_mw: f.num("Demand Forecast (MW)"),
        demand_mw: f.num("Demand (MW)"),
        net_generation_mw: f.num("Net Generation (MW)"),
        total_interchange_mw: f.num("Total Interchange (MW)"),
        region: Some(f.text("Region").to_owned()).filter(|r| !r.is_empty()),
        values: numeric.iter().map(|h| f.num(h)).collect(),
        names: Arc::clone(numeric),
    })
}

impl BalanceReader<File> {
    /// Open a downloaded BALANCE file.
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_reader(File::open(path)?)
    }
}

impl<R: Read> BalanceReader<R> {
    /// Read BALANCE rows from any reader.
    pub fn from_reader(rdr: R) -> Result<Self> {
        Reader::new(rdr, &BALANCE_TYPED, balance_row)
    }
}

/// One BA-subregion-hour of the SUBREGION file.
#[derive(Debug, Clone, PartialEq)]
pub struct SubregionRow {
    /// Parent BA code.
    pub balancing_authority: String,
    /// Local data date.
    pub data_date: NaiveDate,
    /// Hour number within the local day.
    pub hour_number: u8,
    /// Subregion code.
    pub sub_region: String,
    /// Demand, MW.
    pub demand_mw: Option<f64>,
    /// End of hour, local clock.
    pub local_end: NaiveDateTime,
    /// End of hour, UTC.
    pub utc_end: NaiveDateTime,
}

/// Iterator over a SUBREGION file.
pub type SubregionReader<R = File> = Reader<R, SubregionRow>;

fn subregion_row(f: &Fields<'_>, _: &Arc<[String]>) -> Result<SubregionRow> {
    Ok(SubregionRow {
        balancing_authority: f.required("Balancing Authority")?.to_owned(),
        data_date: f.date("Data Date")?,
        hour_number: f.hour()?,
        sub_region: f.required("Sub-Region")?.to_owned(),
        demand_mw: f.num("Demand (MW)"),
        local_end: f.timestamp("Local Time at End of Hour")?,
        utc_end: f.timestamp("UTC Time at End of Hour")?,
    })
}

impl SubregionReader<File> {
    /// Open a downloaded SUBREGION file.
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_reader(File::open(path)?)
    }
}

impl<R: Read> SubregionReader<R> {
    /// Read SUBREGION rows from any reader.
    pub fn from_reader(rdr: R) -> Result<Self> {
        Reader::new(rdr, &[], subregion_row)
    }
}

/// One BA-to-neighbour-hour of the INTERCHANGE file.
#[derive(Debug, Clone, PartialEq)]
pub struct InterchangeRow {
    /// Reporting BA.
    pub balancing_authority: String,
    /// Local data date.
    pub data_date: NaiveDate,
    /// Hour number within the local day.
    pub hour_number: u8,
    /// Directly interconnected BA.
    pub diba: String,
    /// Interchange, MW (EIA-930 sign convention).
    pub interchange_mw: Option<f64>,
    /// End of hour, local clock.
    pub local_end: NaiveDateTime,
    /// End of hour, UTC.
    pub utc_end: NaiveDateTime,
    /// Region of the reporting BA.
    pub region: Option<String>,
    /// Region of the neighbour.
    pub diba_region: Option<String>,
}

/// Iterator over an INTERCHANGE file.
pub type InterchangeReader<R = File> = Reader<R, InterchangeRow>;

fn interchange_row(f: &Fields<'_>, _: &Arc<[String]>) -> Result<InterchangeRow> {
    let opt = |name| Some(f.text(name).to_owned()).filter(|v: &String| !v.is_empty());
    Ok(InterchangeRow {
        balancing_authority: f.required("Balancing Authority")?.to_owned(),
        data_date: f.date("Data Date")?,
        hour_number: f.hour()?,
        diba: f
            .required("Directly Interconnected Balancing Authority")?
            .to_owned(),
        interchange_mw: f.num("Interchange (MW)"),
        local_end: f.timestamp("Local Time at End of Hour")?,
        utc_end: f.timestamp("UTC Time at End of Hour")?,
        region: opt("Region"),
        diba_region: opt("DIBA_Region"),
    })
}

impl InterchangeReader<File> {
    /// Open a downloaded INTERCHANGE file.
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_reader(File::open(path)?)
    }
}

impl<R: Read> InterchangeReader<R> {
    /// Read INTERCHANGE rows from any reader.
    pub fn from_reader(rdr: R) -> Result<Self> {
        Reader::new(rdr, &[], interchange_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_month_file_names_and_urls() {
        let f = SixMonthFile {
            dataset: Dataset::Balance,
            year: 2026,
            half: HalfYear::JulDec,
        };
        assert_eq!(f.file_name(), "EIA930_BALANCE_2026_Jul_Dec.csv");
        assert_eq!(
            f.url("https://www.eia.gov/"),
            "https://www.eia.gov/electricity/gridmonitor/sixMonthFiles/EIA930_BALANCE_2026_Jul_Dec.csv"
        );
        let g = SixMonthFile {
            dataset: Dataset::Interchange,
            year: 2024,
            half: HalfYear::JanJun,
        };
        assert_eq!(g.file_name(), "EIA930_INTERCHANGE_2024_Jan_Jun.csv");
    }

    #[test]
    fn timestamp_and_number_parsing() {
        let t = parse_timestamp("07/01/2026 7:00:00 AM").unwrap();
        assert_eq!(t.to_string(), "2026-07-01 07:00:00");
        let t = parse_timestamp("07/01/2026 12:00:00 AM").unwrap();
        assert_eq!(t.to_string(), "2026-07-01 00:00:00");
        let t = parse_timestamp("11/01/2026 11:00:00 PM").unwrap();
        assert_eq!(t.to_string(), "2026-11-01 23:00:00");
        assert!(parse_timestamp("garbage").is_err());
        assert_eq!(parse_num("1,234"), Some(1234.0));
        assert_eq!(parse_num(" -5 "), Some(-5.0));
        assert_eq!(parse_num(""), None);
        assert_eq!(parse_num("x"), None);
    }

    const BALANCE: &str = include_str!("../../tests/fixtures/eia930_balance_sample.csv");

    #[test]
    fn balance_rows_parse_typed_and_by_header() {
        let rows: Vec<BalanceRow> = BalanceReader::from_reader(BALANCE.as_bytes())
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(rows.len(), 3);
        let r = &rows[0];
        assert_eq!(r.balancing_authority, "AECI");
        assert_eq!(r.data_date.to_string(), "2026-07-01");
        assert_eq!(r.hour_number, 1);
        assert_eq!(r.utc_end.to_string(), "2026-07-01 06:00:00");
        assert_eq!(r.demand_forecast_mw, Some(2633.0));
        assert_eq!(r.demand_mw, Some(2707.0));
        assert_eq!(r.net_generation_mw, Some(2702.0));
        assert_eq!(r.total_interchange_mw, Some(-5.0));
        assert_eq!(r.region.as_deref(), Some("MIDW"));
        assert_eq!(r.value("Sum(Valid DIBAs) (MW)"), Some(-5.0));
        assert_eq!(r.value("Net Generation (MW) from Coal"), Some(917.0));
        assert_eq!(r.value("Demand (MW) (Adjusted)"), Some(2707.0));
        assert_eq!(r.value("Demand (MW) (Imputed)"), None, "blank cell");
        assert_eq!(r.value("No Such Column"), None);
        assert!(!r.value_headers().iter().any(|h| h == "Demand (MW)"));
    }

    #[test]
    fn malformed_row_is_an_error_item_not_a_panic() {
        let bad = "Balancing Authority,Data Date,Hour Number,Local Time at End of Hour,UTC Time at End of Hour\n\
                   AECI,not-a-date,1,07/01/2026 1:00:00 AM,07/01/2026 6:00:00 AM\n";
        let mut it = BalanceReader::from_reader(bad.as_bytes()).unwrap();
        assert!(matches!(it.next(), Some(Err(Error::Bulk(_)))));
        assert!(it.next().is_none());
    }

    #[test]
    fn subregion_and_interchange_parse() {
        let sub = "\"Balancing Authority\",\"Data Date\",\"Hour Number\",\"Sub-Region\",\"Demand (MW)\",\"Local Time at End of Hour\",\"UTC Time at End of Hour\"\n\
                   BHBA,07/01/2026,1,BASI,207,07/01/2026 1:00:00 AM,07/01/2026 7:00:00 AM\n";
        let rows: Vec<_> = SubregionReader::from_reader(sub.as_bytes())
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(
            (rows[0].sub_region.as_str(), rows[0].demand_mw),
            ("BASI", Some(207.0))
        );

        let ic = "\"Balancing Authority\",\"Data Date\",\"Hour Number\",\"Directly Interconnected Balancing Authority\",\"Interchange (MW)\",\"Local Time at End of Hour\",\"UTC Time at End of Hour\",\"Region\",\"DIBA_Region\"\n\
                  AECI,07/01/2026,1,SPA,-312,07/01/2026 1:00:00 AM,07/01/2026 6:00:00 AM,MIDW,CENT\n";
        let rows: Vec<_> = InterchangeReader::from_reader(ic.as_bytes())
            .unwrap()
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(rows[0].diba, "SPA");
        assert_eq!(rows[0].interchange_mw, Some(-312.0));
        assert_eq!(rows[0].diba_region.as_deref(), Some("CENT"));
    }
}

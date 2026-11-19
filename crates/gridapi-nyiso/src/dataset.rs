//! The NYISO MIS public CSV datasets and how their files are named.
//!
//! Daily files live at `<base>/<dir>/<YYYYMMDD><dir>[_zone|_gen].csv`. Older days
//! are only kept in monthly archives at `<base>/<dir>/<YYYYMM01><dir>[_zone|_gen]_csv.zip`,
//! which hold one daily CSV per day under the daily file's name.

use std::fmt;

use chrono::{Datelike, NaiveDate};

/// Whether an LBMP file is per pricing zone or per generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    /// One row per load zone (about 15 zones).
    Zone,
    /// One row per generator bus (hundreds of rows per interval).
    Generator,
}

impl Level {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Zone => "_zone",
            Self::Generator => "_gen",
        }
    }
}

/// A NYISO public CSV dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Dataset {
    /// Day-ahead hourly LBMP (`damlbmp`).
    DayAheadLbmp(Level),
    /// Real-time five-minute LBMP (`realtime`).
    RealTimeLbmp(Level),
    /// Real-time hourly integrated LBMP (`rtlbmp`).
    IntegratedRealTimeLbmp(Level),
    /// Five-minute actual load by zone (`pal`).
    ActualLoad,
    /// Hourly integrated actual load by zone (`palIntegrated`).
    IntegratedLoad,
    /// Hourly zonal load forecast (`isolf`).
    LoadForecast,
    /// Five-minute generation by fuel category (`rtfuelmix`).
    FuelMix,
    /// Day-ahead hourly ancillary service prices (`damasp`).
    DayAheadAncillary,
    /// Real-time five-minute ancillary service prices (`rtasp`).
    RealTimeAncillary,
    /// Interface flows and limits (`ExternalLimitsFlows`).
    InterfaceFlows,
}

impl Dataset {
    /// The directory (and file stem) NYISO uses for the dataset.
    #[must_use]
    pub const fn directory(self) -> &'static str {
        match self {
            Self::DayAheadLbmp(_) => "damlbmp",
            Self::RealTimeLbmp(_) => "realtime",
            Self::IntegratedRealTimeLbmp(_) => "rtlbmp",
            Self::ActualLoad => "pal",
            Self::IntegratedLoad => "palIntegrated",
            Self::LoadForecast => "isolf",
            Self::FuelMix => "rtfuelmix",
            Self::DayAheadAncillary => "damasp",
            Self::RealTimeAncillary => "rtasp",
            Self::InterfaceFlows => "ExternalLimitsFlows",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::DayAheadLbmp(l) | Self::RealTimeLbmp(l) | Self::IntegratedRealTimeLbmp(l) => {
                l.suffix()
            }
            _ => "",
        }
    }

    /// The daily CSV file name for `day`, e.g. `20261117damlbmp_zone.csv`.
    /// The same name is used for the entry inside a monthly archive.
    #[must_use]
    pub fn daily_file(self, day: NaiveDate) -> String {
        format!(
            "{}{}{}.csv",
            day.format("%Y%m%d"),
            self.directory(),
            self.suffix()
        )
    }

    /// The monthly archive name covering `day`, e.g. `20261101damlbmp_zone_csv.zip`.
    #[must_use]
    pub fn archive_file(self, day: NaiveDate) -> String {
        format!(
            "{:04}{:02}01{}{}_csv.zip",
            day.year(),
            day.month(),
            self.directory(),
            self.suffix()
        )
    }
}

impl fmt::Display for Dataset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.directory(), self.suffix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[test]
    fn daily_names_match_the_published_pattern() {
        let day = d(2026, 11, 17);
        let cases = [
            (
                Dataset::DayAheadLbmp(Level::Zone),
                "20261117damlbmp_zone.csv",
            ),
            (
                Dataset::DayAheadLbmp(Level::Generator),
                "20261117damlbmp_gen.csv",
            ),
            (
                Dataset::RealTimeLbmp(Level::Zone),
                "20261117realtime_zone.csv",
            ),
            (
                Dataset::IntegratedRealTimeLbmp(Level::Zone),
                "20261117rtlbmp_zone.csv",
            ),
            (Dataset::ActualLoad, "20261117pal.csv"),
            (Dataset::IntegratedLoad, "20261117palIntegrated.csv"),
            (Dataset::LoadForecast, "20261117isolf.csv"),
            (Dataset::FuelMix, "20261117rtfuelmix.csv"),
            (Dataset::DayAheadAncillary, "20261117damasp.csv"),
            (Dataset::RealTimeAncillary, "20261117rtasp.csv"),
            (Dataset::InterfaceFlows, "20261117ExternalLimitsFlows.csv"),
        ];
        for (dataset, expected) in cases {
            assert_eq!(dataset.daily_file(day), expected, "{dataset}");
        }
    }

    #[test]
    fn archive_names_use_the_first_of_the_month() {
        // Verified live: LBMP archives carry the level, others do not.
        let day = d(2026, 11, 17);
        assert_eq!(
            Dataset::DayAheadLbmp(Level::Zone).archive_file(day),
            "20261101damlbmp_zone_csv.zip"
        );
        assert_eq!(
            Dataset::RealTimeLbmp(Level::Generator).archive_file(day),
            "20261101realtime_gen_csv.zip"
        );
        assert_eq!(
            Dataset::FuelMix.archive_file(day),
            "20261101rtfuelmix_csv.zip"
        );
        assert_eq!(
            Dataset::ActualLoad.archive_file(d(2001, 1, 31)),
            "20010101pal_csv.zip"
        );
    }

    #[test]
    fn display_names_the_file_family() {
        assert_eq!(
            Dataset::DayAheadLbmp(Level::Zone).to_string(),
            "damlbmp_zone"
        );
        assert_eq!(Dataset::LoadForecast.to_string(), "isolf");
    }
}

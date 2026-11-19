//! Naming of IESO report files.
//!
//! A file lives at `<base>/<Report>/PUB_<Report>[_<stamp>][_v<N>].<ext>`:
//!
//! * `stamp` is `YYYYMMDD` (daily reports), `YYYYMMDDHH` (hourly, hour-ending
//!   01..=24) or `YYYY` (yearly), or absent for the rolling "current" file.
//! * `_v<N>` is a revision. The file *without* `_v<N>` is an alias that always
//!   carries the newest content (it can be a few minutes ahead of the highest
//!   `_v<N>`, which are immutable snapshots).

use chrono::NaiveDate;

/// The payload format of a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// IESO XML (`.xml`).
    Xml,
    /// Comma separated values (`.csv`).
    Csv,
}

impl Format {
    /// File extension without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Xml => "xml",
            Self::Csv => "csv",
        }
    }
}

/// How a report's files are stamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cadence {
    /// One file per delivery date (`YYYYMMDD`).
    Daily,
    /// One file per delivery hour (`YYYYMMDDHH`, hour-ending 01..=24).
    Hourly,
    /// One file per calendar year (`YYYY`).
    Yearly,
}

/// The reports this crate decodes into typed values.
///
/// Any other report on the site can still be fetched as text through
/// [`FileSpec::new`] and [`crate::Client::fetch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Report {
    /// Day-ahead hourly Ontario zonal price (`DAHourlyOntarioZonalPrice`).
    DaHourlyOntarioZonalPrice,
    /// Pre-dispatch hourly Ontario zonal price (`PredispHourlyOntarioZonalPrice`).
    PredispHourlyOntarioZonalPrice,
    /// Real-time five-minute Ontario zonal price (`RealtimeOntarioZonalPrice`).
    RealtimeOntarioZonalPrice,
    /// Real-time five-minute virtual zonal prices (`RealtimeZonalEnergyPrices`).
    RealtimeZonalEnergyPrices,
    /// Day-ahead hourly energy LMP by node (`DAHourlyEnergyLMP`).
    DaHourlyEnergyLmp,
    /// Pre-dispatch hourly energy LMP by node (`PredispHourlyEnergyLMP`).
    PredispHourlyEnergyLmp,
    /// Real-time five-minute energy LMP by node (`RealtimeEnergyLMP`).
    RealtimeEnergyLmp,
    /// Hourly market and Ontario demand (`Demand`).
    Demand,
    /// Per-generator output, capability and capacity (`GenOutputCapability`).
    GenOutputCapability,
    /// Hourly output by fuel type (`GenOutputbyFuelHourly`).
    GenOutputByFuelHourly,
    /// Intertie schedules and actual flows (`IntertieScheduleFlow`).
    IntertieScheduleFlow,
    /// Adequacy report (`Adequacy3`).
    Adequacy3,
}

impl Report {
    /// The directory (and file-name) stem on the IESO site.
    pub const fn name(self) -> &'static str {
        match self {
            Self::DaHourlyOntarioZonalPrice => "DAHourlyOntarioZonalPrice",
            Self::PredispHourlyOntarioZonalPrice => "PredispHourlyOntarioZonalPrice",
            Self::RealtimeOntarioZonalPrice => "RealtimeOntarioZonalPrice",
            Self::RealtimeZonalEnergyPrices => "RealtimeZonalEnergyPrices",
            Self::DaHourlyEnergyLmp => "DAHourlyEnergyLMP",
            Self::PredispHourlyEnergyLmp => "PredispHourlyEnergyLMP",
            Self::RealtimeEnergyLmp => "RealtimeEnergyLMP",
            Self::Demand => "Demand",
            Self::GenOutputCapability => "GenOutputCapability",
            Self::GenOutputByFuelHourly => "GenOutputbyFuelHourly",
            Self::IntertieScheduleFlow => "IntertieScheduleFlow",
            Self::Adequacy3 => "Adequacy3",
        }
    }

    /// Payload format of this report's files.
    pub const fn format(self) -> Format {
        match self {
            Self::DaHourlyEnergyLmp
            | Self::PredispHourlyEnergyLmp
            | Self::RealtimeEnergyLmp
            | Self::Demand => Format::Csv,
            _ => Format::Xml,
        }
    }

    /// How this report's files are stamped.
    pub const fn cadence(self) -> Cadence {
        match self {
            Self::RealtimeOntarioZonalPrice
            | Self::RealtimeZonalEnergyPrices
            | Self::RealtimeEnergyLmp => Cadence::Hourly,
            Self::Demand | Self::GenOutputByFuelHourly => Cadence::Yearly,
            _ => Cadence::Daily,
        }
    }
}

/// The time stamp part of a file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stamp {
    /// No stamp: the rolling file for the current period.
    Current,
    /// A delivery date (`YYYYMMDD`).
    Day(NaiveDate),
    /// A delivery date and hour-ending `1..=24` (`YYYYMMDDHH`).
    Hour(NaiveDate, u8),
    /// A calendar year (`YYYY`).
    Year(i32),
}

impl Stamp {
    /// The text that goes in the file name, or `None` for [`Stamp::Current`].
    pub fn render(self) -> Option<String> {
        match self {
            Self::Current => None,
            Self::Day(d) => Some(d.format("%Y%m%d").to_string()),
            Self::Hour(d, h) => Some(format!("{}{h:02}", d.format("%Y%m%d"))),
            Self::Year(y) => Some(format!("{y:04}")),
        }
    }
}

/// Which revision of a file to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Version {
    /// The un-suffixed alias: always the newest content. One request.
    #[default]
    Newest,
    /// The highest `_v<N>` found in the directory listing. Costs an extra
    /// request for the listing (several MB for the real-time reports).
    HighestListed,
    /// A specific `_v<N>` snapshot.
    Exact(u32),
}

/// Everything needed to name one file on the site.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileSpec {
    /// Report directory/stem, e.g. `DAHourlyOntarioZonalPrice`.
    pub report: String,
    /// File extension without the dot.
    pub extension: String,
    /// Period stamp.
    pub stamp: Stamp,
    /// Revision to read.
    pub version: Version,
}

impl FileSpec {
    /// A spec for an arbitrary report: current period, newest content.
    pub fn new(report: impl Into<String>, extension: impl Into<String>) -> Self {
        Self {
            report: report.into(),
            extension: extension.into(),
            stamp: Stamp::Current,
            version: Version::Newest,
        }
    }

    /// A spec for a known report.
    pub fn of(report: Report, stamp: Stamp) -> Self {
        Self {
            report: report.name().to_owned(),
            extension: report.format().extension().to_owned(),
            stamp,
            version: Version::Newest,
        }
    }

    /// Set the stamp.
    #[must_use]
    pub fn at(mut self, stamp: Stamp) -> Self {
        self.stamp = stamp;
        self
    }

    /// Set the revision policy.
    #[must_use]
    pub fn version(mut self, version: Version) -> Self {
        self.version = version;
        self
    }

    /// The file name for a given revision number (`None` = the alias).
    pub fn file_name(&self, revision: Option<u32>) -> String {
        let stamp = self
            .stamp
            .render()
            .map(|s| format!("_{s}"))
            .unwrap_or_default();
        let revision = revision.map(|n| format!("_v{n}")).unwrap_or_default();
        format!("PUB_{}{stamp}{revision}.{}", self.report, self.extension)
    }

    /// URL of the file under `base` (no trailing slash).
    pub fn url(&self, base: &str, revision: Option<u32>) -> String {
        format!("{base}/{}/{}", self.report, self.file_name(revision))
    }

    /// URL of the report's directory listing under `base`.
    pub fn directory_url(&self, base: &str) -> String {
        format!("{base}/{}/", self.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[test]
    fn stamps_render_in_the_site_format() {
        assert_eq!(Stamp::Current.render(), None);
        assert_eq!(
            Stamp::Day(d(2026, 11, 17)).render().as_deref(),
            Some("20261117")
        );
        assert_eq!(
            Stamp::Hour(d(2026, 11, 18), 2).render().as_deref(),
            Some("2026111802")
        );
        assert_eq!(Stamp::Year(2026).render().as_deref(), Some("2026"));
    }

    #[test]
    fn file_names_cover_alias_and_revisions() {
        let spec = FileSpec::of(
            Report::DaHourlyOntarioZonalPrice,
            Stamp::Day(d(2026, 11, 17)),
        );
        assert_eq!(
            spec.file_name(None),
            "PUB_DAHourlyOntarioZonalPrice_20261117.xml"
        );
        assert_eq!(
            spec.file_name(Some(1)),
            "PUB_DAHourlyOntarioZonalPrice_20261117_v1.xml"
        );
        assert_eq!(
            spec.url("https://reports-public.ieso.ca/public", None),
            "https://reports-public.ieso.ca/public/DAHourlyOntarioZonalPrice/PUB_DAHourlyOntarioZonalPrice_20261117.xml"
        );
    }

    #[test]
    fn current_file_has_no_stamp_and_csv_reports_use_csv() {
        let spec = FileSpec::of(Report::Demand, Stamp::Current);
        assert_eq!(spec.file_name(None), "PUB_Demand.csv");
        let rt = FileSpec::of(Report::RealtimeEnergyLmp, Stamp::Hour(d(2026, 11, 18), 24));
        assert_eq!(
            rt.file_name(Some(12)),
            "PUB_RealtimeEnergyLMP_2026111824_v12.csv"
        );
    }

    #[test]
    fn report_metadata_matches_the_live_site() {
        assert_eq!(Report::Demand.cadence(), Cadence::Yearly);
        assert_eq!(Report::RealtimeOntarioZonalPrice.cadence(), Cadence::Hourly);
        assert_eq!(Report::Adequacy3.cadence(), Cadence::Daily);
        assert_eq!(Report::DaHourlyEnergyLmp.format(), Format::Csv);
        assert_eq!(
            Report::GenOutputByFuelHourly.name(),
            "GenOutputbyFuelHourly"
        );
    }

    #[test]
    fn directory_url_ends_with_a_slash() {
        let spec = FileSpec::new("Demand", "csv");
        assert_eq!(
            spec.directory_url("https://x.test/public"),
            "https://x.test/public/Demand/"
        );
    }
}

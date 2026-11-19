//! Parsing of the Apache directory listings that index the reports site, and
//! selection of the latest `_vN` revision from a listing.

use chrono::NaiveDateTime;

/// One report directory in the site root listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDirectory {
    /// Directory name, e.g. `DAHourlyOntarioZonalPrice`.
    pub name: String,
    /// Last-modified time as shown by the server (Eastern, no zone).
    pub modified: Option<NaiveDateTime>,
    /// Human description, e.g. `Day-Ahead Hourly Ontario Zonal Price Report`.
    pub description: String,
}

/// One file in a report directory listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportFile {
    /// Full file name, e.g. `PUB_Demand_2026_v318.csv`.
    pub name: String,
    /// Report stem, e.g. `Demand`.
    pub report: String,
    /// The `YYYY`, `YYYYMMDD` or `YYYYMMDDHH` stamp, if the name has one.
    pub stamp: Option<String>,
    /// The `_vN` revision, if the name has one (the alias has none).
    pub version: Option<u32>,
    /// File extension without the dot.
    pub extension: String,
    /// Last-modified time as shown by the server (Eastern, no zone).
    pub modified: Option<NaiveDateTime>,
    /// Size column as shown by the server (`2.2K`, `3.4M`).
    pub size: String,
}

/// Split `PUB_<Report>[_<stamp>][_v<N>].<ext>` into its parts. Returns `None`
/// for names that do not follow the convention.
pub fn parse_file_name(name: &str) -> Option<(String, Option<String>, Option<u32>, String)> {
    let rest = name.strip_prefix("PUB_")?;
    let (stem, extension) = rest.rsplit_once('.')?;
    let mut parts = stem.split('_');
    let report = parts.next().filter(|r| !r.is_empty())?.to_owned();
    let mut stamp = None;
    let mut version = None;
    for part in parts {
        if let Some(n) = part.strip_prefix('v').and_then(|n| n.parse::<u32>().ok()) {
            version = Some(n);
        } else if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            stamp = Some(part.to_owned());
        } else {
            return None;
        }
    }
    Some((report, stamp, version, extension.to_owned()))
}

/// Parse the site root listing into report directories.
pub fn parse_directory_index(html: &str) -> Vec<ReportDirectory> {
    rows(html)
        .filter(|row| row.href.ends_with('/') && !row.href.starts_with('/'))
        .map(|row| ReportDirectory {
            name: row.href.trim_end_matches('/').to_owned(),
            modified: row.modified,
            description: row.description,
        })
        .collect()
}

/// Parse a report directory listing into files. Entries that do not follow
/// the `PUB_...` naming convention are skipped.
pub fn parse_file_index(html: &str) -> Vec<ReportFile> {
    rows(html)
        .filter_map(|row| {
            let (report, stamp, version, extension) = parse_file_name(&row.href)?;
            Some(ReportFile {
                name: row.href,
                report,
                stamp,
                version,
                extension,
                modified: row.modified,
                size: row.size,
            })
        })
        .collect()
}

/// The highest-numbered revision for `report`/`stamp`/`extension`, if any.
/// Files without a `_vN` part (the alias) are ignored.
pub fn latest_version<'a>(
    files: &'a [ReportFile],
    report: &str,
    stamp: Option<&str>,
    extension: &str,
) -> Option<&'a ReportFile> {
    files
        .iter()
        .filter(|f| f.report == report && f.extension == extension && f.stamp.as_deref() == stamp)
        .filter(|f| f.version.is_some())
        .max_by_key(|f| f.version)
}

struct Row {
    href: String,
    modified: Option<NaiveDateTime>,
    size: String,
    description: String,
}

/// Walk the `<a href="..">name</a>   <date> <time> <size> <description>` lines.
fn rows(html: &str) -> impl Iterator<Item = Row> + '_ {
    html.lines().filter_map(|line| {
        let after_open = line.split_once("<a href=\"")?.1;
        let (href, rest) = after_open.split_once('"')?;
        if href.starts_with('?') || href.is_empty() {
            return None;
        }
        let tail = rest.split_once("</a>").map_or("", |(_, t)| t);
        let mut fields = tail.split_whitespace();
        let date = fields.next().unwrap_or_default();
        let time = fields.next().unwrap_or_default();
        let size = fields.next().unwrap_or_default().to_owned();
        let description = fields.collect::<Vec<_>>().join(" ");
        let modified =
            NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%d-%b-%Y %H:%M").ok();
        Some(Row {
            href: href.to_owned(),
            modified,
            size,
            description,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = include_str!("../tests/fixtures/index_root.html");
    const RT: &str = include_str!("../tests/fixtures/index_rt_ontario_zonal.html");
    const DEMAND: &str = include_str!("../tests/fixtures/index_demand.html");

    #[test]
    fn file_names_split_into_report_stamp_version_extension() {
        assert_eq!(
            parse_file_name("PUB_RealtimeOntarioZonalPrice_2026111802_v3.xml"),
            Some((
                "RealtimeOntarioZonalPrice".into(),
                Some("2026111802".into()),
                Some(3),
                "xml".into()
            ))
        );
        assert_eq!(
            parse_file_name("PUB_Demand.csv"),
            Some(("Demand".into(), None, None, "csv".into()))
        );
        assert_eq!(
            parse_file_name("PUB_Demand_2026_v318.csv"),
            Some((
                "Demand".into(),
                Some("2026".into()),
                Some(318),
                "csv".into()
            ))
        );
        assert_eq!(parse_file_name("README.txt"), None);
        assert_eq!(parse_file_name("PUB_X_notastamp.xml"), None);
    }

    #[test]
    fn root_listing_yields_directories_with_descriptions() {
        let dirs = parse_directory_index(ROOT);
        let da = dirs
            .iter()
            .find(|d| d.name == "DAHourlyOntarioZonalPrice")
            .expect("DA zonal dir present");
        assert!(da
            .description
            .contains("Day-Ahead Hourly Ontario Zonal Price"));
        assert!(da.modified.is_some());
        assert!(dirs
            .iter()
            .all(|d| !d.name.contains('/') && d.name != "Parent Directory"));
    }

    #[test]
    fn file_listing_parses_alias_and_revisions() {
        let files = parse_file_index(RT);
        assert!(files
            .iter()
            .any(|f| f.version.is_none() && f.stamp.is_none()));
        let v3 = files
            .iter()
            .find(|f| f.name == "PUB_RealtimeOntarioZonalPrice_2026111802_v3.xml")
            .expect("v3 present");
        assert_eq!(v3.version, Some(3));
        assert_eq!(v3.size, "2.2K");
        assert_eq!(
            v3.modified.map(|m| m.to_string()).as_deref(),
            Some("2026-11-18 01:12:00")
        );
    }

    #[test]
    fn latest_version_picks_the_numerically_highest_revision() {
        let files = parse_file_index(RT);
        let best = latest_version(
            &files,
            "RealtimeOntarioZonalPrice",
            Some("2026111802"),
            "xml",
        )
        .expect("a revision exists");
        assert_eq!(best.version, Some(7));
        assert!(latest_version(
            &files,
            "RealtimeOntarioZonalPrice",
            Some("1999010101"),
            "xml"
        )
        .is_none());
    }

    #[test]
    fn revision_ten_outranks_revision_nine() {
        let mk = |n: u32| ReportFile {
            name: format!("PUB_X_20260101_v{n}.xml"),
            report: "X".into(),
            stamp: Some("20260101".into()),
            version: Some(n),
            extension: "xml".into(),
            modified: None,
            size: String::new(),
        };
        let files = vec![mk(9), mk(10), mk(2)];
        assert_eq!(
            latest_version(&files, "X", Some("20260101"), "xml").and_then(|f| f.version),
            Some(10),
            "numeric, not lexical, ordering"
        );
    }

    #[test]
    fn yearly_listing_separates_the_current_alias_from_revisions() {
        let files = parse_file_index(DEMAND);
        let best = latest_version(&files, "Demand", Some("2026"), "csv").expect("2026 revisions");
        assert!(best.version.unwrap_or(0) > 1);
        assert!(files
            .iter()
            .any(|f| f.name == "PUB_Demand.csv" && f.stamp.is_none()));
        assert!(
            latest_version(&files, "Demand", None, "csv").is_none(),
            "alias has no revision"
        );
    }
}

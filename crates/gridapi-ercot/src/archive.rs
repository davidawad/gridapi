//! Archive listing and download (zipped CSV/XML posting files).
//!
//! # Warning: ERCOT caps downloads of the same report at 3 per 12 months
//!
//! ERCOT's Data Access Portal terms say: "You may not download the same report from the
//! Data Access Portal more than three times within a 12-month period"
//! (<https://www.ercot.com/help/terms/data-portal>). The download helpers
//! (`Client::download_archive`, `Client::download_archives`) do not count or enforce this:
//! download each report once and keep the bytes. Listing (`Client::archives`) returns metadata only, not the report.

use std::io::Read;

use chrono::NaiveDateTime;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::page::{parse_datetime, PageMeta};
use crate::request::fmt_datetime;

/// Maximum nested-zip depth [`ArchiveFile::extract`] descends (bulk downloads nest one level).
const MAX_ZIP_DEPTH: usize = 3;

/// Upper bound on one extracted file, as a zip-bomb guard.
const MAX_EXTRACTED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// One posted document in a product's archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Document id, the handle for downloading.
    pub doc_id: u64,
    /// File label, e.g. `SPPHLZNP6905_20261117_2345_csv`.
    pub friendly_name: String,
    /// When ERCOT posted the document (Central time).
    pub post_datetime: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryWire {
    doc_id: u64,
    friendly_name: String,
    post_datetime: String,
}

/// One page of archive listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivePage {
    /// Paging block.
    pub meta: PageMeta,
    /// Documents on this page, newest first by default.
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Debug, Deserialize)]
struct PageWire {
    #[serde(rename = "_meta")]
    meta: PageMeta,
    #[serde(default)]
    archives: Vec<EntryWire>,
}

impl ArchivePage {
    /// Parses an archive listing response.
    ///
    /// # Errors
    ///
    /// [`Error::Decode`] when the body is not an archive listing.
    pub fn from_json(body: &[u8]) -> Result<Self> {
        let wire: PageWire =
            serde_json::from_slice(body).map_err(|e| Error::decode("archive listing", e))?;
        let entries = wire
            .archives
            .into_iter()
            .map(|e| {
                parse_datetime(&e.post_datetime)
                    .map(|post_datetime| ArchiveEntry {
                        doc_id: e.doc_id,
                        friendly_name: e.friendly_name,
                        post_datetime,
                    })
                    .map_err(|err| Error::decode("archive postDatetime", err))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            meta: wire.meta,
            entries,
        })
    }
}

/// Filters for an archive listing.
///
/// ```
/// use chrono::NaiveDate;
/// use gridapi_ercot::ArchiveQuery;
///
/// let day = NaiveDate::from_ymd_opt(2026, 11, 17).expect("valid date");
/// let q = ArchiveQuery::new().posted_between(
///     day.and_hms_opt(0, 0, 0).expect("valid time"),
///     day.and_hms_opt(23, 59, 59).expect("valid time"),
/// );
/// assert_eq!(q.params()[0].0, "postDatetimeFrom");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveQuery {
    from: Option<NaiveDateTime>,
    to: Option<NaiveDateTime>,
    page: Option<u32>,
    size: Option<u32>,
}

impl ArchiveQuery {
    /// An unfiltered listing (newest documents first).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Restricts to documents posted within `from..=to`.
    #[must_use]
    pub fn posted_between(mut self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.from = Some(from);
        self.to = Some(to);
        self
    }

    /// Requests 1-based page `page`.
    #[must_use]
    pub fn page(mut self, page: u32) -> Self {
        self.page = Some(page);
        self
    }

    /// Requests `size` listings per page.
    #[must_use]
    pub fn size(mut self, size: u32) -> Self {
        self.size = Some(size);
        self
    }

    /// The configured page size, if any.
    #[must_use]
    pub fn page_size(&self) -> Option<u32> {
        self.size
    }

    /// Query-string pairs for this listing.
    #[must_use]
    pub fn params(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        out.extend(
            self.from
                .map(|t| ("postDatetimeFrom".to_owned(), fmt_datetime(t))),
        );
        out.extend(
            self.to
                .map(|t| ("postDatetimeTo".to_owned(), fmt_datetime(t))),
        );
        out.extend(self.page.map(|p| ("page".to_owned(), p.to_string())));
        out.extend(self.size.map(|s| ("size".to_owned(), s.to_string())));
        out
    }
}

/// A downloaded archive: raw zip bytes as served by ERCOT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveFile {
    /// Name from the `Content-Disposition` header, if sent.
    pub file_name: Option<String>,
    /// The zip bytes. Bulk downloads are a zip of per-document zips.
    pub bytes: Vec<u8>,
}

/// One file unpacked from an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedFile {
    /// Path inside the (innermost) zip.
    pub name: String,
    /// File contents.
    pub bytes: Vec<u8>,
}

impl ExtractedFile {
    /// The contents as UTF-8 text (CSV and XML postings are UTF-8).
    ///
    /// # Errors
    ///
    /// [`Error::Archive`] when the bytes are not valid UTF-8.
    pub fn text(&self) -> Result<&str> {
        std::str::from_utf8(&self.bytes)
            .map_err(|e| Error::Archive(format!("{} is not UTF-8: {e}", self.name)))
    }

    /// True when the file name ends in `.csv` (case-insensitive).
    #[must_use]
    pub fn is_csv(&self) -> bool {
        self.name.to_ascii_lowercase().ends_with(".csv")
    }
}

impl ArchiveFile {
    /// Unpacks every file, descending into nested zips (bulk downloads nest one level).
    ///
    /// # Errors
    ///
    /// [`Error::Archive`] when the zip is corrupt, nests too deeply, or a file is oversized.
    pub fn extract(&self) -> Result<Vec<ExtractedFile>> {
        extract_zip(&self.bytes, 0)
    }
}

fn extract_zip(bytes: &[u8], depth: usize) -> Result<Vec<ExtractedFile>> {
    if depth >= MAX_ZIP_DEPTH {
        return Err(Error::Archive(format!(
            "zip nested deeper than {MAX_ZIP_DEPTH} levels"
        )));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Archive(format!("not a zip file: {e}")))?;
    let mut out = Vec::new();
    for idx in 0..zip.len() {
        let mut file = zip
            .by_index(idx)
            .map_err(|e| Error::Archive(format!("entry {idx}: {e}")))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|e| Error::Archive(format!("entry {idx} has an unreadable name: {e}")))?
            .into_owned();
        let mut buf = Vec::new();
        file.by_ref()
            .take(MAX_EXTRACTED_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| Error::Archive(format!("reading {name}: {e}")))?;
        if name.to_ascii_lowercase().ends_with(".zip") {
            out.extend(extract_zip(&buf, depth + 1)?);
        } else {
            out.push(ExtractedFile { name, bytes: buf });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_listing_with_millisecond_timestamps() {
        let page = ArchivePage::from_json(include_bytes!("../tests/fixtures/archive_listing.json"))
            .expect("fixture");
        assert_eq!(page.meta.total_records, 4);
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].doc_id, 1_300_006_200);
        assert_eq!(
            page.entries[0].friendly_name,
            "SPPHLZNP6905_20261117_2345_csv"
        );
        assert_eq!(
            page.entries[0].post_datetime.to_string(),
            "2026-11-17 23:47:01"
        );
    }

    #[test]
    fn query_params_use_api_names_in_order() {
        let t = |h| {
            chrono::NaiveDate::from_ymd_opt(2026, 11, 17)
                .expect("date")
                .and_hms_opt(h, 0, 0)
                .expect("time")
        };
        let names: Vec<_> = ArchiveQuery::new()
            .posted_between(t(1), t(2))
            .page(3)
            .size(10)
            .params()
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(
            names,
            ["postDatetimeFrom", "postDatetimeTo", "page", "size"]
        );
        assert!(ArchiveQuery::new().params().is_empty());
    }

    #[test]
    fn extracts_csv_from_a_real_posting_zip() {
        let file = ArchiveFile {
            file_name: None,
            bytes: include_bytes!("../tests/fixtures/posting.zip").to_vec(),
        };
        let files = file.extract().expect("valid zip");
        assert_eq!(files.len(), 1);
        assert!(files[0].is_csv(), "{}", files[0].name);
        let text = files[0].text().expect("utf8");
        assert!(
            text.starts_with("DeliveryDate,DeliveryHour,DeliveryInterval,SettlementPointName"),
            "{}",
            &text[..text.len().min(120)]
        );
    }

    #[test]
    fn extracts_nested_zips_from_a_bulk_download() {
        let file = ArchiveFile {
            file_name: None,
            bytes: include_bytes!("../tests/fixtures/bulk_nested.zip").to_vec(),
        };
        let files = file.extract().expect("valid nested zip");
        assert_eq!(files.len(), 2, "one csv per inner zip");
        assert!(files.iter().all(ExtractedFile::is_csv));
    }

    #[test]
    fn non_zip_bytes_are_an_archive_error() {
        let err = ArchiveFile {
            file_name: None,
            bytes: b"<html>bot check</html>".to_vec(),
        }
        .extract()
        .expect_err("not a zip");
        assert!(matches!(err, Error::Archive(_)), "{err}");
    }
}

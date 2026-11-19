//! Zip extraction and OASIS error-document detection.
//!
//! OASIS answers HTTP 200 even when the request failed: the zip then holds a
//! small XML file with an `ERROR` element instead of the data file. This module
//! turns that into [`Error::Oasis`].

use std::io::{Cursor, Read};

use crate::error::Error;
use crate::record::{parse_csv, parse_xml, Record};

/// Cap on total decompressed bytes per response, a guard against zip bombs.
/// The largest real response seen (the full node list) is about 3 MiB.
pub const MAX_UNZIPPED_BYTES: u64 = 512 * 1024 * 1024;

/// One file extracted from a response archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// The file name inside the archive.
    pub name: String,
    /// The decompressed contents.
    pub data: Vec<u8>,
}

/// Decompresses every file in a zip archive.
///
/// # Errors
///
/// [`Error::Zip`] when the archive is malformed, empty, or expands beyond
/// [`MAX_UNZIPPED_BYTES`].
pub fn unzip(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, Error> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| Error::Zip(e.to_string()))?;
    let mut remaining = MAX_UNZIPPED_BYTES;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|e| Error::Zip(e.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .map_err(|e| Error::Zip(e.to_string()))?
            .into_owned();
        let mut data = Vec::new();
        // Read one byte past the budget so an overrun is detectable.
        file.take(remaining.saturating_add(1))
            .read_to_end(&mut data)
            .map_err(|e| Error::Zip(format!("reading {name}: {e}")))?;
        remaining = remaining
            .checked_sub(data.len() as u64)
            .ok_or_else(|| Error::Zip(format!("archive exceeds {MAX_UNZIPPED_BYTES} bytes")))?;
        entries.push(ArchiveEntry { name, data });
    }
    if entries.is_empty() {
        return Err(Error::Zip("archive contains no files".into()));
    }
    Ok(entries)
}

/// Extracts `(code, description)` when `xml` is an OASIS error document.
///
/// Returns `None` for well-formed XML without an `ERROR` element, and for text
/// that is not XML at all.
#[must_use]
pub fn oasis_error(xml: &str) -> Option<(u32, String)> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let error = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "ERROR")?;
    let child_text = |name: &str| {
        error
            .children()
            .find(|c| c.is_element() && c.tag_name().name() == name)
            .and_then(|c| c.text())
            .map(|t| t.trim().to_owned())
    };
    let code = child_text("ERR_CODE").and_then(|c| c.parse::<u32>().ok())?;
    Some((code, child_text("ERR_DESC").unwrap_or_default()))
}

/// True when the first non-blank byte opens an XML document.
fn looks_like_xml(data: &[u8]) -> bool {
    let body = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

fn snippet(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(200)])
        .trim()
        .to_owned()
}

/// Decodes a `SingleZip` response body into records.
///
/// Each archive file is sniffed (XML starts with `<`, otherwise CSV), so the
/// result does not depend on which `resultformat` was requested. The first
/// OASIS error document found wins over any data. A body that is not a zip is
/// accepted only if it is an OASIS error XML.
///
/// # Errors
///
/// [`Error::Oasis`] for an error document, [`Error::UnexpectedBody`] for a
/// non-zip, non-XML body (for example an HTML proxy page), plus the zip, CSV
/// and XML parse errors.
pub fn decode_response(bytes: &[u8]) -> Result<Vec<Record>, Error> {
    let entries = if bytes.starts_with(b"PK") {
        unzip(bytes)?
    } else if looks_like_xml(bytes) {
        vec![ArchiveEntry {
            name: "response.xml".into(),
            data: bytes.to_vec(),
        }]
    } else {
        return Err(Error::UnexpectedBody(snippet(bytes)));
    };

    entries
        .iter()
        .map(|entry| decode_entry(&entry.data))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.into_iter().flatten().collect())
}

fn decode_entry(data: &[u8]) -> Result<Vec<Record>, Error> {
    if !looks_like_xml(data) {
        return parse_csv(data);
    }
    let text = std::str::from_utf8(data).map_err(|e| Error::Xml(e.to_string()))?;
    match oasis_error(text) {
        Some((code, description)) => Err(Error::Oasis { code, description }),
        // A genuine report always has REPORT_ITEMs; "no data" arrives as an ERROR
        // document. Well-formed XML with neither is a stray page, worth a retry.
        None => match parse_xml(text)? {
            rows if rows.is_empty() => Err(Error::UnexpectedBody(snippet(data))),
            rows => Ok(rows),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(files: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in files {
            writer.start_file(*name, zip::write::SimpleFileOptions::default())?;
            writer.write_all(data)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    #[test]
    fn csv_entry_decodes() -> Result<(), Box<dyn std::error::Error>> {
        let zipped = zip_of(&[("a.csv", b"X,Y\n1,2\n")])?;
        let rows = decode_response(&zipped)?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("y"), Some("2"));
        Ok(())
    }

    #[test]
    fn error_xml_inside_zip_is_oasis_error() -> Result<(), Box<dyn std::error::Error>> {
        let xml = br#"<?xml version="1.0"?><m:R xmlns:m="u"><m:ERROR><m:ERR_CODE>1004</m:ERR_CODE><m:ERR_DESC>Data can be requested for period of 31 days only</m:ERR_DESC></m:ERROR></m:R>"#;
        let zipped = zip_of(&[("e.xml", xml)])?;
        match decode_response(&zipped) {
            Err(Error::Oasis { code, description }) => {
                assert_eq!(code, 1004);
                assert!(description.contains("31 days"));
            }
            other => return Err(format!("expected Oasis error, got {other:?}").into()),
        }
        Ok(())
    }

    #[test]
    fn bare_error_xml_without_zip_is_detected() {
        let xml = b"<R><ERROR><ERR_CODE>1000</ERR_CODE><ERR_DESC>none</ERR_DESC></ERROR></R>";
        assert!(matches!(
            decode_response(xml),
            Err(Error::Oasis { code: 1000, .. })
        ));
    }

    #[test]
    fn html_body_is_unexpected() {
        let html =
            b"CAISO Acceptable Use Policy Violation. Please retry your request after 5 seconds.";
        assert!(matches!(
            decode_response(html),
            Err(Error::UnexpectedBody(_))
        ));
    }

    #[test]
    fn stray_xml_without_report_items_is_unexpected() {
        assert!(matches!(
            decode_response(b"<html>maintenance</html>"),
            Err(Error::UnexpectedBody(_))
        ));
    }

    #[test]
    fn corrupt_zip_is_zip_error() {
        assert!(matches!(
            decode_response(b"PK\x03\x04garbage"),
            Err(Error::Zip(_))
        ));
    }

    #[test]
    fn empty_archive_is_zip_error() -> Result<(), Box<dyn std::error::Error>> {
        let zipped = zip_of(&[])?;
        assert!(matches!(decode_response(&zipped), Err(Error::Zip(_))));
        Ok(())
    }

    #[test]
    fn multiple_entries_concatenate() -> Result<(), Box<dyn std::error::Error>> {
        let zipped = zip_of(&[("a.csv", b"X\n1\n"), ("b.csv", b"X\n2\n3\n")])?;
        assert_eq!(decode_response(&zipped)?.len(), 3);
        Ok(())
    }

    #[test]
    fn non_error_xml_is_parsed_as_report() -> Result<(), Box<dyn std::error::Error>> {
        let xml = b"<R><REPORT_ITEM><REPORT_DATA><VALUE>1</VALUE></REPORT_DATA></REPORT_ITEM></R>";
        let rows = decode_response(&zip_of(&[("r.xml", xml)])?)?;
        assert_eq!(rows[0].get("VALUE"), Some("1"));
        Ok(())
    }
}

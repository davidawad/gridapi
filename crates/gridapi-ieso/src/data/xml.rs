//! The envelope shared by every IESO XML report.

use chrono::NaiveDateTime;
use serde::{de::DeserializeOwned, Deserialize};

use crate::error::Result;

/// A decoded XML report: header metadata plus the typed body.
#[derive(Debug, Clone, PartialEq)]
pub struct Document<B> {
    /// `DocRevision` from the header.
    pub revision: Option<u32>,
    /// `CreatedAt` from the header (Eastern Standard Time, no zone in the file).
    pub created_at: Option<NaiveDateTime>,
    /// The report content.
    pub body: B,
}

#[derive(Deserialize)]
struct Envelope<B> {
    #[serde(rename = "DocHeader", alias = "IMODocHeader")]
    header: Header,
    #[serde(rename = "DocBody", alias = "IMODocBody")]
    body: B,
}

#[derive(Deserialize)]
struct Header {
    #[serde(rename = "DocRevision", default)]
    revision: Option<u32>,
    #[serde(rename = "CreatedAt", default)]
    created_at: Option<NaiveDateTime>,
}

/// Decode an IESO XML report whose body has shape `B`.
///
/// Both generations of envelope are accepted (`Document/DocHeader/DocBody` and
/// the older `IMODocument/IMODocHeader/IMODocBody`).
pub fn parse_document<B: DeserializeOwned>(xml: &str) -> Result<Document<B>> {
    let xml = xml.trim_start_matches('\u{feff}');
    let envelope: Envelope<B> = quick_xml::de::from_str(xml)?;
    Ok(Document {
        revision: envelope.header.revision,
        created_at: envelope.header.created_at,
        body: envelope.body,
    })
}

//! The portal's anonymous file browser: list folders, download files, find the newest file.
//!
//! ```text
//! GET /file-browser-api/?fsName=<source>&path=<folder>&type=folder      -> JSON array of entries
//! GET /file-browser-api/download/<source>?path=<file>                   -> the file
//! ```
//!
//! `<source>` is the page slug from the portal menu (`da-lmp-by-settlement-location`, ...);
//! [`Client::file_browser_pages`] lists them all. A wrong source answers `404` with an empty body.

use std::borrow::Cow;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::Client;

/// How deep [`Client::newest_file`] will descend before giving up.
const MAX_DESCENT: usize = 4;

/// How many of the most recent months [`Client::newest_file`] tries before giving up.
const RECENT_MONTHS: usize = 3;

/// Digits in a year folder name (`2026`).
const YEAR_WIDTH: usize = 4;

/// Digits in a month folder name (`11`).
const MONTH_WIDTH: usize = 2;

/// A file source (`fsName`): one folder tree on the portal, such as `rtbm-lmp-by-location`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileSource(Cow<'static, str>);

impl FileSource {
    /// A source known at compile time.
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// A source named at run time, e.g. one discovered via [`Client::file_browser_pages`].
    pub fn new(name: impl Into<String>) -> Self {
        Self(Cow::Owned(name.into()))
    }

    /// The source name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Day-ahead LMPs by settlement location (hourly, one file per operating day).
pub const DA_LMP_BY_LOCATION: FileSource = FileSource::from_static("da-lmp-by-settlement-location");
/// Real-time balancing market LMPs by settlement location (five-minute files).
pub const RTBM_LMP_BY_LOCATION: FileSource = FileSource::from_static("rtbm-lmp-by-location");
/// Hourly load by control zone.
pub const HOURLY_LOAD: FileSource = FileSource::from_static("hourly-load");
/// Generation mix by fuel, five-minute (last two hours) and historical.
pub const GENERATION_MIX: FileSource = FileSource::from_static("generation-mix-historical");

/// Whether an entry is a folder or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EntryKind {
    /// A folder.
    Folder,
    /// A downloadable file.
    File,
    /// Anything else the portal might add.
    #[serde(other)]
    Other,
}

/// One folder or file in a listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The source this entry belongs to.
    #[serde(rename = "fsName")]
    pub fs_name: String,
    /// The last path segment.
    pub name: String,
    /// The full path within the source, starting with `/`.
    pub path: String,
    /// Folder or file.
    #[serde(rename = "type")]
    pub kind: EntryKind,
    /// File size in bytes (files only).
    #[serde(default)]
    pub size: Option<u64>,
    /// When the file was posted, in Central Prevailing Time as the portal displays it.
    #[serde(default, deserialize_with = "modified_time")]
    pub modified: Option<NaiveDateTime>,
}

fn modified_time<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<NaiveDateTime>, D::Error> {
    let text = Option::<String>::deserialize(d)?;
    Ok(text.and_then(|s| NaiveDateTime::parse_from_str(s.trim(), "%m/%d/%Y %I:%M:%S %p").ok()))
}

/// A file-browser page from the portal menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPage {
    /// The source to pass to [`Client::list`] (the page slug).
    pub source: FileSource,
    /// The page title, e.g. `LMP By Settlement Location`.
    pub title: String,
    /// The menu path, e.g. `Public/Integrated Marketplace/Day-Ahead Market`.
    pub menu_path: String,
}

#[derive(Deserialize)]
struct MenuNode {
    name: String,
    slug: Option<String>,
    #[serde(rename = "pageType")]
    page_type: Option<String>,
    #[serde(rename = "childList", default)]
    children: Vec<MenuNode>,
}

fn collect_pages(nodes: &[MenuNode], prefix: &str, out: &mut Vec<BrowserPage>) {
    for node in nodes {
        if let (Some("file-browser"), Some(slug)) =
            (node.page_type.as_deref(), node.slug.as_deref())
        {
            out.push(BrowserPage {
                source: FileSource::new(slug),
                title: node.name.clone(),
                menu_path: prefix.to_owned(),
            });
        }
        let next = if prefix.is_empty() {
            node.name.clone()
        } else {
            format!("{prefix}/{}", node.name)
        };
        collect_pages(&node.children, &next, out);
    }
}

impl Client {
    /// List the folder `path` (use `/` for the root) of `source`.
    ///
    /// Folders sort before files; within each, names ascend.
    pub async fn list(&self, source: &FileSource, path: &str) -> Result<Vec<Entry>> {
        let url = self.url("file-browser-api/");
        let query = [
            ("fsName".to_owned(), source.as_str().to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("type".to_owned(), "folder".to_owned()),
        ];
        self.get_json(&url, &query).await
    }

    /// Download the file at `path` of `source`.
    pub async fn download(&self, source: &FileSource, path: &str) -> Result<Vec<u8>> {
        let url = self.url(&format!("file-browser-api/download/{}", source.as_str()));
        self.get_bytes(&url, &[("path".to_owned(), path.to_owned())])
            .await
    }

    /// Every file-browser page on the portal menu, with the [`FileSource`] to browse it.
    pub async fn file_browser_pages(&self) -> Result<Vec<BrowserPage>> {
        let nodes: Vec<MenuNode> = self.get_json(&self.url("api/menu/"), &[]).await?;
        let mut pages = Vec::new();
        collect_pages(&nodes, "", &mut pages);
        Ok(pages)
    }

    /// The file with the greatest name under `subfolder` of the newest `YYYY/MM` folder.
    ///
    /// Works for trees laid out `/<year>/<month>/<subfolder>[/<day>]/<file>`, such as
    /// `By_Day` and `By_Interval`. Posted file names sort chronologically, so the greatest
    /// name is the newest file. Falls back to earlier months when the newest is still empty.
    pub async fn newest_file(&self, source: &FileSource, subfolder: &str) -> Result<Entry> {
        let years = newest_names(&self.list(source, "/").await?, 1, YEAR_WIDTH);
        let Some(year) = years.first() else {
            return Err(empty(source, "/"));
        };
        let year_path = format!("/{year}");
        let months = newest_names(
            &self.list(source, &year_path).await?,
            RECENT_MONTHS,
            MONTH_WIDTH,
        );
        for month in &months {
            let root = format!("{year_path}/{month}/{subfolder}");
            match self.newest_below(source, &root).await {
                Ok(entry) => return Ok(entry),
                Err(e) if e.is_not_found() || matches!(e, Error::Empty { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Err(empty(source, &format!("{year_path}/*/{subfolder}")))
    }

    async fn newest_below(&self, source: &FileSource, root: &str) -> Result<Entry> {
        let mut path = root.to_owned();
        for _ in 0..MAX_DESCENT {
            let entries = self.list(source, &path).await?;
            if let Some(file) = entries
                .iter()
                .filter(|e| e.kind == EntryKind::File)
                .max_by(|a, b| a.name.cmp(&b.name))
            {
                return Ok(file.clone());
            }
            match entries
                .iter()
                .filter(|e| e.kind == EntryKind::Folder)
                .max_by(|a, b| a.name.cmp(&b.name))
            {
                Some(folder) => path.clone_from(&folder.path),
                None => break,
            }
        }
        Err(empty(source, root))
    }
}

fn empty(source: &FileSource, path: &str) -> Error {
    Error::Empty {
        fs_name: source.as_str().to_owned(),
        path: path.to_owned(),
    }
}

/// The `count` greatest all-digit folder names of exactly `width` digits, newest first
/// (zero-padded names sort chronologically).
///
/// Only digit names count: the portal root of `da-lmp-by-settlement-location` lists a
/// `RePrice` folder beside the years, and a plain string sort puts it after `2026`.
fn newest_names(entries: &[Entry], count: usize, width: usize) -> Vec<String> {
    let mut names: Vec<String> = entries
        .iter()
        .filter(|e| e.kind == EntryKind::Folder)
        .map(|e| e.name.clone())
        .filter(|n| n.len() == width && n.bytes().all(|b| b.is_ascii_digit()))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names.truncate(count);
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> Entry {
        Entry {
            fs_name: "s".into(),
            name: name.into(),
            path: format!("/{name}"),
            kind: EntryKind::Folder,
            size: None,
            modified: None,
        }
    }

    #[test]
    fn newest_names_are_descending_and_bounded() {
        let entries = [folder("2024"), folder("2026"), folder("2025")];
        assert_eq!(newest_names(&entries, 2, 4), vec!["2026", "2025"]);
    }

    /// Regression (): the live portal root lists `RePrice` after `2026`, and the
    /// newest-name sort picked it as the newest year, so `newest_file` failed with
    /// "nothing found under `/RePrice/*/By_Day`".
    #[test]
    fn newest_names_skip_folders_that_are_not_numeric_dates() {
        let entries = [
            folder("2025"),
            folder("2026"),
            folder("RePrice"),
            folder("202"),
            folder("20266"),
        ];
        assert_eq!(newest_names(&entries, 1, 4), vec!["2026"]);
        let months = [folder("09"), folder("11"), folder("archive"), folder("1")];
        assert_eq!(newest_names(&months, 3, 2), vec!["11", "09"]);
    }

    #[test]
    fn entry_decodes_the_portal_shape() {
        let json = r#"{"fsName":"rtbm-lmp-by-location","items":null,"modified":"11/17/2026 11:47:13 PM","name":"a.csv","path":"/2026/a.csv","size":178833,"type":"file"}"#;
        let e: Entry = serde_json::from_str(json).expect("entry");
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.size, Some(178_833));
        assert_eq!(
            e.modified.map(|m| m.to_string()).as_deref(),
            Some("2026-11-17 23:47:13")
        );
        let folder: Entry = serde_json::from_str(
            r#"{"fsName":"x","items":null,"modified":null,"name":"2013","path":"/2013","size":null,"type":"folder"}"#,
        )
        .expect("folder");
        assert_eq!(
            (folder.kind, folder.size, folder.modified),
            (EntryKind::Folder, None, None)
        );
    }

    #[test]
    fn unknown_entry_kinds_do_not_break_decoding() {
        let e: Entry =
            serde_json::from_str(r#"{"fsName":"x","name":"n","path":"/n","type":"symlink"}"#)
                .expect("entry");
        assert_eq!(e.kind, EntryKind::Other);
    }

    #[test]
    fn menu_walk_finds_only_file_browser_pages() {
        let menu: Vec<MenuNode> = serde_json::from_str(
            r#"[{"name":"Public","slug":"public","pageType":null,"childList":[
                {"name":"LMP","slug":"da-lmp","pageType":"file-browser","childList":[]},
                {"name":"Chart","slug":"c","pageType":"tableau","childList":[]}]}]"#,
        )
        .expect("menu");
        let mut pages = Vec::new();
        collect_pages(&menu, "", &mut pages);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].source.as_str(), "da-lmp");
        assert_eq!(pages[0].menu_path, "Public");
    }
}

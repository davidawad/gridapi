//! Keyless bulk files published on `www.eia.gov` (no API key needed):
//!
//! * [`eia860m`]: the monthly EIA-860M generator workbook, including the
//!   Planned, Retired and Canceled sheets the API does not serve.
//! * [`eia930`]: the six-month EIA-930 CSVs (balance, subregion,
//!   interchange), which carry the imputed and adjusted series and go back to
//!   mid-2015.
//!
//! Downloads stream to disk (the files are 14 MB to 100 MB) through the
//! [`Client`], so they share its retry policy and user agent. Parsing is
//! blocking and CPU-bound: call it from `tokio::task::spawn_blocking` inside
//! async code.

use std::path::Path;

use futures_util::StreamExt;
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;

use crate::client::Client;
use crate::error::{Error, Result};

pub mod eia860m;
pub mod eia930;

impl Client {
    /// Stream `url` into `dest`, returning the byte count. The file is
    /// written to `dest` plus a `.part` suffix and renamed only when
    /// complete, so a failed download never leaves a truncated file at
    /// `dest`. A redirect or 404 is [`Error::NotPublished`]. `magic`, when
    /// given, must prefix the body (xlsx is a zip, `PK`); it catches an HTML
    /// error page served with status 200.
    pub async fn download_to(&self, url: &str, dest: &Path, magic: Option<&[u8]>) -> Result<u64> {
        let resp = self.send_with_retry(url, &[], None).await?;
        let status = resp.status();
        if status.is_redirection() || status == StatusCode::NOT_FOUND {
            return Err(Error::NotPublished(url.to_owned()));
        }
        if !status.is_success() {
            return Err(Error::Api {
                status: status.as_u16(),
                code: None,
                message: format!("GET {url} failed"),
            });
        }
        let part = part_path(dest);
        let result = stream_body(resp, &part, magic, url).await;
        match result {
            Ok(n) => {
                tokio::fs::rename(&part, dest).await?;
                Ok(n)
            }
            Err(e) => {
                // Best effort: the partial file is garbage either way.
                let _ = tokio::fs::remove_file(&part).await;
                Err(e)
            }
        }
    }
}

fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    name.into()
}

async fn stream_body(
    resp: reqwest::Response,
    part: &Path,
    magic: Option<&[u8]>,
    url: &str,
) -> Result<u64> {
    let mut file = tokio::fs::File::create(part).await?;
    let mut body = resp.bytes_stream();
    let mut total = 0u64;
    let mut checked = magic.is_none();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(Error::from_reqwest)?;
        if !checked {
            checked = true;
            if magic.is_some_and(|m| !chunk.starts_with(m)) {
                return Err(Error::NotPublished(format!(
                    "{url} (reply is not the expected file type)"
                )));
            }
        }
        file.write_all(&chunk).await?;
        total += chunk.len() as u64;
    }
    file.flush().await?;
    if total == 0 {
        return Err(Error::NotPublished(format!("{url} (empty body)")));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(
            part_path(Path::new("/tmp/a/b.xlsx")),
            Path::new("/tmp/a/b.xlsx.part")
        );
    }
}

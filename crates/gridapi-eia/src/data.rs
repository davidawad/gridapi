//! Data queries: one page, a lazy stream of pages, or everything.
//!
//! EIA caps a JSON reply at 5000 rows. [`Client::data_stream`] and
//! [`Client::data_all`] walk `offset` until the reported `total` is reached,
//! so a query that matches 140 000 rows is one call here.

use futures_util::stream::{self, BoxStream, StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::client::{Client, Warning};
use crate::de::u64_lenient;
use crate::error::Result;
use crate::query::DataQuery;

/// An untyped data row: column name to JSON value. Use it for routes this
/// crate has no typed row for.
pub type Row = Map<String, Value>;

/// One page of a data reply.
#[derive(Debug, Clone, PartialEq)]
pub struct DataPage<R> {
    /// Rows matching the whole query (not just this page).
    pub total: u64,
    /// Format of `period` in the rows, for example `YYYY-MM`.
    pub date_format: Option<String>,
    /// Frequency the rows are aggregated at.
    pub frequency: Option<String>,
    /// Dataset description.
    pub description: Option<String>,
    /// Rows in this page.
    pub rows: Vec<R>,
    /// Warnings EIA attached (the 5000-row notice appears on every reply to
    /// a query with more than 5000 matches, even when `length` is small).
    pub warnings: Vec<Warning>,
}

#[derive(Deserialize)]
struct RawPage<R> {
    #[serde(deserialize_with = "u64_lenient")]
    total: u64,
    #[serde(default, rename = "dateFormat")]
    date_format: Option<String>,
    #[serde(default)]
    frequency: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "Vec::new")]
    data: Vec<R>,
}

impl Client {
    /// Fetch exactly the page `query` describes (its own offset and length;
    /// EIA defaults apply when unset, which is the first 5000 rows).
    pub async fn data_page<R: DeserializeOwned>(&self, query: &DataQuery) -> Result<DataPage<R>> {
        query.validate()?;
        let path = format!("{}/data", query.route());
        let (raw, warnings) = self.get_api::<RawPage<R>>(&path, &query.params()).await?;
        Ok(DataPage {
            total: raw.total,
            date_format: raw.date_format,
            frequency: raw.frequency,
            description: raw.description,
            rows: raw.data,
            warnings,
        })
    }

    /// Lazily page through every row the query matches, yielding one
    /// `Vec<R>` per request. Starts at the query's `offset`, uses its
    /// `length` (default 5000) as page size, and stops when `total` is
    /// reached or a page comes back empty. Without an explicit sort it sorts
    /// by `period` ascending so offsets address a stable order (see
    /// [`DataQuery::without_auto_sort`]). Drop the stream to stop early.
    pub fn data_stream<R>(&self, query: &DataQuery) -> BoxStream<'static, Result<Vec<R>>>
    where
        R: DeserializeOwned + Send + 'static,
    {
        let client = self.clone();
        let query = query.clone();
        let first = Some(query.start_offset());
        stream::try_unfold(first, move |next| {
            let client = client.clone();
            let query = query.clone();
            async move {
                let Some(offset) = next else { return Ok(None) };
                let page = client.data_page::<R>(&query.page_at(offset)).await?;
                let following = next_offset(offset, page.rows.len(), page.total);
                Ok((!page.rows.is_empty()).then_some((page.rows, following)))
            }
        })
        .boxed()
    }

    /// Every row the query matches, concatenated. See [`Client::data_stream`]
    /// for paging rules. Memory grows with the result; stream huge ones.
    pub async fn data_all<R>(&self, query: &DataQuery) -> Result<Vec<R>>
    where
        R: DeserializeOwned + Send + 'static,
    {
        self.data_stream::<R>(query)
            .try_fold(Vec::new(), |mut acc, mut page| async move {
                acc.append(&mut page);
                Ok(acc)
            })
            .await
    }
}

/// Offset of the next page, or `None` when `offset + got` has reached
/// `total` or the page was empty. Pure.
fn next_offset(offset: u64, got: usize, total: u64) -> Option<u64> {
    let next = offset.saturating_add(got as u64);
    (got > 0 && next < total).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_advances_until_total() {
        assert_eq!(next_offset(0, 5000, 12_000), Some(5000));
        assert_eq!(next_offset(5000, 5000, 12_000), Some(10_000));
        assert_eq!(next_offset(10_000, 2000, 12_000), None);
        assert_eq!(next_offset(0, 5000, 5000), None);
        assert_eq!(next_offset(0, 0, 10), None, "an empty page ends paging");
        assert_eq!(next_offset(0, 3, 0), None);
    }
}

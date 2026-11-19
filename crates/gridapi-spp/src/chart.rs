//! The portal's chart feeds (`/chart-api/<chart>/asChart`): the data behind its public dashboard charts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::Client;

/// A dashboard chart with a public data feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Chart {
    /// SPP East generation mix, last two hours.
    GenMix,
    /// SPP West (SWPW) generation mix.
    ///
    /// **Unverified, behind the `unverified` feature**: `chart-api/gen-mix-swpw/asChart`
    /// answers HTTP 404 on the live portal (2026-11-18); it serves this chart only in the
    /// echarts shape at `/asChart/line`. Use [`Client::generation_mix`] with
    /// [`BalancingArea::Swpw`](crate::BalancingArea::Swpw) for the same data.
    #[cfg(feature = "unverified")]
    GenMixSwpw,
    /// SPP East load, wind and solar: forecast against actual.
    LoadForecast,
    /// SPP West load, wind and solar: forecast against actual.
    LoadForecastSwpw,
    /// SPP East area control error.
    Ace,
    /// SPP West area control error.
    AceSwpw,
}

impl Chart {
    /// The feed name used in the URL.
    pub fn slug(self) -> &'static str {
        match self {
            Self::GenMix => "gen-mix",
            #[cfg(feature = "unverified")]
            Self::GenMixSwpw => "gen-mix-swpw",
            Self::LoadForecast => "load-forecast",
            Self::LoadForecastSwpw => "load-forecast-swpw",
            Self::Ace => "ace",
            Self::AceSwpw => "ace-swpw",
        }
    }
}

/// One line of a chart: a label and a value per timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// The series name, e.g. `Actual Load` or `Coal`.
    pub label: String,
    /// One value per entry of [`ChartData::times`]; `None` where the series has no point.
    pub values: Vec<Option<f64>>,
}

/// A chart feed: shared timestamps and the series plotted against them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartData {
    /// The x axis, in UTC.
    pub times: Vec<DateTime<Utc>>,
    /// The plotted series.
    pub series: Vec<Series>,
}

impl ChartData {
    /// The series called `label`.
    pub fn series_named(&self, label: &str) -> Option<&Series> {
        self.series.iter().find(|s| s.label == label)
    }

    /// The last timestamp with a value in the series called `label`, with that value.
    pub fn latest(&self, label: &str) -> Option<(DateTime<Utc>, f64)> {
        let series = self.series_named(label)?;
        self.times
            .iter()
            .zip(&series.values)
            .rev()
            .find_map(|(t, v)| v.map(|v| (*t, v)))
    }
}

#[derive(Deserialize)]
struct ChartWire {
    response: ResponseWire,
}
#[derive(Deserialize)]
struct ResponseWire {
    #[serde(default)]
    labels: Vec<DateTime<Utc>>,
    #[serde(default)]
    datasets: Vec<DatasetWire>,
}
#[derive(Deserialize)]
struct DatasetWire {
    label: String,
    #[serde(default)]
    data: Vec<Option<f64>>,
}

/// Decode a chart feed body.
pub fn parse_chart(bytes: &[u8], what: &str) -> Result<ChartData> {
    let wire: ChartWire = serde_json::from_slice(bytes).map_err(|e| Error::decode(what, e))?;
    let times = wire.response.labels;
    if let Some(bad) = wire
        .response
        .datasets
        .iter()
        .find(|d| d.data.len() != times.len())
    {
        return Err(Error::decode(
            what,
            format!(
                "series {:?} has {} points for {} timestamps",
                bad.label,
                bad.data.len(),
                times.len()
            ),
        ));
    }
    Ok(ChartData {
        times,
        series: wire
            .response
            .datasets
            .into_iter()
            .map(|d| Series {
                label: d.label,
                values: d.data,
            })
            .collect(),
    })
}

impl Client {
    /// Fetch a dashboard chart feed.
    pub async fn chart(&self, chart: Chart) -> Result<ChartData> {
        let url = self.url(&format!("chart-api/{}/asChart", chart.slug()));
        parse_chart(&self.get_bytes(&url, &[]).await?, &url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"pieLabels":null,"response":{"labels":["2026-11-17T06:40:00Z","2026-11-17T06:45:00Z"],"datasets":[{"label":"Actual Solar","data":[null,-0.093],"fill":false},{"label":"Actual Load","data":[21120.910,29080.16]}],"lastModifiedDate":1}}"#;

    #[test]
    fn decodes_labels_and_nullable_points() {
        let c = parse_chart(BODY.as_bytes(), "t").expect("chart");
        assert_eq!(c.times.len(), 2);
        assert_eq!(
            c.series_named("Actual Solar").map(|s| s.values.clone()),
            Some(vec![None, Some(-0.093)])
        );
        assert_eq!(c.latest("Actual Load").map(|(_, v)| v), Some(29080.16));
        assert_eq!(c.latest("nope"), None);
    }

    #[test]
    fn mismatched_series_lengths_are_rejected() {
        let bad = r#"{"response":{"labels":["2026-11-17T06:40:00Z"],"datasets":[{"label":"x","data":[1,2]}]}}"#;
        assert!(parse_chart(bad.as_bytes(), "t").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(Chart::GenMix.slug(), "gen-mix");
        assert_eq!(Chart::AceSwpw.slug(), "ace-swpw");
    }

    #[cfg(feature = "unverified")]
    #[test]
    fn unverified_slugs() {
        assert_eq!(Chart::GenMixSwpw.slug(), "gen-mix-swpw");
    }
}

//! Renders an SVG chart of one day of day-ahead zonal prices decoded by this crate.
//!
//! The prices are synthetic (a deterministic daily curve plus per-zone congestion), written in
//! NYISO's real file layout and served from a local mock server, so the render needs no network:
//! `cargo run -p gridapi-nyiso --example showcase_chart > docs/showcase/nyiso-zones.svg`
//! (`docs/showcase/make-screenshots.sh` does this and converts it to PNG).

#![allow(clippy::many_single_char_names)] // chart geometry: w, h, x, y

use std::collections::BTreeMap;
use std::fmt::Write as _;

use gridapi_nyiso::{chrono::NaiveDate, Client, Level, Source};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

const ZONES: [&str; 11] = [
    "WEST", "GENESE", "CENTRL", "NORTH", "MHK VL", "CAPITL", "HUD VL", "MILLWD", "DUNWOD",
    "N.Y.C.", "LONGIL",
];
const COLORS: [&str; 11] = [
    "#4e79a7", "#59a14f", "#9c755f", "#76b7b2", "#edc948", "#b07aa1", "#bab0ac", "#ff9da7",
    "#f28e2b", "#e15759", "#8cd17d",
];

fn bump(h: f64, centre: f64, width: f64) -> f64 {
    (-((h - centre) / width).powi(2)).exp()
}

/// One synthetic NYISO `damlbmp_zone` file for a day: a morning and an evening peak, with the
/// downstate zones congested in the evening and the upstate zones relieved.
fn synthetic_file() -> String {
    let mut out = String::from(
        "Time Stamp,Name,PTID,LBMP ($/MWHr),Marginal Cost Losses ($/MWHr),Marginal Cost Congestion ($/MWHr)\n",
    );
    for hour in 0..24_u32 {
        let h = f64::from(hour);
        let base = 24.0 + 20.0 * bump(h, 8.0, 2.5) + 34.0 * bump(h, 18.5, 2.8);
        for (i, zone) in ZONES.iter().enumerate() {
            let k = f64::from(u32::try_from(i).unwrap_or(0)) - 5.0;
            let congestion = k * f64::from(3_u8) * bump(h, 19.0, 4.5) + k * bump(h, 8.0, 2.5);
            let losses = 0.9 + 0.12 * k + 0.5 * bump(h, 18.5, 3.0);
            let lbmp = base + congestion + losses;
            let _ = writeln!(
                out,
                "11/17/2026 {hour:02}:00,{zone},{},{lbmp:.2},{losses:.2},{congestion:.2}",
                61_700 + i
            );
        }
    }
    out
}

fn svg(series: &BTreeMap<String, Vec<f64>>) -> String {
    let (w, h, left, right, top, bottom) = (1200.0, 660.0, 84.0, 150.0, 118.0, 84.0);
    let (pw, ph) = (w - left - right, h - top - bottom);
    let (lo, hi) = (10.0, 100.0);
    let x = |hour: f64| left + pw * hour / 23.0;
    let y = |v: f64| top + ph * (1.0 - (v - lo) / (hi - lo));
    let mut s = String::new();
    let _ = write!(
        s,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="Helvetica, Arial, sans-serif">
<rect width="{w}" height="{h}" fill="#ffffff"/>
<text x="{left}" y="50" font-size="30" font-weight="700" fill="#1b1f24">NYISO day-ahead LBMP by zone</text>
<text x="{left}" y="82" font-size="18" fill="#57606a">2026-11-17, $/MWh. Decoded by gridapi-nyiso from a synthetic file in NYISO's layout (invented prices).</text>
"##
    );
    for tick in (10..=100).step_by(15) {
        let yy = y(f64::from(tick));
        let _ = writeln!(
            s,
            r##"<line x1="{left}" x2="{}" y1="{yy}" y2="{yy}" stroke="#e5e7eb"/><text x="{}" y="{}" font-size="15" text-anchor="end" fill="#57606a">{tick}</text>"##,
            left + pw,
            left - 10.0,
            yy + 5.0,
        );
    }
    for hour in (0..24).step_by(3) {
        let _ = writeln!(
            s,
            r##"<text x="{}" y="{}" font-size="15" text-anchor="middle" fill="#57606a">{hour:02}:00</text>"##,
            x(f64::from(hour)),
            top + ph + 28.0
        );
    }
    let _ = writeln!(
        s,
        r##"<text x="{}" y="{}" font-size="15" text-anchor="middle" fill="#57606a">hour beginning, Eastern time</text>"##,
        left + pw / 2.0,
        top + ph + 54.0
    );
    for (i, zone) in ZONES.iter().enumerate() {
        let Some(vals) = series.get(*zone) else {
            continue;
        };
        let pts: Vec<String> = vals
            .iter()
            .enumerate()
            .map(|(hr, v)| {
                format!(
                    "{:.1},{:.1}",
                    x(f64::from(u32::try_from(hr).unwrap_or(0))),
                    y(*v)
                )
            })
            .collect();
        let width = if *zone == "N.Y.C." || *zone == "WEST" {
            3.2
        } else {
            2.0
        };
        let _ = writeln!(
            s,
            r#"<polyline fill="none" stroke="{}" stroke-width="{width}" stroke-linejoin="round" points="{}"/>"#,
            COLORS[i],
            pts.join(" ")
        );
    }
    let mut ends: Vec<(usize, f64)> = ZONES
        .iter()
        .enumerate()
        .filter_map(|(i, z)| series.get(*z).and_then(|v| v.last()).map(|l| (i, y(*l))))
        .collect();
    ends.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut prev = f64::MIN;
    for (i, ey) in ends {
        let ly = if ey - prev < 18.0 { prev + 18.0 } else { ey };
        prev = ly;
        let _ = writeln!(
            s,
            r#"<line x1="{}" x2="{}" y1="{ey:.1}" y2="{ly:.1}" stroke="{c}"/><text x="{}" y="{}" font-size="14" font-weight="700" fill="{c}">{}</text>"#,
            left + pw,
            left + pw + 8.0,
            left + pw + 12.0,
            ly + 5.0,
            ZONES[i],
            c = COLORS[i],
        );
    }
    s.push_str("</svg>\n");
    s
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/damlbmp/20261117damlbmp_zone.csv"))
        .respond_with(ResponseTemplate::new(200).set_body_string(synthetic_file()))
        .mount(&server)
        .await;
    let client = Client::builder().base_url(server.uri()).build()?;
    let day = NaiveDate::from_ymd_opt(2026, 11, 17).ok_or("bad date")?;
    let rows = client
        .day_ahead_lbmp(Level::Zone, day)
        .source(Source::DailyOnly)
        .fetch_strict()
        .await?;
    let mut series: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for r in &rows {
        series.entry(r.name.clone()).or_default().push(r.lbmp);
    }
    print!("{}", svg(&series));
    Ok(())
}

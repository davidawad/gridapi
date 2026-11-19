//! State-estimator total ERCOT generation.

use chrono::NaiveDateTime;

use crate::error::Result;
use crate::page::{FromRow, Row};
use crate::request::{Query, Report};

/// NP6-625-CD: state-estimator load report, total ERCOT generation.
#[derive(Debug, Clone, Copy)]
pub struct SystemGeneration;

/// One state-estimator run's total generation.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemGenerationRecord {
    /// State-estimator execution time (Central time).
    pub se_exe_time: NaiveDateTime,
    /// Clock-change marker published with the timestamp (`s` standard, `d` daylight).
    pub se_exe_time_dst: String,
    /// State-estimator generation, MW.
    pub se_mw: f64,
    /// State-estimator reactive power, MVAR.
    pub se_mvar: f64,
    /// SCADA generation, MW.
    pub scada_mw: f64,
    /// SCADA reactive power, MVAR (published as text; parsed when numeric).
    pub scada_mvar: Option<f64>,
}

impl FromRow for SystemGenerationRecord {
    fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            se_exe_time: row.datetime("seExeTime")?,
            se_exe_time_dst: row.str("seExeTimeDST")?.to_owned(),
            se_mw: row.f64("seMW")?,
            se_mvar: row.f64("seMVAR")?,
            scada_mw: row.f64("scadaMW")?,
            scada_mvar: row.opt_f64("scadaMVAR")?,
        })
    }
}

impl Report for SystemGeneration {
    type Record = SystemGenerationRecord;
    const EMIL_ID: &'static str = "NP6-625-CD";
    const ARTIFACT: &'static str = "se_ld_rpt_ercot_gen";
}

impl Query<SystemGeneration> {
    /// Restricts to state-estimator runs within `from..=to`.
    #[must_use]
    pub fn se_exe_time(self, from: NaiveDateTime, to: NaiveDateTime) -> Self {
        self.datetime_range("seExeTime", from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::ReportPage;

    #[test]
    fn generation_fixture_parses_text_mvar() {
        let recs = ReportPage::from_json(include_bytes!("../../tests/fixtures/ercot_gen.json"))
            .expect("page")
            .records::<SystemGenerationRecord>()
            .expect("records");
        assert_eq!(recs.len(), 2);
        assert!((recs[0].se_mw - 43_216.325_069).abs() < 1e-6);
        assert_eq!(recs[0].scada_mvar, Some(6_886.052_424));
        assert_eq!(recs[0].se_exe_time_dst, "s");
    }
}

//! Pricing node and aggregate pricing node list rows (`ATL_PNODE`, `ATL_APNODE`).

use chrono::{DateTime, Utc};

use crate::error::Error;
use crate::record::{FromRecord, Record};

/// One pricing node (`PNode`) or aggregate pricing node (`APNode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    /// The node id as used in the `node` query parameter.
    pub id: String,
    /// The node type code (`AG`, `CPZ`, ...), when CAISO classifies it.
    pub node_type: Option<String>,
    /// Free-text description.
    pub description: Option<String>,
    /// When the node definition took effect, UTC.
    pub effective_start: Option<DateTime<Utc>>,
    /// When the node definition ends, UTC.
    pub effective_end: Option<DateTime<Utc>>,
}

impl FromRecord for NodeRow {
    fn from_record(r: &Record) -> Result<Self, Error> {
        Ok(Self {
            id: r.require(&["PNODE_ID", "APNODE_ID", "NODE_ID"])?.to_owned(),
            node_type: r.first(&["PNODE_TYPE", "APNODE_TYPE"]).map(str::to_owned),
            description: r.first(&["DESCRIPTION"]).map(str::to_owned),
            effective_start: r.optional_timestamp(&["EFF_START_DT_GMT"])?,
            effective_end: r.optional_timestamp(&["EFF_END_DT_GMT"])?,
        })
    }
}

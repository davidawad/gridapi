//! Reference data: pool assets and pool participants.

use serde::Deserialize;

use crate::serde_util::opt_text;

/// One pool asset.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Asset {
    /// Descriptive name, for example `AST1 Example`.
    pub asset_name: String,
    /// Short id, for example `AST1`.
    #[serde(rename = "asset_ID")]
    pub asset_id: String,
    /// `SOURCE` or `SINK`.
    pub asset_type: String,
    /// `Active`, `Retired`, ...
    pub operating_status: String,
    /// Owning participant's name.
    pub pool_participant_name: String,
    /// Owning participant's id.
    #[serde(rename = "pool_participant_ID")]
    pub pool_participant_id: String,
    /// Net-to-grid flag, when set.
    #[serde(default, deserialize_with = "opt_text")]
    pub net_to_grid_asset_flag: Option<String>,
    /// Storage flag, when set.
    #[serde(default, deserialize_with = "opt_text")]
    pub asset_incl_storage_flag: Option<String>,
}

/// An agent acting for a pool participant.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Agent {
    /// Agent id.
    #[serde(rename = "agent_ID")]
    pub agent_id: String,
    /// Agent name.
    pub agent_name: String,
}

/// A registered pool participant.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PoolParticipant {
    /// Legal name.
    pub pool_participant_name: String,
    /// Short id, for example `PPI1`.
    #[serde(rename = "pool_participant_ID")]
    pub pool_participant_id: String,
    /// Corporate contact block (free text, multi-line).
    #[serde(default, deserialize_with = "opt_text")]
    pub corporate_contact: Option<String>,
    /// Agents acting for the participant.
    #[serde(default)]
    pub agent_list: Vec<Agent>,
}

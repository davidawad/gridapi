//! The three CAISO trading hubs and their OASIS node ids.

use std::fmt;
use std::str::FromStr;

use crate::error::Error;

/// A CAISO trading hub (a load-weighted average of generator nodes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TradingHub {
    /// NP15, northern California.
    Np15,
    /// SP15, southern California.
    Sp15,
    /// ZP26, the central-valley zone.
    Zp26,
}

impl TradingHub {
    /// All three hubs.
    pub const ALL: [Self; 3] = [Self::Np15, Self::Sp15, Self::Zp26];

    /// The OASIS `node` value for the hub's generation aggregate.
    #[must_use]
    pub fn node_id(self) -> &'static str {
        match self {
            Self::Np15 => "TH_NP15_GEN-APND",
            Self::Sp15 => "TH_SP15_GEN-APND",
            Self::Zp26 => "TH_ZP26_GEN-APND",
        }
    }

    /// The node ids of every hub, ready for [`Client::day_ahead_lmp`](crate::Client::day_ahead_lmp).
    #[must_use]
    pub fn all_node_ids() -> [&'static str; 3] {
        Self::ALL.map(Self::node_id)
    }
}

impl fmt::Display for TradingHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.node_id())
    }
}

impl FromStr for TradingHub {
    type Err = Error;

    /// Accepts `NP15`, `TH_NP15`, or the full node id, case-insensitively.
    fn from_str(s: &str) -> Result<Self, Error> {
        let key = s.trim().to_ascii_uppercase();
        Self::ALL
            .into_iter()
            .find(|hub| {
                let full = hub.node_id();
                key == full
                    || key == full.trim_end_matches("_GEN-APND")
                    || key == full.trim_start_matches("TH_").trim_end_matches("_GEN-APND")
            })
            .ok_or_else(|| Error::InvalidQuery(format!("unknown trading hub {s:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_match_oasis() {
        assert_eq!(TradingHub::Np15.node_id(), "TH_NP15_GEN-APND");
        assert_eq!(TradingHub::Sp15.node_id(), "TH_SP15_GEN-APND");
        assert_eq!(TradingHub::Zp26.node_id(), "TH_ZP26_GEN-APND");
    }

    #[test]
    fn parses_short_and_full_names() -> Result<(), Error> {
        for hub in TradingHub::ALL {
            assert_eq!(hub.node_id().parse::<TradingHub>()?, hub);
            assert_eq!(hub.to_string().to_lowercase().parse::<TradingHub>()?, hub);
        }
        assert_eq!("sp15".parse::<TradingHub>()?, TradingHub::Sp15);
        assert_eq!("TH_ZP26".parse::<TradingHub>()?, TradingHub::Zp26);
        assert!("palo_verde".parse::<TradingHub>().is_err());
        Ok(())
    }
}

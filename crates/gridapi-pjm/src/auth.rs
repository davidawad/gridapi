//! Subscription-key handling: the key you provisioned with PJM. The public
//! key the Data Miner 2 web app ships lives in `public_key.rs`, behind the
//! off-by-default `pjm-public-web-key` feature.

use std::fmt;

use crate::error::Error;

/// Environment variable read by [`KeyMode::from_env`].
pub const API_KEY_ENV: &str = "PJM_API_KEY";

/// Where to register for your own subscription key.
pub const API_PORTAL_URL: &str = "https://apiportal.pjm.com/";

/// A PJM API-management subscription key. `Debug` never prints the value.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a key, trimming whitespace.
    ///
    /// # Errors
    /// [`Error::InvalidKey`] when the key is empty or not printable ASCII (it
    /// is sent as an HTTP header value).
    pub fn new(key: impl AsRef<str>) -> Result<Self, Error> {
        let key = key.as_ref().trim();
        if key.is_empty() {
            return Err(Error::InvalidKey("key is empty"));
        }
        if !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::InvalidKey("key must be printable ASCII"));
        }
        Ok(Self(key.to_owned()))
    }

    /// The raw key, for building the request header.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Which key the client sends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum KeyMode {
    /// `PJM_API_KEY` when set and non-empty; otherwise building the client
    /// fails with [`Error::MissingKey`]. It never falls back to the public
    /// web-app key.
    #[default]
    Auto,
    /// A key you provisioned at <https://apiportal.pjm.com/>.
    User(ApiKey),
    /// The key the Data Miner 2 web app ships, fetched at runtime from
    /// `settings.json` and refetched when PJM answers 401 (it rotates).
    ///
    /// Only compiled with the off-by-default `pjm-public-web-key` cargo
    /// feature, and [`ClientBuilder::build`](crate::ClientBuilder::build)
    /// refuses it unless the builder was also given
    /// [`i_accept_pjm_public_key_risk`](crate::ClientBuilder::i_accept_pjm_public_key_risk).
    ///
    /// # Why this is opt-in and loud
    ///
    /// The key belongs to PJM's own web app, not to you. PJM's Data Miner 2
    /// terms (<https://www.pjm.com/-/media/DotCom/etools/edatafeed/data-license-agreement-edata-feed-data-miner-2.pdf>)
    /// apply "whether as a guest or a registered user" and say: "non-members
    /// are specifically prohibited from republishing data"; you may not access
    /// the tools "more than ... six (6) times per minute for non-members" or
    /// "reverse engineer, disassemble, decompile, decode, adapt, or otherwise
    /// attempt to derive or gain access to the source of the Data or methods
    /// used to compile the Data". The Data Miner page says "Automatic queries
    /// can be received with a PJM account through the Data Miner application
    /// program interfaces (API)", and non-member accounts attest to internal
    /// business use only. Using a key issued for a different purpose sits
    /// outside that documented route. Prefer your own key.
    #[cfg(feature = "pjm-public-web-key")]
    PublicWebApp,
}

impl KeyMode {
    /// Resolve [`KeyMode::Auto`] against the process environment.
    ///
    /// # Errors
    /// [`Error::MissingKey`] when `PJM_API_KEY` is unset or empty;
    /// [`Error::InvalidKey`] when it is set to an unusable value.
    pub fn from_env() -> Result<Self, Error> {
        Self::resolve_with(std::env::var(API_KEY_ENV).ok().as_deref())
    }

    /// Pure core of [`KeyMode::from_env`]: `var` is the value of `PJM_API_KEY`.
    pub(crate) fn resolve_with(var: Option<&str>) -> Result<Self, Error> {
        match var.map(str::trim).filter(|v| !v.is_empty()) {
            Some(key) => ApiKey::new(key).map(Self::User),
            None => Err(Error::MissingKey),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts() {
        let key = ApiKey::new("abc123").unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
    }

    #[test]
    fn key_validation() {
        assert!(ApiKey::new("  ").is_err());
        assert!(ApiKey::new("has space").is_err());
        assert!(ApiKey::new("naïve").is_err());
        assert_eq!(ApiKey::new(" k1 ").unwrap().expose(), "k1");
    }

    #[test]
    fn env_resolution() {
        assert!(matches!(
            KeyMode::resolve_with(None),
            Err(Error::MissingKey)
        ));
        assert!(matches!(
            KeyMode::resolve_with(Some("  ")),
            Err(Error::MissingKey)
        ));
        assert_eq!(
            KeyMode::resolve_with(Some("k")).unwrap(),
            KeyMode::User(ApiKey::new("k").unwrap())
        );
        assert!(KeyMode::resolve_with(Some("a b")).is_err());
    }
}

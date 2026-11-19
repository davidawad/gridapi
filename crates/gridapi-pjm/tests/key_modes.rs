//! `KeyMode::Auto` reads `PJM_API_KEY` and nothing else. One test function so
//! the process environment is touched by exactly one thread.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gridapi_pjm::{Client, Error, API_KEY_ENV, API_PORTAL_URL};

#[test]
fn auto_uses_only_the_environment_key_and_never_falls_back() {
    std::env::remove_var(API_KEY_ENV);

    // No key: building fails with MissingKey naming the registration URL.
    let err = Client::builder().build().unwrap_err();
    assert!(matches!(err, Error::MissingKey), "{err}");
    assert!(err.to_string().contains("https://apiportal.pjm.com/"));
    assert_eq!(API_PORTAL_URL, "https://apiportal.pjm.com/");
    assert!(matches!(Client::new().unwrap_err(), Error::MissingKey));

    // A blank value is the same as unset.
    std::env::set_var(API_KEY_ENV, "   ");
    assert!(matches!(
        Client::builder().build().unwrap_err(),
        Error::MissingKey
    ));

    // An unusable value is InvalidKey, not a silent fallback.
    std::env::set_var(API_KEY_ENV, "has space");
    assert!(matches!(
        Client::builder().build().unwrap_err(),
        Error::InvalidKey(_)
    ));

    // A real-looking key builds.
    std::env::set_var(API_KEY_ENV, "user-provisioned-key");
    assert!(Client::builder().build().is_ok());
    std::env::remove_var(API_KEY_ENV);
}

/// With the feature compiled in, the public key still needs the explicit
/// acknowledgement, and Auto still never reaches for it.
#[cfg(feature = "pjm-public-web-key")]
#[test]
fn public_web_app_mode_needs_the_acknowledgement() {
    use gridapi_pjm::KeyMode;

    let refused = Client::builder()
        .key_mode(KeyMode::PublicWebApp)
        .build()
        .unwrap_err();
    assert!(
        matches!(refused, Error::PublicKeyNotAcknowledged),
        "{refused}"
    );
    assert!(refused.to_string().contains("i_accept_pjm_public_key_risk"));

    assert!(Client::builder()
        .key_mode(KeyMode::PublicWebApp)
        .i_accept_pjm_public_key_risk()
        .build()
        .is_ok());
}

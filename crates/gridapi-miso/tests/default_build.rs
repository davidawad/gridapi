//! Without `miso-public-web` the crate carries no URL for MISO's website-hosted
//! routes (public-api.misoenergy.org, docs.misoenergy.org, the retired broker):
//! MISO's website terms bar automated means on misoenergy.org.

#![cfg(not(feature = "miso-public-web"))]

use gridapi_miso::Endpoints;

#[test]
fn default_endpoints_name_no_website_hosted_route() {
    for endpoints in [
        Endpoints::default(),
        Endpoints::all_at("http://127.0.0.1:9"),
    ] {
        let shown = format!("{endpoints:?}");
        for banned in [
            "public-api.misoenergy.org",
            "docs.misoenergy.org",
            "MISORTWDDataBroker",
        ] {
            assert!(!shown.contains(banned), "{banned} in {shown}");
        }
    }
}

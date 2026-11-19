//! The keyed Data Exchange API (feature `miso-dataexchange`, untested against the live service: no account).

#![cfg(feature = "miso-dataexchange")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

mod common;

use chrono::NaiveDate;
use common::{Reply, Server};
use gridapi_miso::{Client, DataExchangeQuery, Endpoints, Error, PricingProduct};

#[tokio::test]
async fn data_exchange_requires_a_key_and_sends_it_in_the_gateway_header() {
    let server = Server::start(vec![(
        "/pricing/v1/day-ahead/2026-11-16/lmp-expost",
        vec![
            Reply::ok(r#"{"data":[{"node":"A","lmp":1.5}],"page":{"pageNumber":0,"totalPages":2,"lastPage":false}}"#),
            Reply::ok(r#"{"data":[{"node":"B","lmp":2.5}],"page":{"pageNumber":1,"totalPages":2,"lastPage":true}}"#),
        ],
    )])
    .await;
    let eps = Endpoints::all_at(&server.base);
    let keyless = Client::builder()
        .endpoints(eps.clone())
        .build()
        .expect("client");
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).expect("date");
    let q = DataExchangeQuery::new();
    let err = keyless
        .pricing_page(PricingProduct::DayAheadExPost, date, &q)
        .await
        .expect_err("no key");
    assert!(matches!(err, Error::MissingApiKey), "{err}");
    assert!(
        server.requests().is_empty(),
        "nothing is sent without a key"
    );

    let keyed = Client::builder()
        .endpoints(eps)
        .api_key("secret-key")
        .build()
        .expect("client");
    let rows = keyed
        .data_exchange_all_pages(&PricingProduct::DayAheadExPost.path(date), &q)
        .await
        .expect("pages");
    assert_eq!(rows.len(), 2);
    let seen = server.requests();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0]
            .headers
            .get("ocp-apim-subscription-key")
            .map(String::as_str),
        Some("secret-key")
    );
    assert!(
        seen[1].target.contains("pageNumber=1"),
        "{}",
        seen[1].target
    );
}

/// Needs a MISO account we do not have: untested. Live: never run in public CI.
#[tokio::test]
#[ignore = "needs MISO_API_KEY (no account); the feature is untested"]
async fn live_data_exchange_needs_a_key() {
    let keyless = Client::new()
        .expect("client")
        .data_exchange_page(
            "pricing/v1/day-ahead/2026-11-16/lmp-expost",
            &DataExchangeQuery::new(),
        )
        .await
        .expect_err("no key");
    assert!(matches!(keyless, Error::MissingApiKey));
}

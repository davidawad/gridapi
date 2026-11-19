//! Retry, backoff and header behaviour against an in-process server.

#![cfg(feature = "miso-public-web")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

mod common;

use std::time::Duration;

use common::{Reply, Server};
use gridapi_miso::{Client, Endpoints, Error, RetryPolicy};

const PATH: &str = "/api/Snapshot";
const BODY: &str = r#"[{"t":"Current Demand (MW)","v":"1","d":"x"}]"#;

fn fast(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(20),
    }
}

fn client(server: &Server, retry: RetryPolicy) -> Client {
    Client::builder()
        .endpoints(Endpoints::all_at(&server.base))
        .retry(retry)
        .user_agent("test-agent/9")
        .build()
        .expect("client")
}

#[tokio::test]
async fn retries_503_then_succeeds() {
    let server = Server::start(vec![(
        PATH,
        vec![
            Reply::status(503, "busy"),
            Reply::status(502, "bad gw"),
            Reply::ok(BODY),
        ],
    )])
    .await;
    let c = client(&server, fast(4));
    let snap = c.snapshot().await.expect("third attempt succeeds");
    assert_eq!(snap.len(), 1);
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn honours_429_with_retry_after() {
    let server = Server::start(vec![(
        PATH,
        vec![
            Reply::status(429, "slow down").with_header("retry-after", "1"),
            Reply::ok(BODY),
        ],
    )])
    .await;
    let c = client(
        &server,
        RetryPolicy {
            max_delay: Duration::from_millis(1200),
            ..fast(3)
        },
    );
    let started = std::time::Instant::now();
    c.snapshot().await.expect("retry succeeds");
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "waited {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn gives_up_after_max_attempts_and_reports_the_last_status() {
    let server = Server::start(vec![(PATH, vec![Reply::status(500, "boom")])]).await;
    let c = client(&server, fast(3));
    let err = c.snapshot().await.expect_err("always 500");
    assert!(matches!(err, Error::Status { status: 500, .. }), "{err}");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn client_errors_are_not_retried() {
    let server = Server::start(vec![(PATH, vec![Reply::status(403, "no")])]).await;
    let c = client(&server, fast(5));
    let err = c.snapshot().await.expect_err("403");
    assert!(matches!(err, Error::Status { status: 403, .. }), "{err}");
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn sends_the_configured_user_agent() {
    let server = Server::start(vec![(PATH, vec![Reply::ok(BODY)])]).await;
    let c = client(&server, fast(1));
    c.snapshot().await.expect("ok");
    let ua = server.requests()[0].headers.get("user-agent").cloned();
    assert_eq!(ua.as_deref(), Some("test-agent/9"));
}

#[tokio::test]
async fn malformed_json_is_a_decode_error_not_a_panic() {
    let server = Server::start(vec![(PATH, vec![Reply::ok("<html>maintenance</html>")])]).await;
    let c = client(&server, fast(1));
    let err = c.snapshot().await.expect_err("html");
    assert!(matches!(err, Error::Decode { .. }), "{err}");
}

#[tokio::test]
async fn connection_refused_is_a_retryable_transport_error() {
    let c = Client::builder()
        .endpoints(Endpoints::all_at("http://127.0.0.1:1"))
        .retry(fast(2))
        .build()
        .expect("client");
    let err = c.snapshot().await.expect_err("refused");
    assert!(matches!(err, Error::Transport { .. }), "{err}");
    assert!(err.is_retryable());
}

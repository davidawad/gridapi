//! Retry, backoff and header behaviour against an in-process server.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]

mod common;

use std::time::Duration;

use common::{Reply, Server};
use gridapi_spp::{Client, Error, FileSource, RetryPolicy};

const PATH: &str = "/file-browser-api/download/hourly-load";

fn fast(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        base_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(20),
    }
}

fn client(server: &Server, retry: RetryPolicy) -> Client {
    Client::builder()
        .base_url(server.base.clone())
        .retry(retry)
        .user_agent("test-agent/9")
        .build()
        .expect("client")
}

async fn fetch(c: &Client) -> Result<Vec<u8>, Error> {
    c.download(&FileSource::from_static("hourly-load"), "/x.csv")
        .await
}

#[tokio::test]
async fn retries_5xx_then_succeeds() {
    let server = Server::start(vec![(
        PATH,
        vec![
            Reply::status(503, "busy"),
            Reply::status(500, "boom"),
            Reply::ok("a,b\n"),
        ],
    )])
    .await;
    let body = fetch(&client(&server, fast(4)))
        .await
        .expect("third attempt");
    assert_eq!(body, b"a,b\n");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn honours_429_retry_after() {
    let server = Server::start(vec![(
        PATH,
        vec![
            Reply::status(429, "slow").with_header("retry-after", "1"),
            Reply::ok("ok"),
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
    fetch(&c).await.expect("retry succeeds");
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "waited {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn stops_after_max_attempts() {
    let server = Server::start(vec![(PATH, vec![Reply::status(502, "bad")])]).await;
    let err = fetch(&client(&server, fast(3)))
        .await
        .expect_err("always 502");
    assert!(matches!(err, Error::Status { status: 502, .. }), "{err}");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn does_not_retry_404_and_reports_not_found() {
    let server = Server::start(vec![(PATH, vec![Reply::status(404, "")])]).await;
    let err = fetch(&client(&server, fast(5))).await.expect_err("404");
    assert!(err.is_not_found(), "{err}");
    assert!(!err.is_retryable());
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn sends_the_configured_user_agent_and_no_ui_token() {
    let server = Server::start(vec![(PATH, vec![Reply::ok("x")])]).await;
    fetch(&client(&server, fast(1))).await.expect("ok");
    let seen = &server.requests()[0];
    assert_eq!(
        seen.headers.get("user-agent").map(String::as_str),
        Some("test-agent/9")
    );
    assert!(!seen.headers.contains_key("x-spp-ui-token"));
}

#[tokio::test]
async fn connection_refused_is_retryable() {
    let c = Client::builder()
        .base_url("http://127.0.0.1:1")
        .retry(fast(2))
        .build()
        .expect("client");
    let err = fetch(&c).await.expect_err("refused");
    assert!(matches!(err, Error::Transport { .. }), "{err}");
    assert!(err.is_retryable());
}

#[tokio::test]
async fn html_instead_of_json_is_a_decode_error() {
    let server = Server::start(vec![(
        "/file-browser-api/",
        vec![Reply::ok("<html>login</html>")],
    )])
    .await;
    let c = client(&server, fast(1));
    let err = c
        .list(&FileSource::from_static("hourly-load"), "/")
        .await
        .expect_err("html");
    assert!(matches!(err, Error::Decode { .. }), "{err}");
}

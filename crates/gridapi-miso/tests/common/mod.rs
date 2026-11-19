//! A tiny in-process HTTP/1.1 server for tests: scripted responses per path, recorded requests.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed expectation should abort the test"
)]
#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One scripted response.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            body: body.into(),
            headers: vec![],
        }
    }
    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            headers: vec![],
        }
    }
    pub fn with_header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_owned(), v.to_owned()));
        self
    }
}

/// A recorded request.
#[derive(Debug, Clone)]
pub struct Seen {
    pub target: String,
    pub headers: HashMap<String, String>,
}

pub struct Server {
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    /// Start a server. Each path pops its next scripted reply; the last one repeats.
    pub async fn start(script: Vec<(&str, Vec<Reply>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let routes: HashMap<String, Vec<Reply>> =
            script.into_iter().map(|(p, r)| (p.to_owned(), r)).collect();
        let routes = Arc::new(Mutex::new(routes));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_task = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let routes = Arc::clone(&routes);
                let seen = Arc::clone(&seen_task);
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 2048];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match sock.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    let mut lines = text.split("\r\n");
                    let target = lines
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_owned();
                    let headers: HashMap<String, String> = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                        .collect();
                    seen.lock().expect("lock").push(Seen {
                        target: target.clone(),
                        headers,
                    });
                    let path = target.split('?').next().unwrap_or("/").to_owned();
                    let reply = {
                        let mut routes = routes.lock().expect("lock");
                        match routes.get_mut(&path) {
                            Some(list) if list.len() > 1 => list.remove(0),
                            Some(list) => list
                                .first()
                                .cloned()
                                .unwrap_or_else(|| Reply::status(404, "empty")),
                            None => Reply::status(404, "no such route"),
                        }
                    };
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\ncontent-type: application/json\r\n",
                        reply.status,
                        reply.body.len()
                    );
                    for (k, v) in &reply.headers {
                        let _ = write!(head, "{k}: {v}\r\n");
                    }
                    head.push_str("\r\n");
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&reply.body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Self { base, seen }
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("lock").clone()
    }
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

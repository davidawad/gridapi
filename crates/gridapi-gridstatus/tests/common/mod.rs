//! Minimal scripted HTTP/1.1 server for tests (no extra dependencies).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One scripted reply.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
}

/// A request as the server saw it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    pub headers: HashMap<String, String>,
}

impl Recorded {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    /// Decoded query pairs.
    pub fn query(&self) -> HashMap<String, String> {
        let q = self.target.split_once('?').map_or("", |(_, q)| q);
        q.split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (decode(k), decode(v))
            })
            .collect()
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

pub struct Mock {
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl Mock {
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

/// Start a server answering requests with `script` in order; once the script
/// is exhausted the last reply repeats.
pub async fn serve(script: Vec<Reply>) -> Mock {
    assert!(!script.is_empty(), "script must have at least one reply");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    let script = Arc::new(script);
    tokio::spawn(async move {
        let mut served = 0_usize;
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let reply = script[served.min(script.len() - 1)].clone();
            served += 1;
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0_u8; 2048];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let mut lines = head.split("\r\n");
                let first = lines.next().unwrap_or_default();
                let mut parts = first.split(' ');
                let method = parts.next().unwrap_or_default().to_owned();
                let target = parts.next().unwrap_or_default().to_owned();
                let headers = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                    .collect();
                log.lock().unwrap().push(Recorded {
                    method,
                    target,
                    headers,
                });
                let mut out = format!(
                    "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                    reply.status,
                    reply.body.len()
                );
                for (k, v) in &reply.headers {
                    writeln!(out, "{k}: {v}\r").unwrap();
                }
                out.push_str("\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&reply.body).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Mock {
        base_url: format!("http://{addr}/v1"),
        requests,
    }
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    std::fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

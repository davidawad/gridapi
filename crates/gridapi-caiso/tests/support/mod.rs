//! A scripted HTTP/1.1 server for client tests: no mock-server dependency.

// Each integration-test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

/// One scripted response.
#[derive(Clone)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub delay: Duration,
}

impl Canned {
    pub fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
            delay: Duration::ZERO,
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            ..Self::ok(body.as_bytes().to_vec())
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

/// A running server. Responses are served in order; the last one repeats.
pub struct MockServer {
    pub addr: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
}

impl MockServer {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The request head of every request seen so far (request line plus headers).
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// The decoded query parameter `key` of the `n`th request's request line.
    pub fn query_param(&self, n: usize, key: &str) -> Option<String> {
        let head = self.requests().get(n)?.clone();
        let target = head.split_whitespace().nth(1)?.to_owned();
        let query = target.split_once('?')?.1.to_owned();
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == key).then(|| v.replace("%3A", ":").replace("%2C", ","))
        })
    }
}

pub async fn serve(script: Vec<Canned>) -> Result<MockServer, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    let cursor = Arc::new(Mutex::new(0usize));
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (log, script, cursor) = (Arc::clone(&log), script.clone(), Arc::clone(&cursor));
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let index = {
                    let Ok(mut c) = cursor.lock() else { return };
                    let i = *c;
                    *c += 1;
                    i
                };
                if let Ok(mut l) = log.lock() {
                    l.push(String::from_utf8_lossy(&head).into_owned());
                }
                let Some(canned) = script.get(index).or(script.last()) else {
                    return;
                };
                tokio::time::sleep(canned.delay).await;
                let mut reply = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    canned.status,
                    canned.body.len()
                );
                for (k, v) in &canned.headers {
                    let _ = write!(reply, "{k}: {v}\r\n");
                }
                reply.push_str("\r\n");
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.write_all(&canned.body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    Ok(MockServer { addr, requests })
}

/// Zips `files` the way OASIS does.
pub fn zip_of(files: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        writer.start_file(*name, zip::write::SimpleFileOptions::default())?;
        writer.write_all(data)?;
    }
    Ok(writer.finish()?.into_inner())
}

pub fn fixture(name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    Ok(std::fs::read(path)?)
}

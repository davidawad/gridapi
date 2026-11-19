//! A tiny scripted HTTP/1.1 server for client tests. No external mock crate.
//!
//! Routes map a request path (without query) to a queue of canned responses.
//! Each request consumes the head of the queue; the last response repeats.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One canned response.
#[derive(Clone)]
pub struct Canned {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Canned {
    pub fn ok(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: format!("status {status}"),
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_owned(), v.to_owned()));
        self
    }
}

/// A recorded request.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Server {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn paths(&self) -> Vec<String> {
        self.requests().into_iter().map(|s| s.path).collect()
    }
}

pub async fn serve(routes: Vec<(&str, Vec<Canned>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let routes: Arc<Mutex<HashMap<String, Vec<Canned>>>> = Arc::new(Mutex::new(
        routes.into_iter().map(|(p, r)| (p.to_owned(), r)).collect(),
    ));
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_task = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
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
                let request_line = lines.next().unwrap_or_default();
                let target = request_line.split_whitespace().nth(1).unwrap_or("/");
                let path = target.split('?').next().unwrap_or("/").to_owned();
                let headers = lines
                    .take_while(|l| !l.is_empty())
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                    .collect();
                if let Ok(mut g) = seen.lock() {
                    g.push(Seen {
                        path: target.to_owned(),
                        headers,
                    });
                }
                let canned = routes.lock().ok().and_then(|mut r| {
                    let queue = r.get_mut(&path)?;
                    if queue.len() > 1 {
                        Some(queue.remove(0))
                    } else {
                        queue.first().cloned()
                    }
                });
                let canned = canned.unwrap_or_else(|| Canned::status(404));
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain; charset=utf-8\r\n",
                    canned.status,
                    canned.body.len()
                );
                for (k, v) in &canned.headers {
                    head.extend([k.as_str(), ": ", v.as_str(), "\r\n"]);
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(canned.body.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Server {
        base: format!("http://{addr}/public"),
        seen,
    }
}

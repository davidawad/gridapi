//! Minimal scripted HTTP server for offline tests (`tiny_http` on a thread).

#![allow(dead_code, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tiny_http::{Header, Response, Server};

/// One scripted reply.
#[derive(Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.into(),
        }
    }

    pub fn bytes(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn with_header(mut self, k: &'static str, v: &str) -> Self {
        self.headers.push((k, v.to_owned()));
        self
    }
}

pub struct MockServer {
    pub base: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    server: Arc<Server>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    /// Serve every request with `handler(request_index, url)`.
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(usize, &str) -> Reply + Send + 'static,
    {
        let server = Arc::new(Server::http("127.0.0.1:0").expect("bind mock server"));
        let base = format!("http://{}", server.server_addr().to_ip().expect("ip addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (s, r) = (Arc::clone(&server), Arc::clone(&requests));
        let thread = std::thread::spawn(move || {
            for req in s.incoming_requests() {
                let url = req.url().to_owned();
                let idx = {
                    let mut g = r.lock().expect("lock");
                    g.push(url.clone());
                    g.len() - 1
                };
                let reply = handler(idx, &url);
                let mut resp = Response::from_data(reply.body).with_status_code(reply.status);
                for (k, v) in reply.headers {
                    resp.add_header(
                        Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header"),
                    );
                }
                let _ = req.respond(resp);
            }
        });
        Self {
            base,
            requests,
            server,
            thread: Some(thread),
        }
    }

    /// Serve a fixed list of replies in order (the last one repeats).
    pub fn script(replies: Vec<Reply>) -> Self {
        Self::start(move |i, _| replies[i.min(replies.len() - 1)].clone())
    }

    pub fn seen(&self) -> Vec<String> {
        self.requests.lock().expect("lock").clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

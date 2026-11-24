//! Development mode. `rustkit dev` turns it on by starting the application
//! with these environment variables:
//!
//! - `RUSTKIT_DEV=1`: serve the dev client and its event stream, and stop
//!   browsers from caching prerendered pages.
//! - `RUSTKIT_DEV_BUILD`: a number that changes with every rebuild. Pages
//!   carry it; the dev client reloads when the server reports a different one.
//! - `RUSTKIT_DEV_LISTEN_FD`: a listening socket the dev process owns, so
//!   connections wait in its backlog while the application restarts.
//! - `RUSTKIT_DEV_SESSIONS`: a file that keeps sessions across restarts.
//!
//! The dev process writes one JSON status per line to the application's
//! stdin (`{"status":"building"}`, `{"status":"failed","output":"…"}`,
//! `{"status":"idle"}`). They are forwarded to every open page.

use std::convert::Infallible;
use std::io::BufRead;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};

use axum::Router;
use axum::body::Body;
use axum::http::{Response, header};
use axum::routing::get;
use tokio::sync::mpsc;

const CLIENT_JS: &str = include_str!("dev.js");

/// True when the application runs under `rustkit dev`.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("RUSTKIT_DEV").is_ok_and(|v| v == "1"))
}

pub(crate) fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| std::env::var("RUSTKIT_DEV_BUILD").unwrap_or_else(|_| "0".into()))
}

pub(crate) fn session_file() -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    std::env::var_os("RUSTKIT_DEV_SESSIONS").map(PathBuf::from)
}

/// The `<script>` tag every page gets in development, or nothing.
pub(crate) fn script_tag() -> String {
    if enabled() {
        format!("<script src=\"/_rk/dev.js\" data-build=\"{}\"></script>\n", crate::js::escape(build_id()))
    } else {
        String::new()
    }
}

/// Binds `addr`, or adopts the socket `rustkit dev` passed down.
pub async fn listener(addr: &str) -> std::io::Result<tokio::net::TcpListener> {
    #[cfg(unix)]
    if let Some(fd) = std::env::var("RUSTKIT_DEV_LISTEN_FD").ok().and_then(|v| v.parse::<i32>().ok()) {
        use std::os::fd::FromRawFd;
        // SAFETY: `rustkit dev` hands this process exactly one inherited
        // listening socket under this number, and nothing else owns it.
        let std = unsafe { std::net::TcpListener::from_raw_fd(fd) };
        std.set_nonblocking(true)?;
        return tokio::net::TcpListener::from_std(std);
    }
    tokio::net::TcpListener::bind(addr).await
}

/// Fans status lines from the dev process out to every open page.
#[derive(Default)]
struct Hub {
    last: Mutex<Option<String>>,
    subscribers: Mutex<Vec<mpsc::UnboundedSender<String>>>,
}

impl Hub {
    fn publish(&self, json: &str) {
        let frame = format!("event: status\ndata: {json}\n\n");
        *self.last.lock().unwrap() = Some(frame.clone());
        self.subscribers.lock().unwrap().retain(|tx| tx.send(frame.clone()).is_ok());
    }

    fn subscribe(&self) -> Events {
        let (tx, rx) = mpsc::unbounded_channel();
        // Reconnect quickly after a restart; then say which build this is.
        let mut first = format!("retry: 200\nevent: build\ndata: {}\n\n", build_id());
        if let Some(last) = self.last.lock().unwrap().clone() {
            first.push_str(&last);
        }
        self.subscribers.lock().unwrap().push(tx);
        Events { first: Some(first), rx }
    }
}

struct Events {
    first: Option<String>,
    rx: mpsc::UnboundedReceiver<String>,
}

impl futures_core::Stream for Events {
    type Item = Result<String, Infallible>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(first) = self.first.take() {
            return Poll::Ready(Some(Ok(first)));
        }
        self.rx.poll_recv(cx).map(|m| m.map(Ok))
    }
}

/// Adds the dev routes and starts reading status lines from stdin.
pub(crate) fn install(router: Router) -> Router {
    let hub = Arc::new(Hub::default());
    let reader = hub.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.starts_with('{') {
                reader.publish(line);
            }
        }
        // `rustkit dev` went away; don't outlive it holding its socket.
        if std::env::var_os("RUSTKIT_DEV_LISTEN_FD").is_some() {
            std::process::exit(0);
        }
    });
    router
        .route(
            "/_rk/dev.js",
            get(|| async {
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                    .header(header::CACHE_CONTROL, "no-store")
                    .body(Body::from(CLIENT_JS))
                    .unwrap()
            }),
        )
        .route(
            "/_rk/dev/events",
            get(move || {
                let events = hub.subscribe();
                async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::CACHE_CONTROL, "no-store")
                        .body(Body::from_stream(events))
                        .unwrap()
                }
            }),
        )
}

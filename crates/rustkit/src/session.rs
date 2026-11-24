//! Server-side sessions identified by an opaque cookie.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const COOKIE_NAME: &str = "rk_session";

#[derive(Debug, Clone)]
pub struct SessionData {
    pub user_id: Option<String>,
    pub csrf: String,
    last_seen: Instant,
}

/// In-memory session store. Sessions expire after `idle_timeout` without use.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<String, SessionData>>>,
    idle_timeout: Duration,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(12 * 60 * 60))
    }
}

impl SessionStore {
    pub fn new(idle_timeout: Duration) -> Self {
        Self { inner: Arc::default(), idle_timeout }
    }

    pub fn load(&self, id: &str) -> Option<SessionData> {
        let mut map = self.inner.lock().unwrap();
        let expired = map.get(id).is_some_and(|s| s.last_seen.elapsed() > self.idle_timeout);
        if expired {
            map.remove(id);
            return None;
        }
        let s = map.get_mut(id)?;
        s.last_seen = Instant::now();
        Some(s.clone())
    }

    /// Creates a session and returns its id.
    pub fn create(&self, user_id: Option<String>) -> (String, SessionData) {
        let id = random_token();
        let data = SessionData { user_id, csrf: random_token(), last_seen: Instant::now() };
        self.inner.lock().unwrap().insert(id.clone(), data.clone());
        (id, data)
    }

    pub fn destroy(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
    }
}

/// 256 bits from the operating system's CSPRNG, hex encoded.
pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("operating system random source unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two secrets without exiting early on the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

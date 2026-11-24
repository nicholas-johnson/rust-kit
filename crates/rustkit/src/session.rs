//! Server-side sessions identified by an opaque cookie.

use std::collections::HashMap;
use std::path::PathBuf;
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
    /// Development only: a file the sessions are saved to whenever one is
    /// created or destroyed, so a restart doesn't sign everyone out.
    file: Option<Arc<PathBuf>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(12 * 60 * 60))
    }
}

impl SessionStore {
    pub fn new(idle_timeout: Duration) -> Self {
        Self { inner: Arc::default(), idle_timeout, file: None }
    }

    /// A store that loads its sessions from `path` and saves them back on
    /// every change. Meant for `rustkit dev`; idle times restart on load.
    pub fn persistent(path: PathBuf) -> Self {
        let mut store = Self::default();
        let saved: HashMap<String, (Option<String>, String)> =
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let now = Instant::now();
        *store.inner.lock().unwrap() = saved
            .into_iter()
            .map(|(id, (user_id, csrf))| (id, SessionData { user_id, csrf, last_seen: now }))
            .collect();
        store.file = Some(Arc::new(path));
        store
    }

    fn save(&self, map: &HashMap<String, SessionData>) {
        let Some(path) = &self.file else { return };
        let saved: HashMap<&str, (&Option<String>, &str)> =
            map.iter().map(|(id, d)| (id.as_str(), (&d.user_id, d.csrf.as_str()))).collect();
        let tmp = path.with_extension("tmp");
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            std::fs::write(&tmp, serde_json::to_vec(&saved).unwrap_or_default())?;
            std::fs::rename(&tmp, path.as_path())
        });
        if let Err(e) = written {
            eprintln!("rustkit: couldn't save dev sessions to {}: {e}", path.display());
        }
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
        let mut map = self.inner.lock().unwrap();
        map.insert(id.clone(), data.clone());
        self.save(&map);
        (id, data)
    }

    pub fn destroy(&self, id: &str) {
        let mut map = self.inner.lock().unwrap();
        if map.remove(id).is_some() {
            self.save(&map);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_store_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("rustkit-sessions-{}", random_token()));
        let path = dir.join("sessions.json");

        let first = SessionStore::persistent(path.clone());
        let (alice, data) = first.create(Some("alice".into()));
        let (gone, _) = first.create(None);
        first.destroy(&gone);

        let second = SessionStore::persistent(path);
        let loaded = second.load(&alice).expect("session kept");
        assert_eq!(loaded.user_id.as_deref(), Some("alice"));
        assert_eq!(loaded.csrf, data.csrf);
        assert!(second.load(&gone).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

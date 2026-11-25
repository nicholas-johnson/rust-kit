//! Server-side sessions identified by an opaque cookie.
//!
//! The default store is a map in this process. [`SessionStore::persistent`]
//! writes that map to a file so a restart keeps the sessions. The cookie
//! still holds only the session id.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const COOKIE_NAME: &str = "rk_session";

#[derive(Debug, Clone)]
pub struct SessionData {
    pub user_id: Option<String>,
    pub csrf: String,
    last_seen: SystemTime,
}

/// Session store. Sessions expire after `idle_timeout` without use.
///
/// With a file, the same sessions are loaded the next time a process opens
/// that path. The file is one process's copy: two processes must not share it.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<String, SessionData>>>,
    idle_timeout: Duration,
    /// When set, the map is saved here whenever a session is created, used,
    /// or destroyed.
    file: Option<Arc<PathBuf>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSession {
    user_id: Option<String>,
    csrf: String,
    last_seen_ms: u64,
}

impl SavedSession {
    fn from_data(data: &SessionData) -> Self {
        let last_seen_ms = data
            .last_seen
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        Self {
            user_id: data.user_id.clone(),
            csrf: data.csrf.clone(),
            last_seen_ms,
        }
    }

    fn into_data(self) -> SessionData {
        SessionData {
            user_id: self.user_id,
            csrf: self.csrf,
            last_seen: UNIX_EPOCH + Duration::from_millis(self.last_seen_ms),
        }
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(12 * 60 * 60))
    }
}

impl SessionStore {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            inner: Arc::default(),
            idle_timeout,
            file: None,
        }
    }

    /// Loads sessions from `path` and saves them back whenever one is created,
    /// used, or destroyed. A missing file starts empty. A file written before
    /// `last_seen` was stored is treated as used just now.
    pub fn persistent(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut store = Self::default();
        *store.inner.lock().unwrap() = read_sessions(&path);
        store.file = Some(Arc::new(path));
        store
    }

    fn save(&self, map: &HashMap<String, SessionData>) {
        let Some(path) = &self.file else { return };
        let saved: HashMap<&str, SavedSession> = map
            .iter()
            .map(|(id, data)| (id.as_str(), SavedSession::from_data(data)))
            .collect();
        let bytes = match serde_json::to_vec(&saved) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("rustkit: couldn't encode sessions: {e}");
                return;
            }
        };
        let tmp = path.with_extension("tmp");
        let written = (|| {
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            write_private(&tmp, &bytes)?;
            std::fs::rename(&tmp, path.as_path())
        })();
        if let Err(e) = written {
            eprintln!("rustkit: couldn't save sessions to {}: {e}", path.display());
        }
    }

    pub fn load(&self, id: &str) -> Option<SessionData> {
        let mut map = self.inner.lock().unwrap();
        let now = SystemTime::now();
        let expired = map
            .get(id)
            .is_some_and(|s| idle_expired(s.last_seen, self.idle_timeout, now));
        if expired {
            map.remove(id);
            self.save(&map);
            return None;
        }
        let data = map.get_mut(id)?;
        data.last_seen = now;
        let data = data.clone();
        self.save(&map);
        Some(data)
    }

    /// Creates a session and returns its id.
    pub fn create(&self, user_id: Option<String>) -> (String, SessionData) {
        let id = random_token();
        let data = SessionData {
            user_id,
            csrf: random_token(),
            last_seen: SystemTime::now(),
        };
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

fn idle_expired(last_seen: SystemTime, timeout: Duration, now: SystemTime) -> bool {
    now.duration_since(last_seen).unwrap_or(Duration::ZERO) > timeout
}

fn read_sessions(path: &Path) -> HashMap<String, SessionData> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => return HashMap::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(e) => {
            eprintln!(
                "rustkit: couldn't read sessions from {}: {e}",
                path.display()
            );
            return HashMap::new();
        }
    };
    match decode_sessions(&bytes) {
        Some(map) => map,
        None => {
            eprintln!(
                "rustkit: ignoring unreadable session file {}; the next change replaces it",
                path.display()
            );
            HashMap::new()
        }
    }
}

/// Current files are a map of id to `{user_id, csrf, last_seen_ms}`.
/// Files written before that are a map of id to `[user_id, csrf]`.
fn decode_sessions(bytes: &[u8]) -> Option<HashMap<String, SessionData>> {
    if let Ok(saved) = serde_json::from_slice::<HashMap<String, SavedSession>>(bytes) {
        return Some(
            saved
                .into_iter()
                .map(|(id, rec)| (id, rec.into_data()))
                .collect(),
        );
    }
    let saved: HashMap<String, (Option<String>, String)> = serde_json::from_slice(bytes).ok()?;
    let now = SystemTime::now();
    Some(
        saved
            .into_iter()
            .map(|(id, (user_id, csrf))| {
                (
                    id,
                    SessionData {
                        user_id,
                        csrf,
                        last_seen: now,
                    },
                )
            })
            .collect(),
    )
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.sync_all()?;
    Ok(())
}

/// 256 bits from the operating system's CSPRNG, hex encoded.
pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("operating system random source unavailable");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares two secrets without exiting early on the first difference.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rustkit-sessions-{}", random_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn persistent_store_survives_a_restart() {
        let dir = temp_dir();
        let path = dir.join("sessions.json");

        let first = SessionStore::persistent(path.clone());
        let (alice, data) = first.create(Some("alice".into()));
        let (gone, _) = first.create(None);
        first.destroy(&gone);

        let second = SessionStore::persistent(&path);
        let loaded = second.load(&alice).expect("session kept");
        assert_eq!(loaded.user_id.as_deref(), Some("alice"));
        assert_eq!(loaded.csrf, data.csrf);
        assert!(second.load(&gone).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistent_store_keeps_the_idle_clock() {
        let dir = temp_dir();
        let path = dir.join("sessions.json");
        let old = u64::try_from(
            SystemTime::now()
                .checked_sub(Duration::from_secs(13 * 60 * 60))
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let fresh = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let body = serde_json::json!({
            "old": {"user_id": "alice", "csrf": "abc", "last_seen_ms": old},
            "new": {"user_id": null, "csrf": "def", "last_seen_ms": fresh},
        });
        std::fs::write(&path, serde_json::to_vec(&body).unwrap()).unwrap();

        let store = SessionStore::persistent(&path);
        assert!(
            store.load("old").is_none(),
            "idle timeout still applies after a restart"
        );
        assert_eq!(store.load("new").unwrap().csrf, "def");

        let again = SessionStore::persistent(&path);
        assert!(
            again.load("old").is_none(),
            "presenting an expired id removes it from the file"
        );
        assert!(again.load("new").is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persistent_store_reads_a_legacy_file() {
        let dir = temp_dir();
        let path = dir.join("sessions.json");
        std::fs::write(&path, br#"{"abc":["alice","tok"]}"#).unwrap();

        let store = SessionStore::persistent(&path);
        let loaded = store.load("abc").expect("legacy file");
        assert_eq!(loaded.user_id.as_deref(), Some("alice"));
        assert_eq!(loaded.csrf, "tok");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn session_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir();
        let path = dir.join("sessions.json");
        let store = SessionStore::persistent(&path);
        store.create(Some("alice".into()));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

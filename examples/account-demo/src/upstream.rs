//! A stand-in for the upstream accounts REST service.
//!
//! It keeps records in memory, adds a fixed delay to imitate a network hop,
//! and counts calls so tests can prove how many reads a page made. Swapping in
//! a real HTTP client changes this file only; resolvers call the same methods.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustkit::Error;

#[derive(Clone, Debug)]
pub struct AccountRecord {
    pub id: String,
    pub display_name: String,
    /// Present upstream, never sent to the browser.
    pub email: String,
}

#[derive(Clone, Debug)]
pub struct ActivityRecord {
    pub account_id: String,
    pub description: String,
}

pub struct AccountsService {
    accounts: Mutex<HashMap<String, AccountRecord>>,
    activity: Vec<ActivityRecord>,
    latency: Duration,
    /// `GET /accounts/{id}` calls.
    pub account_reads: AtomicUsize,
    /// `GET /accounts/{id}/activity` calls.
    pub activity_reads: AtomicUsize,
}

impl AccountsService {
    pub fn demo(latency: Duration) -> Self {
        let acct = |id: &str, name: &str| {
            (id.to_owned(), AccountRecord { id: id.into(), display_name: name.into(), email: format!("{id}@example.com") })
        };
        let act = |id: &str, d: &str| ActivityRecord { account_id: id.into(), description: d.into() };
        Self {
            accounts: Mutex::new(HashMap::from([acct("alice", "Alice Liddell"), acct("bob", "Bob Marley")])),
            activity: vec![
                act("alice", "Signed in from a new browser"),
                act("alice", "Changed notification settings"),
                act("alice", "Downloaded March invoice"),
                act("bob", "Added a payment method"),
                act("bob", "Invited a teammate"),
            ],
            latency,
            account_reads: AtomicUsize::new(0),
            activity_reads: AtomicUsize::new(0),
        }
    }

    /// `GET /accounts/{id}`
    pub async fn get(&self, id: &str) -> Result<AccountRecord, Error> {
        self.account_reads.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        self.accounts.lock().unwrap().get(id).cloned().ok_or(Error::NotFound)
    }

    /// `GET /accounts/{id}/activity`
    pub async fn activity(&self, id: &str) -> Result<Vec<ActivityRecord>, Error> {
        self.activity_reads.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        Ok(self.activity.iter().filter(|a| a.account_id == id).cloned().collect())
    }

    /// `PATCH /accounts/{id}` with `{ "displayName": … }`
    pub async fn update_display_name(&self, id: &str, name: &str) -> Result<(), Error> {
        tokio::time::sleep(self.latency).await;
        let mut map = self.accounts.lock().unwrap();
        let rec = map.get_mut(id).ok_or(Error::NotFound)?;
        rec.display_name = name.to_owned();
        Ok(())
    }
}

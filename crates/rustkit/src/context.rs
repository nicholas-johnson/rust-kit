//! The scoped context handed to resolvers, page renderers and actions.

use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::error::Error;
use crate::session::{COOKIE_NAME, SessionData, SessionStore};

/// The signed-in user for the current request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
}

struct Inner<S> {
    app: Arc<S>,
    store: SessionStore,
    session: Mutex<Option<(String, SessionData)>>,
    /// Set when anything about the response depends on the session.
    personalised: AtomicBool,
    set_cookie: Mutex<Option<String>>,
    secure_cookies: bool,
    build_time: bool,
}

/// Per-request context. Dereferences to the application state, so a resolver
/// can write `ctx.accounts_service`. Cloning is cheap.
pub struct RequestContext<S> {
    inner: Arc<Inner<S>>,
}

impl<S> Clone for RequestContext<S> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<S> Deref for RequestContext<S> {
    type Target = S;
    fn deref(&self) -> &S {
        &self.inner.app
    }
}

impl<S> RequestContext<S> {
    pub(crate) fn new(
        app: Arc<S>,
        store: SessionStore,
        cookie_header: Option<&str>,
        secure_cookies: bool,
    ) -> Self {
        let session = cookie_header
            .and_then(|h| cookie_value(h, COOKIE_NAME))
            .and_then(|id| store.load(id).map(|data| (id.to_owned(), data)));
        Self {
            inner: Arc::new(Inner {
                app,
                store,
                session: Mutex::new(session),
                personalised: AtomicBool::new(false),
                set_cookie: Mutex::new(None),
                secure_cookies,
                build_time: false,
            }),
        }
    }

    /// A context with no request, used to prerender public routes at build time.
    pub(crate) fn for_build(app: Arc<S>) -> Self {
        Self {
            inner: Arc::new(Inner {
                app,
                store: SessionStore::default(),
                session: Mutex::new(None),
                personalised: AtomicBool::new(false),
                set_cookie: Mutex::new(None),
                secure_cookies: false,
                build_time: true,
            }),
        }
    }

    fn touch(&self) {
        self.inner.personalised.store(true, Ordering::Relaxed);
    }

    /// True once the response has read anything from the session.
    pub fn is_personalised(&self) -> bool {
        self.inner.personalised.load(Ordering::Relaxed)
    }

    pub fn user(&self) -> Option<User> {
        self.touch();
        let s = self.inner.session.lock().unwrap();
        s.as_ref().and_then(|(_, d)| d.user_id.clone()).map(|id| User { id })
    }

    pub fn require_user(&self) -> Result<User, Error> {
        if self.inner.build_time {
            return Err(Error::Internal(
                "a prerendered route read the session; remove `prerender` or load this source on the client"
                    .into(),
            ));
        }
        self.user().ok_or(Error::Unauthorized)
    }

    /// The session's CSRF token, creating an anonymous session if needed.
    pub fn csrf_token(&self) -> String {
        self.touch();
        let mut s = self.inner.session.lock().unwrap();
        if let Some((_, d)) = s.as_ref() {
            return d.csrf.clone();
        }
        let (id, data) = self.inner.store.create(None);
        let token = data.csrf.clone();
        self.set_session_cookie(&id);
        *s = Some((id, data));
        token
    }

    pub(crate) fn check_csrf(&self, submitted: Option<&str>) -> bool {
        let s = self.inner.session.lock().unwrap();
        match (s.as_ref(), submitted) {
            (Some((_, d)), Some(t)) => crate::session::constant_time_eq(&d.csrf, t),
            _ => false,
        }
    }

    /// Signs a user in. The session id is replaced to prevent fixation.
    pub fn login(&self, user_id: impl Into<String>) {
        self.touch();
        let mut s = self.inner.session.lock().unwrap();
        if let Some((old, _)) = s.take() {
            self.inner.store.destroy(&old);
        }
        let (id, data) = self.inner.store.create(Some(user_id.into()));
        self.set_session_cookie(&id);
        *s = Some((id, data));
    }

    pub fn logout(&self) {
        self.touch();
        if let Some((old, _)) = self.inner.session.lock().unwrap().take() {
            self.inner.store.destroy(&old);
        }
        *self.inner.set_cookie.lock().unwrap() =
            Some(format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"));
    }

    fn set_session_cookie(&self, id: &str) {
        let secure = if self.inner.secure_cookies { "; Secure" } else { "" };
        *self.inner.set_cookie.lock().unwrap() =
            Some(format!("{COOKIE_NAME}={id}; Path=/; HttpOnly; SameSite=Lax{secure}"));
    }

    pub(crate) fn take_set_cookie(&self) -> Option<String> {
        self.inner.set_cookie.lock().unwrap().take()
    }
}

fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then_some(v)
    })
}

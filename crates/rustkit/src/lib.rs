//! Rust-kit runtime.
//!
//! The compiler (`rustkit-compiler`) turns page files into Rust render
//! functions and browser modules. This crate supplies what that generated code
//! runs against: the request context, sessions, CSRF checks, the HTTP router,
//! and the JavaScript value semantics the server renderer evaluates with.

mod app;
mod context;
pub mod dev;
mod error;
pub mod js;
mod session;

pub use app::{App, Asset, BoxFut, BuildReport, Form, Manifest, Page, Redirect, RenderFn, Route, SourceApiFn, document};
pub use context::{RequestContext, User};
pub use error::Error;
pub use session::{SessionStore, constant_time_eq, random_token};

/// Re-exports used by generated code, so applications need not depend on them.
#[doc(hidden)]
pub mod __private {
    pub use serde_json;
    pub use tokio::try_join;
}

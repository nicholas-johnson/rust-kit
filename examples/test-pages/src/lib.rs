//! Small pages, each exercising one feature of the compiler and runtime.
//!
//! `tests/pages.rs` checks what the server sends. `tests/parity.rs` runs each
//! page's browser module in Node and checks it renders the same HTML as Rust.

pub mod sources;

use std::sync::Mutex;

use rustkit::{Error, Form, Redirect};

/// Generated from `src/routes/**/*.rk` by `build.rs`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

#[derive(Default)]
pub struct AppState {
    /// The last message posted to `/form/echo`.
    pub last_message: Mutex<String>,
}

async fn echo(ctx: rustkit::RequestContext<AppState>, form: Form) -> Result<Redirect, Error> {
    let msg = form.get("message").unwrap_or("").trim();
    if msg.is_empty() || msg.chars().count() > 200 {
        return Err(Error::BadRequest("Message must be 1 to 200 characters.".into()));
    }
    *ctx.last_message.lock().unwrap() = msg.to_owned();
    Ok(Redirect::to("/form"))
}

pub fn app(secure_cookies: bool) -> rustkit::App<AppState> {
    rustkit::App::new(generated::manifest(), AppState::default())
        .secure_cookies(secure_cookies)
        .action("/form/echo", echo)
}

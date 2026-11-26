//! {{name}}: a Rust-kit application.

pub mod sources;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

/// Shared application state. Every request context dereferences to it, and
/// the generated code refers to it as `crate::AppState`.
pub struct AppState {
    pub app_name: String,
}

/// The application. `secure_cookies` marks the session cookie `Secure`; turn
/// it off only for plain `http://` on localhost.
pub fn app(secure_cookies: bool) -> rustkit::App<AppState> {
    let state = AppState { app_name: "{{name}}".into() };
    rustkit::App::new(generated::manifest(), state).secure_cookies(secure_cookies)
}

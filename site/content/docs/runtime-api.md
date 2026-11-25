---
title: Runtime API
description: Every public item in the rustkit crate, with signatures, behaviour and examples.
section: Reference
order: 2
---

The `rustkit` crate is what your application and the generated code run against. It is small: one application builder, a request context, an error type, a handful of plain structs, two security helpers, and the `js` module that gives the Rust renderer JavaScript's value rules.

```toml
[dependencies]
rustkit = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

Everything below is exported from the crate root unless it says `rustkit::js`.

## App

`App<S>` turns the compiled [manifest](#compiled-output-types) and your application state into an axum `Router`. `S` is your state type, normally `crate::AppState`. All methods are available when `S: Send + Sync + 'static`.

| Method | Signature | What it does |
|---|---|---|
| `new` | `fn new(manifest: Manifest<S>, state: S) -> Self` | Creates the app with an empty in-memory session store, `Secure` cookies on, and `/login` as the login path. |
| `secure_cookies` | `fn secure_cookies(self, on: bool) -> Self` | Whether the session cookie carries `Secure`. Default `true`. Turn it off only for plain-HTTP local development. |
| `login_path` | `fn login_path(self, path: impl Into<String>) -> Self` | Where to send a visitor when something returns `Error::Unauthorized`. Default `/login`. |
| `session_store` | `fn session_store(self, store: SessionStore) -> Self` | Replaces the in-memory store. Under `rustkit dev`, the dev session file is left in place. |
| `action` | `fn action<F, Fut>(self, path: &str, f: F) -> Self` | Registers a `POST` form action. `F: Fn(RequestContext<S>, Form) -> Fut`, `Fut: Future<Output = Result<Redirect, Error>> + Send`. |
| `routes` | `fn routes(&self) -> &[Route<S>]` | The compiled routes, for reporting or tests. |
| `state` | `fn state(&self) -> Arc<S>` | A handle on the shared state, for example to read counters in tests. |
| `prerender` | `async fn prerender(&self) -> Result<Vec<(&'static str, String)>, Error>` | Renders every route marked `prerender` with a request-free context. |
| `build` | `async fn build(&self, dir: &Path) -> std::io::Result<BuildReport>` | Writes prerendered pages as `index.html` files and every asset into `dir`. |
| `into_router` | `async fn into_router(self) -> Result<axum::Router, Error>` | Prerenders, then builds the router. Fails if any prerender fails. |

```rust
pub fn app() -> rustkit::App<AppState> {
    rustkit::App::new(generated::manifest(), AppState::new())
        .secure_cookies(true)
        .action("/account/profile", actions::update_profile)
        .action("/logout", actions::logout)
}
```

### What `into_router` serves

| Path | Method | Behaviour |
|---|---|---|
| A prerendered route | `GET` | HTML rendered once at startup. `Cache-Control: public, max-age=300`. |
| Any other route | `GET` | Renders per request. Adds `Server-Timing: sources;dur=…, render;dur=…`. `Cache-Control: private, no-store` plus `Vary: Cookie` if the render read the session, otherwise `no-cache`. `Error::Unauthorized` becomes a 303 to `{login_path}?next={route}`. |
| `/_rk/source/{name}` | `GET` | Only if some page loads a source with `load="client"`. Runs the resolver and returns JSON, always `private, no-store`. Requests with `Sec-Fetch-Site: cross-site` get 403. Errors are JSON `{"error": "…"}` with the error's status. |
| Each action path | `POST` | Checks origin (403), then the `_csrf` field (400, "This form has expired. Reload the page and try again."), then runs the action. `Ok(Redirect)` becomes a 303; `Error::Unauthorized` redirects to `login_path`. |
| `/_rk/assets/…` | `GET` | Content-hashed JS and CSS. `Cache-Control: public, max-age=31536000, immutable`. |
| Anything else | | 404 error page. |

Error pages are small HTML documents showing the status code and [`public_message`](#error). `Upstream` and `Internal` errors are also printed to stderr with their details. See [Errors](/docs/errors/), [Caching](/docs/caching/) and [Security](/docs/security/).

## RequestContext

`RequestContext<S>` is passed to resolvers (`&RequestContext<S>`), page renderers and actions (by value). It dereferences to your state, so `ctx.accounts_service` reaches a field of `S`. Cloning is cheap: it is an `Arc`. You cannot construct one yourself; the router and the build create them.

| Method | Signature | What it does |
|---|---|---|
| `user` | `fn user(&self) -> Option<User>` | The signed-in user, if any. Marks the response as personalised. |
| `require_user` | `fn require_user(&self) -> Result<User, Error>` | The user, or `Error::Unauthorized`. During a prerender it returns `Error::Internal` instead, explaining that a prerendered route read the session. |
| `csrf_token` | `fn csrf_token(&self) -> String` | The session's CSRF token. Creates an anonymous session (and sets its cookie) if there is none. Marks the response as personalised. Generated code calls this for every `<form method="post">`. |
| `login` | `fn login(&self, user_id: impl Into<String>)` | Signs a user in. Destroys the old session and issues a new id, so a planted session id is useless. |
| `logout` | `fn logout(&self)` | Destroys the session and expires the cookie. |
| `is_personalised` | `fn is_personalised(&self) -> bool` | True once anything has read the session. Decides the cache headers. |

```rust
type Ctx = rustkit::RequestContext<crate::AppState>;

pub async fn account(ctx: &Ctx) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(&user.id).await?;
    Ok(AccountView { display_name: record.display_name })
}
```

### User

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
}
```

The id is whatever string you passed to `login`.

## Error

Every resolver, renderer and action returns `Result<_, rustkit::Error>`. Each variant maps to a status code and a message that is safe to show a visitor.

| Variant | `status()` | `public_message()` |
|---|---|---|
| `Unauthorized` | 401 | Please sign in. |
| `Forbidden` | 403 | You don't have access to this. |
| `NotFound` | 404 | Not found. |
| `BadRequest(String)` | 400 | The string you supplied. |
| `Upstream(String)` | 502 | A service this page depends on is unavailable. Please try again. |
| `Internal(String)` | 500 | Something went wrong. |

| Item | Signature | Notes |
|---|---|---|
| `internal` | `fn internal(e: impl Display) -> Error` | Shorthand for `Error::Internal(e.to_string())`. |
| `status` | `fn status(&self) -> u16` | As above. |
| `public_message` | `fn public_message(&self) -> &str` | As above. Internal details never reach the response. |
| `Display` | | `upstream error: …` and `internal error: …` include the detail; other variants print their public message. |
| `From<serde_json::Error>` | | Becomes `Internal`, so `?` works on serialisation. |

`Error` also implements `std::error::Error`, `Debug`, `Clone`, `PartialEq` and `Eq`.

```rust
return Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()));
```

## Form and Redirect

```rust
#[derive(Debug, Default, Clone)]
pub struct Form(pub HashMap<String, String>);

impl Form {
    pub fn get(&self, name: &str) -> Option<&str>;
}

pub struct Redirect(pub String);

impl Redirect {
    pub fn to(path: impl Into<String>) -> Self;
}
```

`Form` holds the URL-encoded body of a `POST`, including the `_csrf` field. `Redirect` becomes a `303 See Other` with `Cache-Control: no-store`. A path that is not a valid header value redirects to `/`. See [Form actions](/docs/form-actions/).

## Compiled output types

The generated `manifest()` function builds these. You normally only pass a `Manifest` to `App::new` and read `Route` fields for reporting.

```rust
pub struct Manifest<S> {
    pub routes: Vec<Route<S>>,
    pub assets: Vec<Asset>,
    /// Sources some page loads in the browser. Only these get an endpoint.
    pub client_sources: &'static [&'static str],
    pub source_api: SourceApiFn<S>,
}

pub struct Route<S> {
    pub path: &'static str,
    /// The single `{name}` in `path`, when the route has one.
    pub param: Option<&'static str>,
    pub prerender: bool,
    pub render: RenderFn<S>,
    /// Runtime plus page module, uncompressed bytes. 0 for a page with no module.
    pub js_bytes: usize,
    pub css_bytes: usize,
}

pub struct Asset {
    pub path: &'static str,
    pub content_type: &'static str,
    pub body: &'static str,
}

pub struct Page {
    pub html: String,
    /// Wall time spent waiting on resolvers.
    pub sources_time: Duration,
    /// Time spent producing HTML once the data was available.
    pub render_time: Duration,
}

pub struct BuildReport {
    /// Paths written, relative to the build directory.
    pub files: Vec<String>,
}

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type RenderFn<S> = for<'a> fn(&'a RequestContext<S>, Option<&'a str>) -> BoxFut<'a, Result<Page, Error>>;
pub type SourceApiFn<S> = for<'a> fn(
    &'a str,
    &'a RequestContext<S>,
    &'a [(&'a str, &'a str)],
) -> Option<BoxFut<'a, Result<serde_json::Value, Error>>>;
```

[Generated code](/docs/generated-code/) shows a real manifest.

## document

```rust
pub fn document(title: &str, css: &[&str], body: &str, payload: Option<&str>, js: Option<&str>) -> String
```

Wraps a rendered body in the HTML document: doctype, `lang="en"`, charset and viewport meta tags, the escaped `<title>`, a stylesheet link and a `modulepreload` link when present, then the body, the [handoff payload](/docs/handoff-payload/) in `<script type="application/json" id="rk-payload">`, and the module `<script>`. Generated code calls it; you rarely need to. There is no way to customise the document shell yet. A layout wraps the body; see [Pages and routes](/docs/pages-and-routes/#layouts).

## Sessions and secrets

```rust
#[derive(Clone)]
pub struct SessionStore { /* map, optionally backed by a file */ }

impl Default for SessionStore { /* 12 hour idle timeout, in memory */ }

impl SessionStore {
    pub fn new(idle_timeout: Duration) -> Self;
    pub fn persistent(path: impl Into<PathBuf>) -> Self;
    pub fn load(&self, id: &str) -> Option<SessionData>;
    pub fn create(&self, user_id: Option<String>) -> (String, SessionData);
    pub fn destroy(&self, id: &str);
}

pub fn random_token() -> String;
pub fn constant_time_eq(a: &str, b: &str) -> bool;
```

`SessionStore` is the map `App` uses. `load` refreshes the idle timer and drops a session that has expired. `new` keeps the map in memory. `persistent` loads a JSON file and writes it back when a session is created, used, or destroyed, so a new process that opens the same path keeps the session. Pass it with `App::session_store`. `persistent` uses the 12-hour timeout. One process only: do not point two processes at the file. See [Sessions](/docs/sessions/).

`random_token` returns 256 bits from the operating system's random source as 64 hex characters. Session ids and CSRF tokens use it. `constant_time_eq` compares two strings without stopping at the first difference (it does return early when the lengths differ). Use it whenever you compare a secret.

## The js module

`rustkit::js` holds the JavaScript value semantics the server renderer uses. Generated code calls it for every interpolation, attribute and condition. [JS value semantics](/docs/js-value-semantics/) explains the rules in depth.

| Item | Signature | Purpose |
|---|---|---|
| `Val<'a>` | `enum { Undefined, Null, Bool(bool), Num(f64), Str(Cow<'a, str>) }` | A JavaScript primitive. |
| `Val::str` | `fn str(s: &'a str) -> Val<'a>` | Borrowed string value. |
| `Val::from_len` | `fn from_len(n: usize) -> Val<'static>` | A length as a number. |
| `Val::truthy` | `fn truthy(&self) -> bool` | JavaScript `Boolean(v)`. |
| `Val::to_number` | `fn to_number(&self) -> f64` | JavaScript `Number(v)` for primitives. |
| `Val::to_text` | `fn to_text(&self) -> Cow<'_, str>` | Interpolated text; `null` and `undefined` are empty. |
| `ToJs` | `trait ToJs { fn to_js(&self) -> Val<'_>; }` | Implemented for `str`, `String`, `bool`, all integer and float types, `Option<T>`, `&T`, `Box<T>` and `Val`. |
| `JsLength` | `trait JsLength { fn js_length(&self) -> usize; }` | `str` and `String` count UTF-16 units; `Vec<T>` and `[T]` count elements. |
| `length` | `fn length<T: JsLength + ?Sized>(v: &T) -> Val<'static>` | `.length`. |
| `strict_eq` | `fn strict_eq(a: &Val, b: &Val) -> bool` | `===`. |
| `lt`, `le`, `gt`, `ge` | `fn lt(a: &Val, b: &Val) -> bool` | `<`, `<=`, `>`, `>=`. |
| `number_to_string` | `fn number_to_string(n: f64) -> String` | `Number.prototype.toString()`. |
| `escape_into` | `fn escape_into(out: &mut String, s: &str)` | Escapes `& < > " '`. |
| `escape` | `fn escape(s: &str) -> String` | Same, returning a new string. |
| `text` | `fn text(out: &mut String, v: &Val)` | Writes an interpolation. |
| `attr` | `fn attr(out: &mut String, name: &str, v: &Val)` | Writes a bound attribute. |
| `html_safe_json` | `fn html_safe_json(v: &serde_json::Value) -> String` | JSON that cannot close a `<script>` element. |

The crate also has a hidden `__private` module that re-exports `serde_json` and `tokio::try_join` for generated code. It is not part of the API.

---
title: Testing your app
description: Write acceptance tests that drive the real router in memory, sign in with a helper, extract CSRF tokens, and assert redirects, per-user data and cache headers.
order: 8
level: Intermediate
duration: 30 min
---

## What you'll build

A test file for the `members` app from [Signing in](/tutorials/sign-in/). The tests send real HTTP requests to the real router, in memory, with no port and no browser. They check what you verified by hand in that tutorial:

- the home page is prerendered, publicly cacheable and ships no JavaScript;
- signed-out visitors are redirected to `/login?next=/notes`;
- Alice and Bob each see only their own notes, with one upstream read per page;
- personal pages are `private, no-store` and carry `Server-Timing`;
- forged and token-less posts are refused;
- signing out ends the session.

## Prerequisites

- The `members` project from [Signing in](/tutorials/sign-in/). If you've also done [Measuring performance](/tutorials/measuring-performance/), that's fine: these tests pass on both versions.

## Why test through the router

`App::into_router()` returns an `axum::Router`, and a router is a `tower::Service`. That means a test can hand it a `Request` and await a `Response` directly. Everything between the two is the real thing: routing, session lookup, CSRF checks, your resolvers, the generated render functions and the response headers. Only the TCP socket is missing.

These tests run in milliseconds, need no free port, and can't be fooled by a cached response in a browser.

## Step 1: Add test dependencies

Add a `[dev-dependencies]` section to `Cargo.toml`:

```toml title=Cargo.toml
[package]
name = "members"
version = "0.1.0"
edition = "2024"

[dependencies]
rustkit = { git = "https://github.com/nicholas-johnson/rust-kit" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

- `tower` with the `util` feature provides `ServiceExt::oneshot`, which sends one request to a service.
- `http-body-util` provides `BodyExt::collect`, which reads a response body into bytes.

## Step 2: Write the helpers

Create `tests/app.rs`. Start with a few helpers. They make each test read like a short script:

```rust title=tests/app.rs
//! Acceptance tests: real router, real sessions, no network.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

// ---------------------------------------------------------------- helpers

struct Res {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Res {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map_or("", |v| v.to_str().unwrap())
    }

    /// The `name=value` part of the response's `Set-Cookie`, if any.
    fn cookie(&self) -> Option<String> {
        let c = self.headers.get(header::SET_COOKIE)?.to_str().unwrap();
        Some(c.split(';').next().unwrap().to_owned())
    }

    /// The CSRF token Rust-kit put in the page's first POST form.
    fn csrf(&self) -> String {
        let marker = "name=\"_csrf\" value=\"";
        let start = self.body.find(marker).expect("page has a CSRF field") + marker.len();
        let len = self.body[start..].find('"').unwrap();
        self.body[start..start + len].to_owned()
    }
}

async fn send(router: &Router, req: Request<Body>) -> Res {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Res { status, headers, body: String::from_utf8(bytes.to_vec()).unwrap() }
}

fn get(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(path).header(header::HOST, "localhost");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::empty()).unwrap()
}

fn post(path: &str, cookie: Option<&str>, form: &str) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::HOST, "localhost")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(form.to_owned())).unwrap()
}

async fn setup() -> (Router, Arc<members::AppState>) {
    let app = members::app(Duration::ZERO, false);
    let state = app.state();
    (app.into_router().await.unwrap(), state)
}

/// Signs in the way a browser does and returns the new session cookie.
async fn sign_in(router: &Router, user: &str) -> String {
    let page = send(router, get("/login", None)).await;
    let anon = page.cookie().expect("the login form starts a session for its CSRF token");
    let form = format!("user={user}&_csrf={}", page.csrf());
    let res = send(router, post("/login", Some(&anon), &form)).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.header("location"), "/notes");
    let cookie = res.cookie().expect("sign-in sets a new session cookie");
    assert_ne!(cookie, anon, "the session id rotates at sign-in");
    cookie
}
```

What each helper is for:

- **`send`** clones the router (cheap: it's reference-counted inside), sends one request with `oneshot`, and collects the status, headers and body into a plain struct.
- **`get` and `post`** build requests the way a browser would. `post` sets `Origin` to match `Host`, because Rust-kit refuses a form post whose `Origin` names a different host. Tests that forget this get a `403` and a confusing afternoon.
- **`csrf`** pulls the token out of the hidden `_csrf` input Rust-kit adds to every POST form. Reading it from the HTML, rather than asking the framework, proves the token really is in the page.
- **`setup`** builds the app with zero latency and insecure cookies, and keeps an `Arc` of the state via `app.state()` so tests can read the service's counters after the app has been turned into a router.
- **`sign_in`** does exactly what a person does: loads the login page (which starts an anonymous session), posts the form with that session's token, and returns the new cookie. It also asserts that the session id changed.

## Step 3: Write the tests

Append these tests to `tests/app.rs`:

```rust title=tests/app.rs
// ------------------------------------------------------------------ tests

#[tokio::test]
async fn home_is_prerendered_and_publicly_cacheable() {
    let app = members::app(Duration::ZERO, false);
    let home = app.routes().iter().find(|r| r.path == "/").unwrap();
    assert!(home.prerender);
    assert_eq!(home.js_bytes, 0);

    let (router, _) = setup().await;
    let res = send(&router, get("/", None)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("cache-control"), "public, max-age=300");
    assert!(res.cookie().is_none());
    assert!(!res.body.contains("<script"));
}

#[tokio::test]
async fn anonymous_visitors_are_sent_to_sign_in() {
    let (router, _) = setup().await;
    let res = send(&router, get("/notes", None)).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.header("location"), "/login?next=/notes");
}

#[tokio::test]
async fn each_user_sees_only_their_own_notes() {
    let (router, state) = setup().await;
    let alice = sign_in(&router, "alice").await;
    let bob = sign_in(&router, "bob").await;
    let before = state.accounts.reads.load(Ordering::SeqCst);

    let a = send(&router, get("/notes", Some(&alice))).await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("<h1>Alice's notes</h1>"));
    assert!(a.body.contains("<li>Buy more tea</li>"));
    assert!(!a.body.contains("Tune the guitar"));

    let b = send(&router, get("/notes", Some(&bob))).await;
    assert!(b.body.contains("<li>Tune the guitar</li>"));
    assert!(!b.body.contains("Buy more tea"));

    // One upstream read per page view.
    assert_eq!(state.accounts.reads.load(Ordering::SeqCst) - before, 2);
}

#[tokio::test]
async fn personalised_pages_stay_out_of_shared_caches() {
    let (router, _) = setup().await;
    let alice = sign_in(&router, "alice").await;
    let res = send(&router, get("/notes", Some(&alice))).await;
    assert_eq!(res.header("cache-control"), "private, no-store");
    assert_eq!(res.header("vary"), "Cookie");
    assert!(res.header("server-timing").starts_with("sources;dur="));
}

#[tokio::test]
async fn posts_without_a_valid_token_or_origin_are_refused() {
    let (router, _) = setup().await;
    let page = send(&router, get("/login", None)).await;
    let anon = page.cookie().unwrap();

    let missing = send(&router, post("/login", Some(&anon), "user=alice")).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);

    let wrong = send(&router, post("/login", Some(&anon), "user=alice&_csrf=0000")).await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);

    let mut forged = post("/login", Some(&anon), &format!("user=alice&_csrf={}", page.csrf()));
    forged.headers_mut().insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    assert_eq!(send(&router, forged).await.status, StatusCode::FORBIDDEN);

    let unknown = send(&router, post("/login", Some(&anon), &format!("user=mallory&_csrf={}", page.csrf()))).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn signing_out_ends_the_session() {
    let (router, _) = setup().await;
    let alice = sign_in(&router, "alice").await;
    let token = send(&router, get("/notes", Some(&alice))).await.csrf();

    let res = send(&router, post("/logout", Some(&alice), &format!("_csrf={token}"))).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.header("location"), "/");
    assert_eq!(res.cookie().as_deref(), Some("rk_session="));

    // The old cookie no longer works.
    let after = send(&router, get("/notes", Some(&alice))).await;
    assert_eq!(after.status, StatusCode::SEE_OTHER);
}
```

## Step 4: Run them

```bash
cargo test
```

The first run compiles `tower` and `http-body-util`. Then you should see:

```text
running 6 tests
test anonymous_visitors_are_sent_to_sign_in ... ok
test home_is_prerendered_and_publicly_cacheable ... ok
test posts_without_a_valid_token_or_origin_are_refused ... ok
test personalised_pages_stay_out_of_shared_caches ... ok
test signing_out_ends_the_session ... ok
test each_user_sees_only_their_own_notes ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The order varies from run to run, because tests run in parallel. Each test calls `setup()`, so each has its own app, its own session store and its own counters. They can't interfere with one another.

Now make one fail on purpose, to check the tests are really looking. In `src/sources.rs`, change the resolver to load a fixed account:

```rust
let account = ctx.accounts.get("alice").await?;
```

Run `cargo test` again. `each_user_sees_only_their_own_notes` fails, because Bob's page now shows Alice's notes. That's exactly the kind of bug these tests exist to catch. Put the line back.

## What the tests prove

Walking through them one at a time:

- **`home_is_prerendered_and_publicly_cacheable`** reads the manifest through `app.routes()` before building the router. `prerender` and `js_bytes` come straight from the compiler, so you're testing what was built, not what you hope was built. A prerendered page that set a cookie would be a bug, because the same HTML is served to everyone.
- **`anonymous_visitors_are_sent_to_sign_in`** checks the exact `Location`. A redirect to the wrong place is still a bug.
- **`each_user_sees_only_their_own_notes`** signs two users into one app and checks both directions. It also counts upstream calls: rendering a page should read the account once, not once per field.
- **`personalised_pages_stay_out_of_shared_caches`** protects against the worst caching bug there is: a CDN serving one person's page to another. `private, no-store` and `Vary: Cookie` are what stop that.
- **`posts_without_a_valid_token_or_origin_are_refused`** covers each layer of CSRF protection separately: missing token, wrong token, foreign origin. The last case is your own validation in `login`.
- **`signing_out_ends_the_session`** checks the response clears the cookie and that the server forgot the session. The old cookie must stop working even if a browser kept it.

> [!TIP]
> Test what the HTML says, not how it's built. Assertions like `contains("<li>Buy more tea</li>")` survive refactors of the template, and they fail when a visitor would notice something wrong.

## Testing interactive pages

These tests cover the server half of an app. They can check that a page ships a module (`body.contains("<script type=\"module\"")`), fetch that module by its URL like any other asset, and read the handoff payload out of `<script id="rk-payload">`. They can't click a button. For the browser half, point a browser automation tool such as Playwright at `cargo run`. The [testing guide](/docs/testing/) goes into more depth.

## Next steps

- [Measuring performance](/tutorials/measuring-performance/) adds a second source to the notes page and measures where request time goes. Run these tests afterwards: they still pass.
- [Testing](/docs/testing/) is the full guide, including client sources and the source endpoint.
- [Runtime API](/docs/runtime-api/) documents `App::routes`, `App::state` and `into_router`.

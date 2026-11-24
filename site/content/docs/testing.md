---
title: Testing
description: Test your pages, sources and actions against the real router in-process, and test what a page compiles to.
section: Guides
order: 1
---

A Rust-kit app is an axum `Router`, so you can test it the way you test any axum service: build the router, send it requests with `tower::ServiceExt::oneshot`, and inspect the responses. Nothing listens on a port and no browser is involved. The account demo's acceptance tests in `examples/account-demo/tests/acceptance.rs` work this way, and this guide follows them.

There are two kinds of test worth writing:

- **HTTP tests** check what your app does: the HTML a signed-in user sees, how many upstream calls a page makes, which requests are refused.
- **Compiler tests** check what a page compiles to: whether it ships JavaScript, which sources load where, which fields cross into the browser.

Browser behaviour, such as a button re-rendering a block, has no test harness yet. Check it by hand in a browser.

## Set up

Add the test-only dependencies:

```toml title=Cargo.toml
[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
serde_json = "1"
```

Your crate already depends on `axum`, `tokio` and `rustkit`. Integration tests in `tests/` can only see your library's public items, so expose a function that builds the app, as the demo's `lib.rs` does. Take anything a test needs to vary as a parameter:

```rust title=src/lib.rs
pub fn app(upstream_latency: Duration, secure_cookies: bool) -> rustkit::App<AppState> {
    let state = AppState { accounts_service: upstream::AccountsService::demo(upstream_latency) };
    rustkit::App::new(generated::manifest(), state)
        .secure_cookies(secure_cookies)
        .action("/account/profile", actions::update_profile)
        .action("/login", actions::login)
        .action("/logout", actions::logout)
}
```

Tests pass `Duration::ZERO` so they do not sleep. The `Secure` flag makes no difference to these tests, because they copy cookies between requests by hand.

## Request helpers

Write a few small helpers once. `send` runs one request through a clone of the router and collects the body:

```rust title=tests/app.rs
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

struct Res {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn send(router: &Router, req: Request<Body>) -> Res {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Res { status, headers, body: String::from_utf8(bytes.to_vec()).unwrap() }
}
```

`GET` and `POST` builders set the headers the framework checks. Actions reject a request whose `Origin` does not match its `Host`, so a form post sets both:

```rust
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
```

### Cookies and CSRF tokens

The session cookie is `rk_session=<64 hex characters>`. Keep the `name=value` part of `Set-Cookie` and send it back in `Cookie`. Every `<form method="post">` contains a hidden `_csrf` field whose value is 64 hex characters, so you can read it from the HTML:

```rust
impl Res {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map_or("", |v| v.to_str().unwrap())
    }
    fn cookie(&self) -> Option<String> {
        let c = self.headers.get(header::SET_COOKIE)?.to_str().unwrap();
        Some(c.split(';').next().unwrap().to_owned())
    }
    fn csrf(&self) -> String {
        let marker = "name=\"_csrf\" value=\"";
        let start = self.body.find(marker).expect("page has a CSRF field") + marker.len();
        self.body[start..start + 64].to_owned()
    }
}
```

### Signing in

Sign in the way a browser does: load the sign-in page, which starts an anonymous session for its CSRF token, then post the form with that cookie and token. The response carries a new cookie, because `ctx.login` rotates the session id.

```rust
async fn sign_in(router: &Router, user: &str) -> String {
    let login = send(router, get("/login", None)).await;
    let anon = login.cookie().expect("login page starts a session");
    let body = format!("user={user}&_csrf={}", login.csrf());
    let res = send(router, post("/login", Some(&anon), &body)).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let cookie = res.cookie().expect("sign-in issues a new cookie");
    assert_ne!(cookie, anon, "session id rotates at sign-in");
    cookie
}
```

You cannot construct a `RequestContext` yourself, so there is no way to call a resolver directly with a fake user. Test resolvers through the pages and endpoints that use them.

## Test a page

```rust
#[tokio::test]
async fn greeting_is_in_raw_html() {
    let router = my_app::app(Duration::ZERO, false).into_router().await.unwrap();
    let cookie = sign_in(&router, "alice").await;

    let res = send(&router, get("/account", Some(&cookie))).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(res.body.contains("<h1>Hello, Alice Liddell</h1>"));
    assert_eq!(res.header("cache-control"), "private, no-store");
    assert_eq!(res.header("vary"), "Cookie");
    assert!(res.header("server-timing").starts_with("sources;dur="));
}
```

Asserting on raw HTML is the point: it proves the content is there before any JavaScript runs.

To check what the browser receives, parse the [handoff payload](/docs/handoff-payload/) and assert on its exact shape. This catches a page that starts sending more fields than it needs:

```rust
let payload = res.body
    .split("id=\"rk-payload\">").nth(1).unwrap()
    .split("</script>").next().unwrap();
let json: serde_json::Value = serde_json::from_str(payload).unwrap();
assert_eq!(json["s"]["account"], serde_json::json!({ "displayName": "Alice Liddell" }));
assert!(!res.body.contains("alice@example.com"), "upstream-only fields stay on the server");
```

## Count upstream calls

To prove a page reads each source once, give your service stand-in counters and keep a handle on the state before the router consumes the app. `App::state()` returns an `Arc<S>`:

```rust
let app = my_app::app(Duration::ZERO, false);
let state = app.state();
let router = app.into_router().await.unwrap();
let cookie = sign_in(&router, "alice").await;

let before = state.accounts_service.account_reads.load(Ordering::SeqCst);
send(&router, get("/account", Some(&cookie))).await;
assert_eq!(state.accounts_service.account_reads.load(Ordering::SeqCst) - before, 1);
```

The demo's `upstream.rs` shows the pattern: an `AtomicUsize` per endpoint, incremented in each method. When you swap in a real HTTP client, keep the counting stand-in for tests.

## Test client sources and security

`GET /_rk/source/{name}` exists only for sources some page loads with `load="client"`. Test its access rules directly:

```rust
let anon = send(&router, get("/_rk/source/activity", None)).await;
assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

let cross = Request::get("/_rk/source/activity")
    .header(header::COOKIE, &cookie)
    .header("sec-fetch-site", "cross-site")
    .body(Body::empty())
    .unwrap();
assert_eq!(send(&router, cross).await.status, StatusCode::FORBIDDEN);

// Server-only sources have no endpoint at all.
let api = send(&router, get("/_rk/source/account", Some(&cookie))).await;
assert_eq!(api.status, StatusCode::NOT_FOUND);
```

And test that actions refuse forged posts:

```rust
let token = send(&router, get("/account", Some(&cookie))).await.csrf();

let missing = send(&router, post("/account/profile", Some(&cookie), "name=Mallory")).await;
assert_eq!(missing.status, StatusCode::BAD_REQUEST);

let mut forged = post("/account/profile", Some(&cookie), &format!("name=Mallory&_csrf={token}"));
forged.headers_mut().insert(header::ORIGIN, "https://evil.example".parse().unwrap());
assert_eq!(send(&router, forged).await.status, StatusCode::FORBIDDEN);
```

## Test prerendered pages

`App::build` writes prerendered routes to a directory. Use a temporary one:

```rust
#[tokio::test]
async fn article_is_static() {
    let app = my_app::app(Duration::ZERO, true);
    let article = app.routes().iter().find(|r| r.path == "/article").unwrap();
    assert!(article.prerender);
    assert_eq!(article.js_bytes, 0);

    let dir = std::env::temp_dir().join(format!("my-app-build-{}", std::process::id()));
    let report = app.build(&dir).await.unwrap();
    assert!(report.files.contains(&"article/index.html".to_owned()));
    let html = std::fs::read_to_string(dir.join("article/index.html")).unwrap();
    assert!(!html.contains("<script"));
    std::fs::remove_dir_all(&dir).ok();
}
```

Asserting `js_bytes == 0` is a cheap guard against a static page quietly gaining a browser module.

## Testing what a page compiles to

The compiler's [`compile_page`](/docs/compiler-api/#compile-page) compiles one page from a string, without touching the file system. Add the compiler as a dev-dependency too:

```toml
[dev-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

Then assert on the output, as the compiler's own tests in `crates/rustkit-compiler/src/tests.rs` do:

```rust
use rustkit_compiler::{Options, compile_page};

const OPTS: Options = Options { state_type: "crate::AppState", sources_module: "crate::sources" };

#[test]
fn account_page_sends_only_the_display_name() {
    let src = include_str!("../src/routes/account.rk");
    let page = compile_page(src, &OPTS)
        .unwrap_or_else(|d| panic!("{}", d.render("account.rk", src)));
    assert_eq!(page.server_sources, ["account"]);
    assert_eq!(page.client_sources, ["activity"]);
    assert!(page.rust.contains("m.insert(\"displayName\".into()"));
    assert!(!page.js.unwrap().contains("/_rk/source/account"));
}

#[test]
fn formatter_call_is_rejected() {
    let src = "<script>import { account } from \"$sources\";</script>\n\
               <DataSource source={account} load=\"server\" let:data>{format(data.joined)}</DataSource>";
    let d = compile_page(src, &OPTS).err().expect("must fail");
    assert!(d.msg.contains("cannot be rendered by the Rust server renderer"));
}
```

Rendering the `Diag` in the panic message gives you the same rustc-style error as a real build. String assertions on generated code are brittle across compiler versions, so keep them for properties you care about, such as which fields reach the payload.

## Run the tests

```bash
cargo test
```

Because the tests use `Duration::ZERO`, they never wait on the imitated network. Run them on every change; the build step compiles your pages first, so a template error fails `cargo test` before any test runs.

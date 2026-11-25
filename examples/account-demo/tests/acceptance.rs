//! The four acceptance properties from the design proposal, run against the
//! real router (no network, no browser). Browser behaviour is checked
//! separately; see the README.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

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

async fn send(router: &Router, req: Request<Body>) -> Res {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    Res { status, headers, body }
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

/// Builds the app and keeps a handle on the upstream call counters.
async fn setup() -> (Router, Arc<account_demo::AppState>) {
    let app = account_demo::app(Duration::ZERO, false);
    let state = app.state();
    (app.into_router().await.unwrap(), state)
}

async fn sign_in(router: &Router, user: &str) -> String {
    let login = send(router, get("/login", None)).await;
    let anon = login.cookie().expect("login page starts a session for its CSRF token");
    let res = send(router, post("/login", Some(&anon), &format!("user={user}&_csrf={}", login.csrf()))).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    let cookie = res.cookie().expect("sign-in issues a new session cookie");
    assert_ne!(cookie, anon, "session id rotates at sign-in");
    cookie
}

// 1. The article builds to a static file and ships zero JavaScript when it has
//    no interactions.
#[tokio::test]
async fn article_builds_to_static_html_without_javascript() {
    let app = account_demo::app(Duration::ZERO, true);
    let article = app.routes().iter().find(|r| r.path == "/article").unwrap();
    assert!(article.prerender);
    assert_eq!(article.js_bytes, 0);

    let dir = std::env::temp_dir().join(format!("rk-build-{}", std::process::id()));
    let report = app.build(&dir).await.unwrap();
    assert!(report.files.contains(&"article/index.html".to_owned()));
    let html = std::fs::read_to_string(dir.join("article/index.html")).unwrap();
    assert!(strip_scope(&html).contains("<h1>Why render on the server?</h1>"));
    assert!(!html.contains("<script"), "static article must not load scripts");
    assert!(html.contains("rel=\"stylesheet\""));
    std::fs::remove_dir_all(&dir).ok();

    let (router, _) = setup().await;
    let res = send(&router, get("/article", None)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.header("cache-control"), "public, max-age=300");
    assert!(!res.body.contains("<script"));
}

// 2. An authenticated request returns the correct greeting in raw HTML.
//    Connecting the browser handlers causes no second account read.
#[tokio::test]
async fn account_greeting_is_in_raw_html_and_read_once() {
    let (router, state) = setup().await;
    let cookie = sign_in(&router, "alice").await;
    let svc = &state.accounts_service;
    let before = svc.account_reads.load(Ordering::SeqCst);

    let res = send(&router, get("/account", Some(&cookie))).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(strip_scope(&res.body).contains("<h1>Hello, Alice Liddell</h1>"), "{}", res.body);
    assert_eq!(svc.account_reads.load(Ordering::SeqCst) - before, 1, "one upstream read per page");

    // The editor's initial value travels in the page; only the field it needs.
    let payload = res.body.split("id=\"rk-payload\">").nth(1).unwrap().split("</script>").next().unwrap();
    let json: serde_json::Value = serde_json::from_str(payload).unwrap();
    assert_eq!(json["s"]["account"], serde_json::json!({ "displayName": "Alice Liddell" }));
    assert!(!res.body.contains("alice@example.com"), "upstream-only fields stay on the server");

    // The browser module has no way to read the account again: the source is
    // server-loaded only, so it has no endpoint and the module never names it.
    let js_url = res.body.split("<script type=\"module\" src=\"").nth(1).unwrap().split('"').next().unwrap();
    let js = send(&router, get(js_url, None)).await.body;
    assert!(!js.contains("_rk/source/account"));
    let api = send(&router, get("/_rk/source/account", Some(&cookie))).await;
    assert_eq!(api.status, StatusCode::NOT_FOUND);
    assert_eq!(svc.account_reads.load(Ordering::SeqCst) - before, 1);

    // Personalised responses stay out of shared caches.
    assert_eq!(res.header("cache-control"), "private, no-store");
    assert_eq!(res.header("vary"), "Cookie");
    assert!(res.header("server-timing").starts_with("sources;dur="));
}

#[tokio::test]
async fn signed_out_visitors_are_sent_to_sign_in() {
    let (router, _) = setup().await;
    let res = send(&router, get("/account", None)).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.header("location"), "/login?next=/account");
}

// 3. Opening activity issues one same-origin request. Two signed-in users
//    receive only their own data through both the HTML and API paths.
#[tokio::test]
async fn users_only_see_their_own_data() {
    let (router, state) = setup().await;
    let alice = sign_in(&router, "alice").await;
    let bob = sign_in(&router, "bob").await;

    let a_html = send(&router, get("/account", Some(&alice))).await.body;
    let b_html = send(&router, get("/account", Some(&bob))).await.body;
    assert!(a_html.contains("Hello, Alice Liddell") && !a_html.contains("Bob"));
    assert!(b_html.contains("Hello, Bob Marley") && !b_html.contains("Alice"));

    let before = state.accounts_service.activity_reads.load(Ordering::SeqCst);
    let a_api = send(&router, get("/_rk/source/activity", Some(&alice))).await;
    let b_api = send(&router, get("/_rk/source/activity", Some(&bob))).await;
    assert_eq!(state.accounts_service.activity_reads.load(Ordering::SeqCst) - before, 2);
    assert_eq!(a_api.status, StatusCode::OK);
    assert_eq!(a_api.header("cache-control"), "private, no-store");
    let a: serde_json::Value = serde_json::from_str(&a_api.body).unwrap();
    let b: serde_json::Value = serde_json::from_str(&b_api.body).unwrap();
    assert_eq!(a["items"].as_array().unwrap().len(), 3);
    assert_eq!(b["items"].as_array().unwrap().len(), 2);
    assert!(!a_api.body.contains("payment method") && !b_api.body.contains("invoice"));
    assert!(!a_api.body.contains("accountId"), "only declared view fields are serialised");

    // No session: the API applies the same check as the page.
    let anon = send(&router, get("/_rk/source/activity", None)).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
    // A cross-site request is refused even with a cookie.
    let cross = Request::get("/_rk/source/activity")
        .header(header::COOKIE, &alice)
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&router, cross).await.status, StatusCode::FORBIDDEN);
}

// 4. Saving a profile works through an ordinary form action and the next page
//    response reflects the upstream change.
#[tokio::test]
async fn saving_the_profile_updates_upstream_and_the_next_page() {
    let (router, state) = setup().await;
    let cookie = sign_in(&router, "alice").await;
    let page = send(&router, get("/account", Some(&cookie))).await;
    let token = page.csrf();

    let res = send(&router, post("/account/profile", Some(&cookie), &format!("name=Alice+L.&_csrf={token}"))).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(res.header("location"), "/account");

    let next = send(&router, get("/account", Some(&cookie))).await;
    assert!(strip_scope(&next.body).contains("<h1>Hello, Alice L.</h1>"));
    let rec = state.accounts_service.get("alice").await.unwrap();
    assert_eq!(rec.display_name, "Alice L.");
}

#[tokio::test]
async fn profile_action_rejects_forged_and_invalid_posts() {
    let (router, state) = setup().await;
    let cookie = sign_in(&router, "alice").await;
    let token = send(&router, get("/account", Some(&cookie))).await.csrf();

    let missing = send(&router, post("/account/profile", Some(&cookie), "name=Mallory")).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);

    let mut forged = post("/account/profile", Some(&cookie), &format!("name=Mallory&_csrf={token}"));
    forged.headers_mut().insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    assert_eq!(send(&router, forged).await.status, StatusCode::FORBIDDEN);

    let empty = send(&router, post("/account/profile", Some(&cookie), &format!("name=++&_csrf={token}"))).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);

    assert_eq!(state.accounts_service.get("alice").await.unwrap().display_name, "Alice Liddell");
}

#[tokio::test]
async fn user_text_is_escaped_in_html_and_payload() {
    let (router, _) = setup().await;
    let cookie = sign_in(&router, "alice").await;
    let token = send(&router, get("/account", Some(&cookie))).await.csrf();
    let evil = "%3C%2Fscript%3E%3Cimg+src%3Dx+onerror%3Dalert(1)%3E";
    send(&router, post("/account/profile", Some(&cookie), &format!("name={evil}&_csrf={token}"))).await;

    let body = send(&router, get("/account", Some(&cookie))).await.body;
    assert!(body.contains("Hello, &lt;/script&gt;&lt;img src=x onerror=alert(1)&gt;"));
    assert!(!body.contains("<img"));
    assert_eq!(body.matches("</script>").count(), 2, "only the payload and module tags close");
}

/// Drops ` data-rk="…"` so an assertion about a tag can ignore scoping.
fn strip_scope(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let needle = " data-rk=\"";
    while let Some(at) = rest.find(needle) {
        out.push_str(&rest[..at]);
        let value = at + needle.len();
        let end = value + rest[value..].find('"').unwrap() + 1;
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

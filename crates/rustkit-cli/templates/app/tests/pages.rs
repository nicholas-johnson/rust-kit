//! What the server sends. Run with `cargo test`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(path: &str) -> (StatusCode, String) {
    let router = {{crate}}::app(false).into_router().await.unwrap();
    let res = router.oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

fn without_comments(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start..].split_once("-->").map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

#[tokio::test]
async fn every_route_renders() {
    for r in {{crate}}::app(false).routes() {
        let (status, body) = get(r.path).await;
        assert_eq!(status, StatusCode::OK, "{}", r.path);
        assert!(body.starts_with("<!doctype html>"), "{}", r.path);
    }
}

#[tokio::test]
async fn the_home_page_ships_no_javascript() {
    let (_, body) = get("/").await;
    assert!(!body.contains("<script"));
}

#[tokio::test]
async fn the_counter_renders_its_first_frame_on_the_server() {
    let (_, body) = get("/counter").await;
    // Interactive parts sit between `<!--rk:…-->` markers the browser module uses.
    let text = without_comments(&body);
    assert!(text.contains(r#"<output id="count">0</output>"#), "{body}");
    assert!(body.contains("<script"), "the counter loads its page module");
}

#[tokio::test]
async fn hello_renders_resolver_data() {
    let (_, body) = get("/hello").await;
    assert!(body.contains("Hello from a Rust resolver."), "{body}");
}

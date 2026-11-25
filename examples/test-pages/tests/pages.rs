//! What the server sends for each small page.

mod common;

use axum::http::StatusCode;
use common::*;

#[tokio::test]
async fn every_route_renders() {
    let app = test_pages::app(false);
    let paths: Vec<&str> = app.routes().iter().map(|r| r.path).collect();
    assert_eq!(paths.len(), 26, "{paths:?}");
    let router = app.into_router().await.unwrap();
    for path in paths {
        let url = located(path);
        let res = send(&router, get(&url, None)).await;
        assert_eq!(res.status, StatusCode::OK, "{path} ({url})");
        assert_eq!(res.header("content-type"), "text/html; charset=utf-8", "{path}");
        assert!(res.body.starts_with("<!doctype html>"), "{path}");
        assert!(res.body.contains("<title>"), "{path}");
    }
}

#[tokio::test]
async fn pages_without_state_ship_no_javascript() {
    let app = test_pages::app(false);
    for r in app.routes() {
        let interactive = !matches!(r.path, "/" | "/static" | "/title" | "/list-server" | "/blog" | "/blog/{slug}/gated" | "/components");
        assert_eq!(r.js_bytes > 0, interactive, "{}: js_bytes = {}", r.path, r.js_bytes);
    }
    let router = app.into_router().await.unwrap();
    for path in ["/", "/static", "/title", "/list-server", "/blog", "/blog/hello/gated", "/components"] {
        let res = send(&router, get(path, None)).await;
        assert!(!res.body.contains("<script"), "{path} must not load scripts");
    }
    for path in ["/", "/static", "/blog"] {
        let res = send(&router, get(path, None)).await;
        assert_eq!(res.header("cache-control"), "public, max-age=300", "{path} is prerendered");
    }
}

#[tokio::test]
async fn title_is_hoisted_into_the_head() {
    let res = send(&router().await, get("/title", None)).await;
    assert!(res.body.contains("<title>Hoisted title</title>"));
    assert_eq!(res.body.matches("<title>").count(), 1);
}

#[tokio::test]
async fn text_is_escaped_and_numbers_format_like_javascript() {
    let res = send(&router().await, get("/text", None)).await;
    let cases = [
        ("markup", "&lt;script&gt;alert(&quot;hi&quot;) &amp; &#39;bye&#39;&lt;/script&gt;"),
        ("quotes", "She said &quot;yes&quot; &amp; he said &#39;no&#39;"),
        ("emoji", "café 🦀"),
        ("emoji-length", "7"),
        ("empty", "[]"),
        ("sum", "0.30000000000000004"),
        ("big", "1e+21"),
        ("tiny", "1.5e-7"),
        ("small", "0.000001"),
        ("neg-zero", "0"),
        ("half", "0.5"),
        ("count", "123456789"),
        ("negative", "-42"),
        ("bools", "true false"),
        ("nothing", "[]"),
        ("something", "here"),
        ("literals", "x &lt; y &amp; z 42 1.25 true []"),
    ];
    for (id, want) in cases {
        assert_eq!(res.inner(id), want, "#{id}");
    }
    assert!(!res.body.contains("<script>alert"), "raw markup must never reach the page");
}

#[tokio::test]
async fn floats_render_as_the_browser_will_receive_them() {
    let res = send(&router().await, get("/nan", None)).await;
    assert_eq!(res.inner("nan"), "[]", "NaN travels as null");
    assert_eq!(res.inner("inf"), "[]", "Infinity travels as null");
    assert_eq!(res.inner("nan-falsy"), "F");
    assert_eq!(res.inner("inf-null"), "true");
    let payload = res.payload().unwrap();
    assert_eq!(payload["s"]["samples"]["notANumber"], serde_json::Value::Null);
    let tenth = payload["s"]["samples"]["tenth"].to_string();
    assert_eq!(res.inner("tenth"), tenth, "an f32 renders as the f64 the payload carries");
}

#[tokio::test]
async fn attributes_follow_the_boolean_and_escaping_rules() {
    let res = send(&router().await, get("/attributes", None)).await;
    assert_eq!(res.tag("a-quoted"), r#"<input id="a-quoted" value="She said &quot;yes&quot; &amp; he said &#39;no&#39;">"#);
    assert_eq!(res.tag("a-true"), r#"<input id="a-true" disabled>"#);
    assert_eq!(res.tag("a-false"), r#"<input id="a-false">"#);
    assert_eq!(res.tag("a-null"), r#"<input id="a-null">"#);
    assert_eq!(res.tag("a-some"), r#"<input id="a-some" title="here">"#);
    assert_eq!(res.tag("a-mixed"), r#"<p id="a-mixed" class="tag a tag-123456789">"#);
    assert_eq!(res.tag("a-empty"), r#"<p id="a-empty" data-value="">"#);
    assert_eq!(res.tag("a-number"), r#"<p id="a-number" data-value="0.30000000000000004">"#);
    assert_eq!(res.tag("a-bare"), r#"<input id="a-bare" type="checkbox" checked>"#);
    assert_eq!(res.tag("a-static"), r#"<p id="a-static" title="single &quot;quoted&quot;">"#);
}

#[tokio::test]
async fn truthiness_matches_javascript() {
    let res = send(&router().await, get("/conditionals", None)).await;
    // empty, "0", 0, -0, 123456789, 1.5e-7, true, false, None, Some, None number,
    // !empty, !!count, yes && some, yes && none, no || zero, no || tiny
    let want = "FTFFTTTFFTFTTTFFT";
    let got: String = (0..want.len()).map(|i| res.inner(&format!("t{i}")).chars().last().unwrap()).collect();
    assert_eq!(got, want);
    assert_eq!(res.inner("chain"), "chain: medium");
}

#[tokio::test]
async fn comparisons_match_javascript() {
    let res = send(&router().await, get("/comparisons", None)).await;
    let want = [
        true,  // "Z" < "a"
        true,  // "é" > "z"
        true,  // crab > "z"
        true,  // crab < U+FF61 in UTF-16, though not in UTF-8
        true,  // "10" > 9 compares as numbers
        true,  // "10" < "9" compares as strings
        false, // 0.1 + 0.2 === 0.3
        true,  // count >= 123456789
        true,  // None === null
        true,  // None number === null
        true,  // "" === ""
        false, // 0 === "0"
        true,  // -0 === 0
        true,  // 0.5 <= 0.5
        true,  // true !== false
        true,  // -42 < 0
        true,  // null < 1
        true,  // "abc" === "abc"
    ];
    for (i, w) in want.iter().enumerate() {
        let text = res.inner(&format!("c{i}"));
        assert!(text.ends_with(&format!(": {w}")), "c{i}: {text}");
    }
}

#[tokio::test]
async fn loops_render_items_indexes_and_empty_lists() {
    let res = send(&router().await, get("/loops", None)).await;
    assert_eq!(res.inner("total"), "3 items");
    let items = res.inner("items");
    let a = items.find("0: Wrench &lt;M8&gt;, 12.5 (2 tags), note: Fits &quot;most&quot; nuts").expect("first item");
    let b = items.find("1: Rope &amp; pulley, 0.30000000000000004 (0 tags)").expect("second item");
    let c = items.find("2: Crab 🦀 figurine, 1e+21 (1 tags)").expect("third item, empty note is falsy");
    assert!(a < b && b < c, "items keep their order");
    assert!(items.contains("<li>0=tools</li><li>1=metal</li>"));
    assert!(!items.contains("note: </li>"));
    assert_eq!(res.inner("empty"), "The second list is empty.");
    assert!(!res.body.contains("never shown"));
}

#[tokio::test]
async fn counter_renders_initial_state_and_blocks() {
    let res = send(&router().await, get("/counter", None)).await;
    assert!(res.tag("dec").contains(" disabled"), "count starts at 0");
    assert_eq!(strip_markers(&res.inner("value")), "0");
    assert!(!res.body.contains("id=\"msg\""));
    assert!(res.body.contains("data-rk-on=\"click:"));
    assert!(res.body.matches("<!--rk:").count() >= 3, "dec attribute, value text and message are blocks");
    assert!(res.payload().is_none(), "nothing from the server is needed in the browser");
}

#[tokio::test]
async fn only_fields_the_browser_reads_are_handed_off() {
    let res = send(&router().await, get("/toggle", None)).await;
    assert!(res.body.contains("<h1>Toggle: café 🦀</h1>"));
    assert!(!res.body.contains("id=\"details\""));
    let payload = res.payload().expect("toggle has a payload");
    let fields: Vec<&String> = payload["s"]["samples"].as_object().unwrap().keys().collect();
    assert_eq!(fields, ["count", "quotes"]);
    assert!(!res.body.contains("&lt;script&gt;"), "unused fields such as markup stay on the server");
}

#[tokio::test]
async fn branches_pick_the_initial_state() {
    let r = router().await;
    for (path, want) in [("/branches-0", "<p>zero</p>"), ("/branches-2", "<p>small: 2</p>"), ("/branches-5", "<p>big: 5</p>")] {
        let res = send(&r, get(path, None)).await;
        assert!(strip_markers(&res.inner("out")).contains(want), "{path}");
    }
}

#[tokio::test]
async fn client_loaded_list_starts_as_a_loading_region() {
    let r = router().await;
    let server = send(&r, get("/list-server", None)).await;
    assert_eq!(server.inner("list").matches("<article>").count(), 3);

    let client = send(&r, get("/list-client", None)).await;
    assert!(!client.body.contains("<article>"));
    assert!(client.body.contains("class=\"rk-loading\""));

    let api = send(&r, get("/_rk/source/items_live", None)).await;
    assert_eq!(api.status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&api.body).unwrap();
    assert_eq!(json["items"].as_array().unwrap().len(), 3);
    assert_eq!(json["items"][0]["name"], "Wrench <M8>");

    let server_only = send(&r, get("/_rk/source/items", None)).await;
    assert_eq!(server_only.status, StatusCode::NOT_FOUND, "server-loaded sources get no endpoint");
}

#[tokio::test]
async fn forms_post_with_csrf_and_show_the_result() {
    let r = router().await;
    let page = send(&r, get("/form", None)).await;
    assert_eq!(page.header("cache-control"), "private, no-store");
    let cookie = page.cookie().expect("the CSRF token starts a session");
    let token = page.csrf();
    assert_eq!(page.payload().unwrap()["csrf"], token.as_str(), "the browser-rendered form gets the same token");
    assert_eq!(page.inner("last"), "No message yet.");

    let missing = send(&r, post("/form/echo", Some(&cookie), "message=hi")).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);

    let mut foreign = post("/form/echo", Some(&cookie), &format!("message=hi&_csrf={token}"));
    foreign.headers_mut().insert("origin", "https://elsewhere.example".parse().unwrap());
    assert_eq!(send(&r, foreign).await.status, StatusCode::FORBIDDEN);

    let empty = send(&r, post("/form/echo", Some(&cookie), &format!("message=+&_csrf={token}"))).await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST);

    let ok = send(&r, post("/form/echo", Some(&cookie), &format!("message=%3Cb%3Ehi%3C%2Fb%3E&_csrf={token}"))).await;
    assert_eq!(ok.status, StatusCode::SEE_OTHER);
    assert_eq!(ok.header("location"), "/form");

    let after = send(&r, get("/form", Some(&cookie))).await;
    assert_eq!(after.inner("last"), "Last message: &lt;b&gt;hi&lt;/b&gt;");
}

#[tokio::test]
async fn layouts_nest_and_the_route_parameter_reaches_the_page() {
    let r = router().await;
    let index = send(&r, get("/blog", None)).await;
    assert!(index.body.contains("id=\"site-layout\""));
    assert!(index.body.contains("id=\"blog-layout\""));
    assert_eq!(index.inner("blog-title"), "Blog");
    assert!(!index.body.contains("id=\"post-layout\""), "the [slug] layout wraps only that directory");
    assert_eq!(index.header("cache-control"), "public, max-age=300");

    let home = send(&r, get("/", None)).await;
    assert!(home.body.contains("id=\"site-layout\""));
    assert!(!home.body.contains("id=\"blog-layout\""));

    let notes = send(&r, get("/blog/notes", None)).await;
    assert_eq!(notes.inner("post-layout-slug"), "notes");
    assert!(notes.inner("post").contains("Notes &amp; drafts"));
    assert_eq!(notes.inner("body"), "A &lt;tag&gt; &amp; more");
    assert!(!notes.body.contains("id=\"extra\""), "the closed page hides the extra paragraph");
    assert_eq!(notes.payload().unwrap()["params"]["slug"], "notes");

    let hello = send(&r, get("/blog/hello/open", None)).await;
    assert_eq!(hello.inner("extra"), "hello — Hello");
    assert_eq!(hello.inner("body"), "The first post.");

    let other = send(&r, get("/blog/nope", None)).await;
    assert_eq!(other.inner("body"), "No post named nope.");

    let server_only = send(&r, get("/_rk/source/post", None)).await;
    assert_eq!(server_only.status, StatusCode::NOT_FOUND, "server-loaded sources get no endpoint");

    let missing = send(&r, get("/_rk/source/post_live", None)).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
    assert!(missing.body.contains("missing route parameter `slug`"));

    let live = send(&r, get("/_rk/source/post_live?slug=notes", None)).await;
    assert_eq!(live.status, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&live.body).unwrap();
    assert_eq!(json["title"], "Notes & drafts");
    assert_eq!(json["body"], "A <tag> & more");

    let gated = send(&r, get("/blog/a%20b/gated", None)).await;
    assert_eq!(gated.status, StatusCode::SEE_OTHER);
    assert_eq!(gated.header("location"), "/login?next=/blog/a%20b/gated");
}

#[tokio::test]
async fn components_render_on_the_server() {
    let r = router().await;
    let home = send(&r, get("/", None)).await;
    assert_eq!(home.inner("site-footer"), "Rust-kit test pages", "a layout can import a component");
    assert!(!home.body.contains("<script"), "a static component adds no script");

    let page = send(&r, get("/components", None)).await;
    assert_eq!(page.inner("app-header"), "A &amp; B");
    assert!(page.inner("intro").contains("<h2>Hello</h2>"));
    assert_eq!(page.inner("inside"), "A card body.");
    assert!(page.body.contains("id=\"note-form\""));
    assert!(page.body.contains("action=\"/form/echo\""));
    assert!(page.body.contains("value=\"a&lt;b&gt;\""));
    assert!(page.body.contains("name=\"_csrf\""), "a form component gets the CSRF field");
    assert!(!page.body.contains("<script"), "these components are static");

    let zero = send(&r, get("/ticks", None)).await;
    assert_eq!(strip_markers(&zero.inner("ticks")), "0");
    assert!(!zero.body.contains("id=\"tick-msg\""));
    let two = send(&r, get("/ticks-2", None)).await;
    assert_eq!(strip_markers(&two.inner("ticks")), "2");
    assert_eq!(strip_markers(&two.inner("tick-msg")), "ticked 2");
}

#[tokio::test]
async fn caching_depends_on_what_the_page_read() {
    let r = router().await;
    assert_eq!(send(&r, get("/text", None)).await.header("cache-control"), "no-cache");
    assert_eq!(send(&r, get("/form", None)).await.header("cache-control"), "private, no-store");
    assert!(send(&r, get("/text", None)).await.header("server-timing").starts_with("sources;dur="));
}

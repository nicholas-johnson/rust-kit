//! Rust and the browser must render the same HTML.
//!
//! For each interactive page, these tests take the HTML Rust sent, find every
//! block (`<!--rk:bN-->…<!--/rk:bN-->`), then load the page's real browser
//! module and the real runtime in Node and ask each block's render function
//! for its HTML with the same state and the same handoff payload. The two must
//! match, apart from block and handler ids, which differ by design.
//!
//! Needs `node` on PATH. Without it the tests are skipped locally and fail in CI.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use axum::Router;
use common::*;
use serde_json::{Value, json};

fn node() -> Option<&'static str> {
    let ok = Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    if !ok && std::env::var_os("CI").is_some() {
        panic!("node is required for the parity tests in CI");
    }
    if !ok {
        eprintln!("skipping parity test: node not found");
    }
    ok.then_some("node")
}

const STUB: &str = r#"export { state } from "./runtime.mjs";
export function start($s, page) { globalThis.__rk = { $s, page }; }
"#;

const HARNESS: &str = r#"import { readFileSync } from "node:fs";
const input = JSON.parse(readFileSync(process.argv[2], "utf8"));
const { esc, attr, LOADING } = await import("./runtime.mjs");
await import("./page.mjs");
const { $s, page } = globalThis.__rk;
Object.assign($s, input.overrides ?? {});
const P = input.payload ?? { s: {} };
let seq = 0;
const r = {
  esc,
  attr,
  csrf: () => P.csrf ?? "",
  on: (type) => `${type}:#${seq++}`,
  block(key, c) {
    const id = "i" + seq++;
    return `<!--rk:${id}-->${render(key, c)}<!--/rk:${id}-->`;
  },
};
function render(key, c) {
  const def = page.blocks[key];
  if (!def) throw new Error("no block " + key);
  if (!def.source) return def.render(c, r);
  const data = input.sources?.[def.source];
  return data === undefined ? LOADING : def.render({ ...c, [def.bind]: data }, r);
}
const blocks = {};
for (const key of input.blocks) blocks[key] = render(key, page.ssr[key]?.(P) ?? {});
// Every server-rendered handler must build into a function from the payload scope.
for (const [key, make] of Object.entries(page.handlers)) {
  if (!page.ssr[key]) continue;
  const fn = make(page.ssr[key](P));
  if (typeof fn !== "function") throw new Error(`handler ${key} did not produce a function`);
}
process.stdout.write(JSON.stringify({ blocks, state: { ...$s } }));
"#;

/// Every block occurrence in server HTML, outermost first: (key, inner HTML).
fn ssr_blocks(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    while let Some(off) = html[i..].find("<!--") {
        let at = i + off;
        let end = at + html[at..].find("-->").unwrap() + 3;
        let inner = &html[at + 4..end - 3];
        if let Some(key) = inner.strip_prefix("rk:") {
            stack.push((key.to_owned(), end));
        } else if let Some(key) = inner.strip_prefix("/rk:") {
            let (open, start) = stack.pop().expect("balanced markers");
            assert_eq!(open, key, "markers nest properly");
            out.push((open, html[start..at].to_owned()));
        }
        i = end;
    }
    assert!(stack.is_empty(), "unclosed block marker");
    out.reverse();
    out
}

/// Block and handler ids differ between server (`b3`, `h0`) and browser
/// (`i7`, `#2`) by design; everything else must be identical.
fn normalize(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        let next_marker = rest.find("<!--");
        let next_on = rest.find("data-rk-on=\"");
        match (next_marker, next_on) {
            (Some(m), on) if on.is_none_or(|o| m < o) => {
                let end = m + rest[m..].find("-->").unwrap() + 3;
                out.push_str(&rest[..m]);
                let c = &rest[m + 4..end - 3];
                out.push_str(if c.starts_with("rk:") {
                    "<!--rk-->"
                } else if c.starts_with("/rk:") {
                    "<!--/rk-->"
                } else {
                    &rest[m..end]
                });
                rest = &rest[end..];
            }
            (_, Some(o)) => {
                let start = o + "data-rk-on=\"".len();
                let end = start + rest[start..].find('"').unwrap();
                out.push_str(&rest[..start]);
                let events: Vec<&str> = rest[start..end].split(' ').map(|p| p.split(':').next().unwrap()).collect();
                out.push_str(&events.join(" "));
                rest = &rest[end..];
            }
            _ => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

struct Page {
    html: Res,
    dir: PathBuf,
}

/// Fetches a page and its browser module, and lays out a Node working directory.
async fn load(router: &Router, path: &str) -> Page {
    let html = send(router, get(path, None)).await;
    let module_url = html.module_url().unwrap_or_else(|| panic!("{path} has no browser module"));
    let module = send(router, get(&module_url, None)).await.body;
    let runtime_name = module.split("from \"./").nth(1).unwrap().split('"').next().unwrap().to_owned();
    let runtime = send(router, get(&format!("/_rk/assets/{runtime_name}"), None)).await.body;

    // Tests run in parallel and may load the same page; give each load its own directory.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rk-parity-{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        path.trim_matches('/').replace('/', "_")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("runtime.mjs"), runtime).unwrap();
    std::fs::write(dir.join("stub.mjs"), STUB).unwrap();
    std::fs::write(dir.join("page.mjs"), module.replace(&format!("./{runtime_name}"), "./stub.mjs")).unwrap();
    std::fs::write(dir.join("harness.mjs"), HARNESS).unwrap();
    Page { html, dir }
}

/// Renders `blocks` in Node with optional state overrides and source data.
fn run(node: &str, page: &Page, blocks: &[String], overrides: Value, sources: Value) -> BTreeMap<String, String> {
    let input = json!({
        "blocks": blocks,
        "overrides": overrides,
        "sources": sources,
        "payload": page.html.payload(),
    });
    let input_path = page.dir.join(format!("input-{}.json", blocks.join("-")));
    std::fs::write(&input_path, input.to_string()).unwrap();
    let out = Command::new(node).arg(page.dir.join("harness.mjs")).arg(&input_path).output().unwrap();
    assert!(out.status.success(), "node failed:\n{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v["blocks"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned())).collect()
}

fn keys(blocks: &[(String, String)]) -> Vec<String> {
    let mut k: Vec<String> = blocks.iter().map(|(k, _)| k.clone()).collect();
    k.sort();
    k.dedup();
    k
}

#[tokio::test]
async fn every_block_renders_the_same_in_rust_and_the_browser() {
    let Some(node) = node() else { return };
    let app = test_pages::app(false);
    let paths: Vec<&str> = app.routes().iter().filter(|r| r.js_bytes > 0).map(|r| r.path).collect();
    assert!(paths.contains(&"/nan"), "the float edge cases are covered");
    let router = app.into_router().await.unwrap();

    let mut checked = 0;
    let mut mismatches = Vec::new();
    for path in &paths {
        let url = located(path);
        let page = load(&router, &url).await;
        let ssr = ssr_blocks(&page.html.body);
        let js = run(node, &page, &keys(&ssr), json!({}), json!({}));
        for (key, rust_html) in &ssr {
            let (r, b) = (normalize(rust_html), normalize(&js[key]));
            if r != b {
                mismatches.push(format!("{path} {key}\n  rust:    {r:?}\n  browser: {b:?}"));
            }
            checked += 1;
        }
        std::fs::remove_dir_all(&page.dir).ok();
    }
    assert!(mismatches.is_empty(), "{} of {checked} blocks differ:\n{}", mismatches.len(), mismatches.join("\n"));
    assert!(checked >= 15, "expected to compare many blocks, compared {checked}");
}

/// Re-rendering a block for new state in the browser must give what Rust
/// would have rendered had the page started in that state.
#[tokio::test]
async fn state_changes_render_what_rust_would_have_rendered() {
    let Some(node) = node() else { return };
    let router = router().await;
    for (from, overrides, reference) in [
        ("/branches-0", json!({ "n": 2 }), "/branches-2"),
        ("/branches-0", json!({ "n": 5 }), "/branches-5"),
        ("/branches-5", json!({ "n": 0 }), "/branches-0"),
        ("/branches-2", json!({ "n": 5 }), "/branches-5"),
        ("/blog/hello", json!({ "open": true }), "/blog/hello/open"),
    ] {
        let page = load(&router, from).await;
        let want = ssr_blocks(&send(&router, get(reference, None)).await.body);
        let got = run(node, &page, &keys(&want), overrides.clone(), json!({}));
        for (key, rust_html) in &want {
            assert_eq!(normalize(&got[key]), normalize(rust_html), "{from} with {overrides} vs {reference}, block {key}");
        }
        std::fs::remove_dir_all(&page.dir).ok();
    }
}

/// The same markup, loaded on the server in one page and in the browser in
/// the other, must produce the same HTML.
#[tokio::test]
async fn client_loaded_data_renders_like_server_loaded_data() {
    let Some(node) = node() else { return };
    let router = router().await;
    let server = send(&router, get("/list-server", None)).await;
    let page = load(&router, "/list-client").await;
    let data: Value = serde_json::from_str(&send(&router, get("/_rk/source/items_live", None)).await.body).unwrap();
    let blocks = ssr_blocks(&page.html.body);
    assert_eq!(blocks.len(), 1, "one client source block");
    let got = run(node, &page, &keys(&blocks), json!({}), json!({ "items_live": data }));
    let browser_list = inner_html(&got[&blocks[0].0], "list").expect("browser rendered the list");
    assert_eq!(browser_list, server.inner("list"));
    std::fs::remove_dir_all(&page.dir).ok();

    let server = send(&router, get("/blog/notes", None)).await;
    let page = load(&router, "/blog/notes/live").await;
    let data: Value = serde_json::from_str(&send(&router, get("/_rk/source/post_live?slug=notes", None)).await.body).unwrap();
    let blocks = ssr_blocks(&page.html.body);
    let post = blocks.iter().find(|(_, h)| h.contains("id=\"post\"") || h.contains("rk-loading")).expect("client post block");
    let got = run(node, &page, &[post.0.clone()], json!({}), json!({ "post_live": data }));
    let browser_post = inner_html(&got[&post.0], "post").expect("browser rendered the post");
    assert_eq!(browser_post, server.inner("post"));
    std::fs::remove_dir_all(&page.dir).ok();
}

#[tokio::test]
async fn nested_and_attribute_blocks_follow_state() {
    let Some(node) = node() else { return };
    let router = router().await;

    let page = load(&router, "/nested").await;
    let ssr = ssr_blocks(&page.html.body);
    let outer = ssr.iter().find(|(_, h)| h.contains("id=\"outer\"")).unwrap().0.clone();
    let got = run(node, &page, &[outer.clone()], json!({ "inner": true, "n": 7 }), json!({}));
    let html = &got[&outer];
    assert!(html.contains("Outer block, n = <!--rk:"), "n is its own block inside the outer block: {html}");
    assert!(html.contains("id=\"inner\""), "{html}");
    assert!(html.contains("&lt;b&gt;not bold&lt;/b&gt;"), "state strings are escaped: {html}");
    assert!(!html.contains("inner-off"));
    std::fs::remove_dir_all(&page.dir).ok();

    let page = load(&router, "/counter").await;
    let ssr = ssr_blocks(&page.html.body);
    let all = keys(&ssr);
    let got = run(node, &page, &all, json!({ "count": 3 }), json!({}));
    let dec = got.values().find(|h| h.contains("id=\"dec\"")).unwrap();
    assert!(!dec.contains("disabled"), "count 3 enables the decrement button: {dec}");
    assert!(got.values().any(|h| h.contains("That is <!--rk:")), "message block shows at 3");
    std::fs::remove_dir_all(&page.dir).ok();
}

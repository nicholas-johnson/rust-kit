//! Rust-kit compiler.
//!
//! Reads `.rk` page files from a routes directory and writes:
//! - one Rust file with a native `render` function per route and a manifest,
//! - one browser module per interactive route plus the shared runtime,
//! - one stylesheet per route that has a `<style>` block.
//!
//! Use [`build_script`] from a crate's `build.rs`.

mod expr;
mod codegen;
mod parse;
mod script;

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

pub use codegen::Options;

const RUNTIME_JS: &str = include_str!("runtime.js");

/// The browser runtime every interactive route loads, as shipped.
pub fn runtime_js() -> &'static str {
    RUNTIME_JS
}

/// A compile error at a byte offset in a page file.
#[derive(Debug, Clone)]
pub struct Diag {
    pub pos: usize,
    pub msg: String,
    pub help: Option<String>,
}

impl Diag {
    pub fn new(pos: usize, msg: impl Into<String>) -> Self {
        Self { pos, msg: msg.into(), help: None }
    }

    pub fn help(mut self, h: impl Into<String>) -> Self {
        self.help = Some(h.into());
        self
    }

    /// Formats the diagnostic with the offending source line, rustc style.
    pub fn render(&self, file: &str, src: &str) -> String {
        let pos = self.pos.min(src.len());
        let line_start = src[..pos].rfind('\n').map_or(0, |i| i + 1);
        let line_no = src[..pos].matches('\n').count() + 1;
        let line_end = src[pos..].find('\n').map_or(src.len(), |i| pos + i);
        let col = src[line_start..pos].chars().count() + 1;
        let gutter = " ".repeat(line_no.to_string().len());
        let mut s = format!(
            "error: {}\n{gutter}--> {file}:{line_no}:{col}\n{gutter} |\n{line_no} | {}\n{gutter} | {}^\n",
            self.msg,
            &src[line_start..line_end],
            " ".repeat(col - 1)
        );
        if let Some(h) = &self.help {
            let _ = writeln!(s, "{gutter} = help: {h}");
        }
        s
    }
}

/// The result of compiling one page in isolation (used by tests and tools).
pub struct CompiledPage {
    pub rust: String,
    pub js: Option<String>,
    pub css: Option<String>,
    pub prerender: bool,
    pub server_sources: Vec<String>,
    pub client_sources: Vec<String>,
}

pub fn compile_page(src: &str, opts: &Options) -> Result<CompiledPage, Diag> {
    let page = parse::parse(src)?;
    let out = codegen::generate(&page, opts)?;
    Ok(CompiledPage {
        rust: out.rust,
        js: out.js,
        css: out.css,
        prerender: out.prerender,
        server_sources: out.server_sources,
        client_sources: out.client_sources,
    })
}

pub struct Config {
    pub routes_dir: PathBuf,
    pub out_dir: PathBuf,
    /// Path of the application state type, e.g. `crate::AppState`.
    pub state_type: String,
    /// Module holding the source resolvers, e.g. `crate::sources`.
    pub sources_module: String,
}

/// Per-route numbers reported by a build.
pub struct RouteSummary {
    pub path: String,
    pub prerender: bool,
    pub js_bytes: usize,
    pub css_bytes: usize,
}

/// 64-bit FNV-1a, used for content-addressed asset names.
fn hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:08x}", h >> 32)
}

fn route_path(rel: &Path) -> String {
    let mut segs: Vec<String> =
        rel.with_extension("").components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if segs.last().is_some_and(|s| s == "index") {
        segs.pop();
    }
    format!("/{}", segs.join("/"))
}

fn find_pages(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            find_pages(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "rk") {
            out.push(p);
        }
    }
    Ok(())
}

/// Compiles every page under `routes_dir`. On failure, returns rendered
/// diagnostics for all pages that failed.
pub fn compile(cfg: &Config) -> Result<Vec<RouteSummary>, String> {
    let mut pages = Vec::new();
    find_pages(&cfg.routes_dir, &mut pages).map_err(|e| format!("reading {}: {e}", cfg.routes_dir.display()))?;
    let opts = Options { state_type: &cfg.state_type, sources_module: &cfg.sources_module };
    let asset_dir = cfg.out_dir.join("rk_assets");
    fs::create_dir_all(&asset_dir).map_err(|e| e.to_string())?;

    let runtime_name = format!("runtime.{}.js", hash(RUNTIME_JS));
    let mut assets: Vec<(String, &str, String)> = Vec::new();
    let mut runtime_used = false;
    let mut modules = String::new();
    let mut routes = String::new();
    let mut client_sources: Vec<String> = Vec::new();
    let mut summaries = Vec::new();
    let mut errors = String::new();

    for file in &pages {
        let src = fs::read_to_string(file).map_err(|e| format!("reading {}: {e}", file.display()))?;
        let rel = file.strip_prefix(&cfg.routes_dir).unwrap();
        let display = rel.display().to_string();
        let page = match parse::parse(&src).and_then(|p| codegen::generate(&p, &opts)) {
            Ok(p) => p,
            Err(d) => {
                errors.push_str(&d.render(&format!("src/routes/{display}"), &src));
                continue;
            }
        };
        let path = route_path(rel);
        let stem: String = rel
            .with_extension("")
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();

        let mut js_bytes = 0;
        let js_url = page.js.as_ref().map(|js| {
            runtime_used = true;
            let js = js.replace("__RUNTIME__", &runtime_name);
            let name = format!("{stem}.{}.js", hash(&js));
            js_bytes = js.len() + RUNTIME_JS.len();
            assets.push((name.clone(), "text/javascript; charset=utf-8", js));
            format!("/_rk/assets/{name}")
        });
        let css_bytes = page.css.as_ref().map_or(0, String::len);
        let css_url = page.css.as_ref().map(|css| {
            let name = format!("{stem}.{}.css", hash(css));
            assets.push((name.clone(), "text/css; charset=utf-8", css.to_string()));
            format!("/_rk/assets/{name}")
        });

        let _ = writeln!(
            modules,
            "/// Compiled from `src/routes/{display}`.\n#[allow(unused, unused_mut, non_snake_case, clippy::all)]\npub mod page_{stem} {{\nconst CSS: Option<&str> = {:?};\nconst JS: Option<&str> = {:?};\n{}}}\n",
            css_url, js_url, page.rust
        );
        let _ = writeln!(
            routes,
            "::rustkit::Route {{ path: {path:?}, prerender: {}, render: page_{stem}::render_boxed, js_bytes: {js_bytes}, css_bytes: {css_bytes} }},",
            page.prerender
        );
        for s in page.client_sources {
            if !client_sources.contains(&s) {
                client_sources.push(s);
            }
        }
        summaries.push(RouteSummary { path, prerender: page.prerender, js_bytes, css_bytes });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    if runtime_used {
        assets.push((runtime_name, "text/javascript; charset=utf-8", RUNTIME_JS.to_owned()));
    }

    let st = &cfg.state_type;
    let sm = &cfg.sources_module;
    let mut out = String::from("// @generated by rustkit-compiler. Do not edit.\n\n");
    out.push_str(&modules);
    let _ = writeln!(out, "/// Sources some page loads in the browser. Only these are reachable over HTTP.");
    let _ = writeln!(out, "pub const CLIENT_SOURCES: &[&str] = &{client_sources:?};\n");
    let _ = writeln!(
        out,
        "pub fn source_api<'a>(name: &'a str, ctx: &'a ::rustkit::RequestContext<{st}>) -> Option<::rustkit::BoxFut<'a, ::std::result::Result<::rustkit::__private::serde_json::Value, ::rustkit::Error>>> {{\n    match name {{"
    );
    for s in &client_sources {
        let _ = writeln!(
            out,
            "        {s:?} => Some(Box::pin(async move {{ let v = {sm}::{s}(ctx).await?; Ok(::rustkit::__private::serde_json::to_value(&v)?) }})),"
        );
    }
    out.push_str("        _ => None,\n    }\n}\n\n");
    let _ = writeln!(out, "pub fn manifest() -> ::rustkit::Manifest<{st}> {{\n    ::rustkit::Manifest {{\n        routes: vec![\n{routes}        ],\n        assets: vec![");
    for (name, ct, body) in &assets {
        fs::write(asset_dir.join(name), body).map_err(|e| e.to_string())?;
        let _ = writeln!(
            out,
            "            ::rustkit::Asset {{ path: \"/_rk/assets/{name}\", content_type: {ct:?}, body: include_str!(concat!(env!(\"OUT_DIR\"), \"/rk_assets/{name}\")) }},"
        );
    }
    out.push_str("        ],\n        client_sources: CLIENT_SOURCES,\n        source_api,\n    }\n}\n");
    fs::write(cfg.out_dir.join("rustkit_generated.rs"), out).map_err(|e| e.to_string())?;
    Ok(summaries)
}

/// Entry point for `build.rs`: compiles `src/routes` into `$OUT_DIR`, using
/// `crate::AppState` and `crate::sources`. Exits with diagnostics on error.
pub fn build_script() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("run from build.rs"));
    let cfg = Config {
        routes_dir: manifest_dir.join("src/routes"),
        out_dir: PathBuf::from(std::env::var("OUT_DIR").expect("run from build.rs")),
        state_type: "crate::AppState".into(),
        sources_module: "crate::sources".into(),
    };
    println!("cargo:rerun-if-changed=src/routes");
    match compile(&cfg) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests;

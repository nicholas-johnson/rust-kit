//! Rust-kit compiler.
//!
//! Reads `.rk` page files from a routes directory and writes:
//! - one Rust file with a native `render` function per route and a manifest,
//! - one browser module per interactive route plus the shared runtime,
//! - one stylesheet per route that has a `<style>` block,
//! - with Tailwind turned on, one app-wide stylesheet linked from every route.
//!
//! Use [`build_script`] from a crate's `build.rs`, or [`Build`] to turn on
//! options such as Tailwind:
//!
//! ```no_run
//! fn main() {
//!     rustkit_compiler::Build::new().tailwind().run();
//! }
//! ```

mod codegen;
mod component;
mod config;
mod css;
mod expr;
mod parse;
mod route;
mod script;
mod tailwind;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

pub use codegen::Options;
pub use config::{FileConfig, load as load_file_config};
pub use tailwind::{TAILWIND_ENV, Tailwind};

const RUNTIME_JS: &str = include_str!("runtime.js");

/// The browser runtime. Lines between `// dev:start` and `// dev:end` are
/// hooks for the `rustkit dev` client; production builds leave them out.
pub fn runtime_js(dev: bool) -> String {
    let mut out = String::with_capacity(RUNTIME_JS.len());
    let mut skipping = false;
    for line in RUNTIME_JS.split_inclusive('\n') {
        match line.trim() {
            "// dev:start" => skipping = !dev,
            "// dev:end" => skipping = false,
            _ if !skipping => out.push_str(line),
            _ => {}
        }
    }
    out
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
        let mut s = format!("error: {}\n{gutter}--> {file}:{line_no}:{col}\n{gutter} |\n{line_no} | {}\n{gutter} | {}^\n", self.msg, &src[line_start..line_end], " ".repeat(col - 1));
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
    let mut page = parse::parse(src)?;
    css::apply(&mut page.style, page.style_global, &mut page.nodes).map_err(|err| {
        Diag::new(page.style_at.unwrap_or(0) + err.pos, err.msg).help("selectors are scoped to this file. Wrap one in `:global(...)`, or mark the block `<style global>`")
    })?;
    let out = codegen::generate(&page, opts)?;
    Ok(CompiledPage { rust: out.rust, js: out.js, css: out.css, prerender: out.prerender, server_sources: out.server_sources, client_sources: out.client_sources })
}

pub struct Config {
    pub routes_dir: PathBuf,
    pub out_dir: PathBuf,
    /// Path of the application state type, e.g. `crate::AppState`.
    pub state_type: String,
    /// Module holding the source resolvers, e.g. `crate::sources`.
    pub sources_module: String,
    /// Include the runtime hooks `rustkit dev` uses to update pages in place.
    pub dev: bool,
    /// The crate's root directory. Tailwind runs here.
    pub crate_dir: PathBuf,
    /// Generate an app-wide Tailwind stylesheet. Off when `None`.
    pub tailwind: Option<Tailwind>,
    /// Scope `<style>` blocks to the file that wrote them. On unless
    /// `rustkit.toml` sets `css_scope = false`.
    pub css_scope: bool,
}

/// Per-route numbers reported by a build.
pub struct RouteSummary {
    pub path: String,
    pub prerender: bool,
    pub js_bytes: usize,
    pub css_bytes: usize,
}

/// 64-bit FNV-1a, used for content-addressed asset names and CSS scope ids.
pub(crate) fn hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:08x}", h >> 32)
}

/// Compiles every page under `routes_dir`. On failure, returns rendered
/// diagnostics for all pages that failed. `layout.rk` files wrap the pages
/// in their directory; they are not routes.
pub fn compile(cfg: &Config) -> Result<Vec<RouteSummary>, String> {
    compile_inner(cfg, true)
}

/// [`compile`] for tools such as `rustkit check`: the same output, without
/// the `cargo:rerun-if-changed` lines a build script prints.
pub fn compile_quiet(cfg: &Config) -> Result<Vec<RouteSummary>, String> {
    compile_inner(cfg, false)
}

fn compile_inner(cfg: &Config, cargo_directives: bool) -> Result<Vec<RouteSummary>, String> {
    let prepared = route::prepare(&cfg.routes_dir, cfg.css_scope)?;
    if cargo_directives {
        for path in &prepared.component_files {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    let opts = Options { state_type: &cfg.state_type, sources_module: &cfg.sources_module };
    let asset_dir = cfg.out_dir.join("rk_assets");
    fs::create_dir_all(&asset_dir).map_err(|e| e.to_string())?;

    let runtime = runtime_js(cfg.dev);
    let runtime_name = format!("runtime.{}.js", hash(&runtime));
    let mut assets: Vec<(String, &str, String)> = Vec::new();
    let mut runtime_used = false;
    let mut modules = String::new();
    let mut routes = String::new();
    let mut client_sources: Vec<String> = Vec::new();
    let mut summaries = Vec::new();
    let mut errors = String::new();

    // App-wide stylesheets, linked from every page before its own `<style>`.
    let mut global_css: Vec<String> = Vec::new();
    let mut global_css_bytes = 0;
    if let Some(tw) = &cfg.tailwind {
        let css = tw.run(&cfg.crate_dir)?;
        let name = format!("tailwind.{}.css", hash(&css));
        global_css_bytes += css.len();
        global_css.push(format!("/_rk/assets/{name}"));
        assets.push((name, "text/css; charset=utf-8", css));
    }

    for file in &prepared.pages {
        let param_sources: BTreeSet<String> = match &file.param {
            Some(param) => prepared.param_sources.iter().filter(|(_, p)| *p == param).map(|(name, _)| name.clone()).collect(),
            None => BTreeSet::new(),
        };
        let page = match codegen::generate_routed(&file.page, &opts, file.param.as_deref(), &param_sources) {
            Ok(p) => p,
            Err(d) => {
                errors.push_str(&route::render_diag(&prepared.spans, &d));
                continue;
            }
        };
        let path = file.pattern.clone();
        let display = &file.display;
        let stem: String = file.rel.with_extension("").to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();

        let mut js_bytes = 0;
        let js_url = page.js.as_ref().map(|js| {
            runtime_used = true;
            let js = js.replace("__RUNTIME__", &runtime_name);
            let name = format!("{stem}.{}.js", hash(&js));
            js_bytes = js.len() + runtime.len();
            assets.push((name.clone(), "text/javascript; charset=utf-8", js));
            format!("/_rk/assets/{name}")
        });
        let css_bytes = global_css_bytes + page.css.as_ref().map_or(0, String::len);
        let mut css_urls = global_css.clone();
        if let Some(css) = &page.css {
            let name = format!("{stem}.{}.css", hash(css));
            assets.push((name.clone(), "text/css; charset=utf-8", css.to_string()));
            css_urls.push(format!("/_rk/assets/{name}"));
        }

        let _ = writeln!(
            modules,
            "/// Compiled from `src/routes/{display}`.\n#[allow(unused, unused_mut, non_snake_case, clippy::all)]\npub mod page_{stem} {{\nconst CSS: &[&str] = &{:?};\nconst JS: Option<&str> = {:?};\n{}}}\n",
            css_urls, js_url, page.rust
        );
        let param = file.param.as_deref();
        let _ = writeln!(routes, "::rustkit::Route {{ path: {path:?}, param: {param:?}, prerender: {}, render: page_{stem}::render_boxed, js_bytes: {js_bytes}, css_bytes: {css_bytes} }},", page.prerender);
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
        assets.push((runtime_name, "text/javascript; charset=utf-8", runtime));
    }

    let st = &cfg.state_type;
    let sm = &cfg.sources_module;
    let mut out = String::from("// @generated by rustkit-compiler. Do not edit.\n\n");
    out.push_str(&modules);
    let _ = writeln!(out, "/// Sources some page loads in the browser. Only these are reachable over HTTP.");
    let _ = writeln!(out, "pub const CLIENT_SOURCES: &[&str] = &{client_sources:?};\n");
    let _ = writeln!(
        out,
        "pub fn source_api<'a>(name: &'a str, ctx: &'a ::rustkit::RequestContext<{st}>, params: &'a [(&'a str, &'a str)]) -> Option<::rustkit::BoxFut<'a, ::std::result::Result<::rustkit::__private::serde_json::Value, ::rustkit::Error>>> {{\n    match name {{"
    );
    for s in &client_sources {
        if let Some(param) = prepared.param_sources.get(s) {
            let _ = writeln!(
                out,
                "        {s:?} => {{\n            let Some(__p) = params.iter().find(|(k, _)| *k == {param:?}).map(|(_, v)| *v).filter(|v| !v.is_empty()) else {{\n                return Some(Box::pin(async {{ Err(::rustkit::Error::BadRequest(\"missing route parameter `{param}`\".into())) }}));\n            }};\n            Some(Box::pin(async move {{ let v = {sm}::{s}(ctx, __p).await?; Ok(::rustkit::__private::serde_json::to_value(&v)?) }}))\n        }}"
            );
        } else {
            let _ = writeln!(out, "        {s:?} => Some(Box::pin(async move {{ let v = {sm}::{s}(ctx).await?; Ok(::rustkit::__private::serde_json::to_value(&v)?) }})),");
        }
    }
    out.push_str("        _ => { let _ = (ctx, params); None }\n    }\n}\n\n");
    let _ = writeln!(out, "pub fn manifest() -> ::rustkit::Manifest<{st}> {{\n    ::rustkit::Manifest {{\n        routes: vec![\n{routes}        ],\n        assets: vec![");
    for (name, ct, body) in &assets {
        fs::write(asset_dir.join(name), body).map_err(|e| e.to_string())?;
        let _ = writeln!(out, "            ::rustkit::Asset {{ path: \"/_rk/assets/{name}\", content_type: {ct:?}, body: include_str!(concat!(env!(\"OUT_DIR\"), \"/rk_assets/{name}\")) }},");
    }
    out.push_str("        ],\n        client_sources: CLIENT_SOURCES,\n        source_api,\n    }\n}\n");
    fs::write(cfg.out_dir.join("rustkit_generated.rs"), out).map_err(|e| e.to_string())?;
    Ok(summaries)
}

/// Entry point for `build.rs`: compiles `src/routes` into `$OUT_DIR`, using
/// `crate::AppState` and `crate::sources`. Exits with diagnostics on error.
///
/// Same as `Build::new().run()`.
pub fn build_script() {
    Build::new().run();
}

/// Build options for `build.rs`.
///
/// ```no_run
/// fn main() {
///     rustkit_compiler::Build::new().tailwind().run();
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Build {
    tailwind: Option<Tailwind>,
}

impl Build {
    pub fn new() -> Self {
        Self::default()
    }

    /// Turns on Tailwind CSS: every page links one minified, app-wide
    /// stylesheet holding the utilities its templates use.
    ///
    /// The input is `src/tailwind.css` if the crate has one, and otherwise
    /// `@import "tailwindcss"` scanning `src/routes`. The CLI is
    /// `$RUSTKIT_TAILWIND`, `node_modules/.bin/tailwindcss`, or `tailwindcss`
    /// on `PATH`, in that order.
    pub fn tailwind(self) -> Self {
        self.tailwind_with(Tailwind { minify: true, ..Tailwind::default() })
    }

    /// Turns on Tailwind CSS with explicit settings.
    pub fn tailwind_with(mut self, tw: Tailwind) -> Self {
        self.tailwind = Some(tw);
        self
    }

    /// Compiles `src/routes` into `$OUT_DIR`. Exits with diagnostics on error.
    pub fn run(self) {
        let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("run from build.rs"));
        println!("cargo:rerun-if-changed=src/routes");
        println!("cargo:rerun-if-changed=src/lib");
        println!("cargo:rerun-if-changed=rustkit.toml");
        println!("cargo:rerun-if-env-changed=RUSTKIT_DEV");
        let file = match config::load(&crate_dir) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        if let Some(tw) = &self.tailwind {
            println!("cargo:rerun-if-env-changed={TAILWIND_ENV}");
            println!("cargo:rerun-if-changed=src/tailwind.css");
            if let Some(p) = &tw.input {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
        let cfg = Config {
            routes_dir: crate_dir.join("src/routes"),
            out_dir: PathBuf::from(std::env::var("OUT_DIR").expect("run from build.rs")),
            state_type: "crate::AppState".into(),
            sources_module: "crate::sources".into(),
            crate_dir,
            tailwind: self.tailwind,
            // `rustkit dev` sets this for its builds.
            dev: std::env::var("RUSTKIT_DEV").is_ok_and(|v| v == "1"),
            css_scope: file.css_scope,
        };
        if let Err(e) = compile(&cfg) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests;

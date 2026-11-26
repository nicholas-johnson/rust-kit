---
title: Compiler API
description: The rustkit-compiler crate's public functions and types, and the files it writes into OUT_DIR.
section: Reference
order: 3
---

`rustkit-compiler` reads `.rk` page files and writes Rust and browser code. You call it from your crate's `build.rs`. It has no dependencies of its own, and it is not on crates.io yet, so depend on it from git:

```toml
[build-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

The crate exposes three functions for builds (`build_script`, `compile`, `runtime_js`), one for compiling a single page in isolation (`compile_page`), and the types they use.

## build_script

```rust
pub fn build_script()
```

The one-line entry point for `build.rs`, and the one the [account demo](/docs/project-structure/) uses:

```rust title=build.rs
fn main() {
    rustkit_compiler::build_script();
}
```

It fixes every setting by convention:

| Setting | Value |
|---|---|
| Pages | `$CARGO_MANIFEST_DIR/src/routes`, searched recursively for `*.rk` |
| Output | `$OUT_DIR` |
| State type | `crate::AppState` |
| Sources module | `crate::sources` |

It prints `cargo:rerun-if-changed=src/routes` and `cargo:rerun-if-changed=src/lib`, and one line for each component file it read, so Cargo reruns it when a page or a component changes. On failure it prints every diagnostic to stderr and exits with status 1, which fails the build. It panics if `CARGO_MANIFEST_DIR` or `OUT_DIR` is unset, which means it only works when Cargo runs it as a build script.

## compile and Config

```rust
pub struct Config {
    pub routes_dir: PathBuf,
    pub out_dir: PathBuf,
    /// Path of the application state type, e.g. `crate::AppState`.
    pub state_type: String,
    /// Module holding the source resolvers, e.g. `crate::sources`.
    pub sources_module: String,
    /// Include the runtime hooks `rustkit dev` uses to update pages in place.
    pub dev: bool,
    /// The crate's root directory. Tailwind runs here, and `build_script`
    /// reads `rustkit.toml` from here.
    pub crate_dir: PathBuf,
    /// Generate an app-wide Tailwind stylesheet. Off when `None`.
    pub tailwind: Option<Tailwind>,
    /// Scope each `<style>` to the file that wrote it. `build_script` sets
    /// this from `rustkit.toml`; it is on when that file is missing.
    pub css_scope: bool,
}

pub fn compile(cfg: &Config) -> Result<Vec<RouteSummary>, String>
```

Use `compile` when the conventions do not fit, for example to keep state in another module:

```rust title=build.rs
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let cfg = rustkit_compiler::Config {
        routes_dir: root.join("src/routes"),
        out_dir: PathBuf::from(std::env::var("OUT_DIR").unwrap()),
        state_type: "crate::state::AppState".into(),
        sources_module: "crate::data::sources".into(),
        dev: false,
        crate_dir: root.clone(),
        tailwind: None,
        css_scope: true,
    };
    println!("cargo:rerun-if-changed=src/routes");
    println!("cargo:rerun-if-changed=src/lib");
    if let Err(diagnostics) = rustkit_compiler::compile(&cfg) {
        eprintln!("{diagnostics}");
        std::process::exit(1);
    }
}
```

`compile` prints `cargo:rerun-if-changed` for each component file it read. It does not print the directory lines; add those as above. It compiles every page it finds, sorted by path. If any page fails, it returns `Err` with the rendered diagnostics for every failing page joined together, and writes no Rust file. Other errors, such as a missing routes directory, come back as `reading <path>: <io error>`.

> [!NOTE]
> Page diagnostics are labelled `src/routes/<relative path>`, even when `routes_dir` points somewhere else. A component file next to that routes directory is labelled `src/lib/…`.

### RouteSummary

```rust
pub struct RouteSummary {
    pub path: String,
    pub prerender: bool,
    pub js_bytes: usize,
    pub css_bytes: usize,
}
```

One per page, in the same order as the routes in the manifest. `js_bytes` is the page module plus the shared runtime, uncompressed; it is 0 when the page needs no module. The same numbers appear on each [`Route`](/docs/runtime-api/#compiled-output-types) at run time.

### How file paths become routes

| File under `routes_dir` | Route path | Module and asset stem |
|---|---|---|
| `index.rk` | `/` | `index` |
| `article.rk` | `/article` | `article` |
| `docs/index.rk` | `/docs` | `docs_index` |
| `docs/getting-started.rk` | `/docs/getting-started` | `docs_getting_started` |

The stem replaces every character that is not an ASCII letter or digit with `_`, and is used for the generated module name (`page_<stem>`) and asset names. Dynamic segments such as `[id].rk` are not supported yet; a file with that name becomes a literal path. See the [roadmap](/docs/roadmap/).

## Output files

`compile` writes into `out_dir`:

| File | Contents |
|---|---|
| `rustkit_generated.rs` | One `pub mod page_<stem>` per page with `render` and `render_boxed`, then `CLIENT_SOURCES`, `source_api` and `manifest()`. |
| `rk_assets/<stem>.<hash>.js` | The page's browser module, for pages that need one. |
| `rk_assets/<stem>.<hash>.css` | The page's `<style>` contents, for pages that have a non-empty one. |
| `rk_assets/runtime.<hash>.js` | The shared browser runtime, written only if at least one page has a module. |

`<hash>` is 8 hex digits taken from a 64-bit FNV-1a hash of the file's final contents, so a file name changes exactly when its contents do. That is what makes the long `immutable` cache lifetime on `/_rk/assets/` safe. Pages with identical styles still get separate files, because the stem differs.

Include the Rust file once in your crate:

```rust title=src/lib.rs
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}
```

The generated code names the runtime as `::rustkit::…` and the assets with `include_str!`, so the binary is self-contained: assets are served from memory and nothing in `OUT_DIR` is needed at run time. [Generated code](/docs/generated-code/) walks through a real example.

## compile_page

```rust
pub fn compile_page(src: &str, opts: &Options) -> Result<CompiledPage, Diag>

pub struct Options<'a> {
    pub state_type: &'a str,
    pub sources_module: &'a str,
}

pub struct CompiledPage {
    /// The body of the page's Rust module: `render` and `render_boxed`.
    pub rust: String,
    /// The browser module, if the page needs one.
    pub js: Option<String>,
    pub css: Option<String>,
    pub prerender: bool,
    /// Sources loaded with `load="server"`, in first-use order.
    pub server_sources: Vec<String>,
    /// Sources loaded with `load="client"`, sorted.
    pub client_sources: Vec<String>,
}
```

Compiles one page from a string, with no file system access. It is what the compiler's own tests and this documentation site use; every `rk` sample on this site goes through it. Use it to unit test what a page compiles to. See [Testing](/docs/testing/#testing-what-a-page-compiles-to).

There are two differences from `compile`. The Rust code lacks the `CSS` and `JS` constants that `compile` adds around it, so it does not build on its own. The browser module still imports from the placeholder `./__RUNTIME__`, which `compile` replaces with the hashed runtime file name.

## Diag

```rust
#[derive(Debug, Clone)]
pub struct Diag {
    /// Byte offset into the page source.
    pub pos: usize,
    pub msg: String,
    pub help: Option<String>,
}

impl Diag {
    pub fn new(pos: usize, msg: impl Into<String>) -> Self;
    pub fn help(self, h: impl Into<String>) -> Self;
    pub fn render(&self, file: &str, src: &str) -> String;
}
```

A compile error. `render` formats it rustc style, with `file` used as the label and `src` the page source the offset refers to. The column counts characters, not bytes. `compile_page` stops at the first error, so there is at most one `Diag` per page.

```rust
let src = "<p>{nope}</p>";
let diag = compile_page(src, &opts).err().unwrap();
eprintln!("{}", diag.render("src/routes/demo.rk", src));
```

prints:

```text
error: `nope` is not defined here
 --> src/routes/demo.rk:1:5
  |
1 | <p>{nope}</p>
  |     ^
  = help: template expressions can read `$state` variables, a `<DataSource let:…>` binding, or an `{#each}` item
```

[Compiler diagnostics](/docs/diagnostics/) lists every message.

## runtime_js

```rust
pub fn runtime_js() -> &'static str
```

The browser runtime source, exactly as shipped in `runtime.<hash>.js`. It is 6,658 bytes unminified and about 2.7 KB gzipped in the current version. Use it if you need to serve or measure the runtime yourself; `compile` already writes it out when a page needs it.

## Not available

There is no command-line compiler, no watch mode, and no option for minification or source maps. CSS scoping is on by default; `rustkit.toml` sets `css_scope = false` for `build_script`, and `Config.css_scope` does the same for `compile`. The `cli` page lists the [commands that do exist](/docs/cli/).

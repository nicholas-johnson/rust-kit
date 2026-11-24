---
title: CLI
description: The commands that exist today, and how to give your own app the same serve and build commands.
section: Reference
order: 6
---

Rust-kit has no command-line tool of its own. There is no `rustkit` binary, no project generator, and no dev server with live reload. Pages compile as part of an ordinary `cargo build`, through your crate's [`build.rs`](/docs/compiler-api/), and your application is a normal Rust binary whose `main` you write.

This page documents the commands that do exist in the repository, then shows how to write the same `serve` and `build` commands for your own app. A dedicated CLI is not available yet; see the [roadmap](/docs/roadmap/).

## Commands in the repository

All of these run from the repository root.

| Command | What it does |
|---|---|
| `cargo test` | Runs the compiler's unit tests, the runtime's unit tests, and the account demo's acceptance tests. |
| `cargo run -p account-demo -- serve [addr]` | Serves the demo app. |
| `cargo run -p account-demo -- build [dir]` | Writes the demo's prerendered pages and assets, and prints bytes per route. |
| `cargo run -p site -- build [dir]` | Builds this documentation site. |

### account-demo serve

```bash
cargo run -p account-demo -- serve 127.0.0.1:3000
```

| Argument | Default | Meaning |
|---|---|---|
| `addr` | `127.0.0.1:3000` | The socket address to listen on. |

`serve` is also what runs when you pass no arguments. It imitates a 25 ms round trip on every call to the in-memory accounts service, so the `Server-Timing` header shows realistic `sources` times. It turns `Secure` cookies off only when the address starts with `127.0.0.1`. On any other address, such as `0.0.0.0:3000` or a LAN address, the session cookie is marked `Secure`. Browsers refuse `Secure` cookies over plain HTTP (some make an exception for `localhost`, not all), so signing in appears not to work. See [Troubleshooting](/docs/troubleshooting/#signing-in-does-nothing-on-plain-http).

Sign in at `/login` as Alice or Bob. The demo accounts have no password.

### account-demo build

```bash
cargo run -p account-demo -- build dist
```

| Argument | Default | Meaning |
|---|---|---|
| `dir` | `dist` | Output directory, created if needed. Existing files are overwritten, not removed. |

It calls [`App::build`](/docs/runtime-api/#app), then prints one line per route. This is the real output for the demo today:

```text
route        mode       JS bytes  CSS bytes
/account     request        7998        689
/article     static            0        546
/            static            0        512
/login       request           0        512

wrote 8 files to dist
```

`static` routes were prerendered and written as `index.html` files. `request` routes render per request, so `build` does not write them; you need the running server for those. JS bytes are the page module plus the shared runtime, uncompressed. See [Performance](/docs/performance/#javascript-per-route).

Any other first argument prints ``unknown command `x`; use `serve [addr]` or `build [dir]` `` and exits with status 2.

### site build

```bash
cargo run -p site -- build site/dist
```

With no directory it writes to `site/dist`. It deletes the output directory first, renders every Markdown file under `site/content`, compiles every `rk` code block with the real compiler, and checks every internal link and anchor. It exits with status 1 if a sample fails to compile, a `fail` sample compiles, front matter is invalid, or a link is broken. Any other arguments print `usage: site build [out-dir]` and exit with status 2.

## Writing your own serve and build

Your app's `main.rs` decides what commands it has. The demo's is 33 lines; here is the same shape for your own crate. It assumes your library exposes an `app()` function that returns a configured [`App`](/docs/runtime-api/#app), like the demo's `account_demo::app`.

```rust title=src/main.rs
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("build") => {
            let out = PathBuf::from(args.get(1).map_or("dist", String::as_str));
            let app = my_app::app(true);
            let report = app.build(&out).await.expect("build failed");
            for r in app.routes() {
                let mode = if r.prerender { "static" } else { "request" };
                println!("{:<24} {:<8} {:>8} JS {:>8} CSS", r.path, mode, r.js_bytes, r.css_bytes);
            }
            println!("wrote {} files to {}", report.files.len(), out.display());
        }
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            let local = addr.starts_with("127.0.0.1") || addr.starts_with("localhost");
            let app = my_app::app(!local);
            let router = app.into_router().await.expect("prerender failed");
            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            println!("listening on http://{addr}");
            axum::serve(listener, router).await.unwrap();
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; use `serve [addr]` or `build [dir]`");
            std::process::exit(2);
        }
    }
}
```

```rust title=src/lib.rs
pub fn app(secure_cookies: bool) -> rustkit::App<AppState> {
    rustkit::App::new(generated::manifest(), AppState::new())
        .secure_cookies(secure_cookies)
        .action("/account/profile", actions::update_profile)
}
```

Your crate needs `tokio` with the `macros` and `rt-multi-thread` features and `axum` 0.8 as direct dependencies for this `main`.

### What the two commands rely on

| Call | Behaviour to know |
|---|---|
| `App::into_router()` | Prerenders every `prerender` route first and returns `Err` if any of them fails, for example because it read the session. |
| `App::build(dir)` | Writes prerendered routes as `<path>/index.html` (`/` becomes `index.html`) and every asset under `_rk/assets/`. Returns a [`BuildReport`](/docs/runtime-api/#compiled-output-types) listing the relative paths. |
| `App::routes()` | The compiled routes with `path`, `prerender`, `js_bytes` and `css_bytes`. |

The binary built by `cargo build --release` contains every page and asset, so `serve` needs nothing else at run time. The output of `build` is useful when a CDN or another web server should serve the static pages and assets directly; see [Deployment](/docs/deployment/).

## Things that do not exist yet

- `rustkit new`, or any other scaffolding command. Set a project up by hand as in [Getting started](/docs/getting-started/).
- A watch mode or live reload. Use a general tool such as `cargo watch` if you have it installed, and restart the server on change.
- Flags for minification, source maps, a base path or environment-specific config.

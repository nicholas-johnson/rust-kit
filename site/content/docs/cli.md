---
title: CLI
description: The rustkit command — new, add, dev, check, routes, build and serve — and the serve and build commands every app has.
section: Reference
order: 6
---

The `rustkit` command creates apps, adds pages to them, and runs them while you work. It wraps Cargo rather than replacing it: pages still compile in an ordinary `cargo build` through your crate's [`build.rs`](/docs/compiler-api/), and your app is still a normal Rust binary whose `main` you own.

## Installing

The crates are not on crates.io yet. Install the command from the repository:

```bash
cargo install --git https://github.com/nicholas-johnson/rust-kit rustkit-cli
```

Inside a checkout of the repository you can also run it without installing, as `cargo run -p rustkit-cli -- <command>`.

## Commands

| Command | What it does |
|---|---|
| `rustkit new <path>` | Creates an app with a static page, a counter, a page that loads server data, a layout, a component and tests. |
| `rustkit add <kind> <name>` | Adds a page, layout or component to an app. |
| `rustkit dev` | Runs the app, and on every save rebuilds it, restarts it and updates open pages. |
| `rustkit check` | Compiles every page, layout and component and reports errors, without running Cargo. |
| `rustkit routes` | Lists every route, how it is served, and the JavaScript and CSS it ships. |
| `rustkit build [dir]` | Runs the app's own `build` command from a release build. |
| `rustkit serve [addr]` | Runs the app's own `serve` command from a release build. |

`rustkit --help` lists them, and `rustkit <command> --help` shows a command's options. Every command that acts on an app takes `-p <package>`. Without it, the command uses the only binary package in the workspace that depends on `rustkit`, or the one whose directory you are in.

### rustkit new

```bash
rustkit new my-app
```

```text
my-app/
├── Cargo.toml
├── build.rs              compiles src/routes into Rust and JavaScript
├── README.md
├── src/
│   ├── lib.rs            AppState and app()
│   ├── main.rs           `serve [addr]` and `build [dir]`
│   ├── sources.rs        the `greeting` resolver
│   ├── lib/Card.rk       a component
│   └── routes/
│       ├── layout.rk     navigation and site-wide styles
│       ├── index.rk      prerendered, ships no JavaScript
│       ├── counter.rk    `$state` and click handlers
│       └── hello.rk      loads `greeting` on the server
└── tests/pages.rs        requests against the real router
```

| Option | Meaning |
|---|---|
| `--name <name>` | The package name. Defaults to the last part of the path. |
| `--tailwind` | Turns on [Tailwind](/docs/styling/), uses utility classes in the layout, and adds a `package.json` for the Tailwind CLI. Run `npm install` before the first build. |
| `--rustkit-path <dir>` | Depends on a local checkout of the repository instead of Git. |
| `--git <url>` | Depends on rust-kit from this Git URL. |

The path must not exist or must be empty. When it is inside a Cargo workspace, the app is added to the workspace's `members`. When that workspace is the rust-kit repository itself, the app depends on the crates by path, so it builds against your local changes.

### rustkit add

```bash
rustkit add page about           # src/routes/about.rk          → /about
rustkit add page blog/[slug]     # src/routes/blog/[slug].rk    → /blog/{slug}
rustkit add layout blog          # src/routes/blog/layout.rk
rustkit add component UserCard   # src/lib/UserCard.rk
```

`add page /` creates the home page, and `add layout /` the app-wide layout. Routes follow the same rules as the compiler (see [Pages and routes](/docs/pages-and-routes/)): at most one `[param]` segment, and `layout` is not a page name. `add` refuses to overwrite a file, and refuses a page whose route an `index.rk` already serves. Component names are PascalCase.

### rustkit dev

```bash
rustkit dev
```

| Option | Default | Meaning |
|---|---|---|
| `--addr <host:port>` | `127.0.0.1:3000` | Where to listen. |
| `--release` | off | Build with the release profile. |
| `--offline` | off | Pass `--offline` to Cargo. |
| `-- <args>` | `serve` | Arguments for the app. |

`rustkit dev` builds the app and runs it. Then it watches the workspace, and when a file changes it rebuilds, restarts and updates the pages you have open. A stylesheet-only change swaps the `<link>` and leaves everything else alone. Other changes update the page node by node and carry `$state` over. The dev process owns the listening socket, so requests made during a restart wait instead of failing, and sessions are saved under `target/rustkit-dev/`, so you stay signed in. A failed build keeps the last good one running and shows the compiler's error over the page.

The app must bind with `rustkit::dev::listener(addr)`. Apps made by `rustkit new` do; elsewhere it replaces `tokio::net::TcpListener::bind`.

### rustkit check

```bash
rustkit check
```

Runs the page compiler directly on `src/routes` and `src/lib`, and prints the same diagnostics as a build. It finishes in milliseconds because it does not compile Rust. Use `cargo check` for that. Without `-p` it checks every app in the workspace. It exits with status 1 if any page fails.

### rustkit routes

```text
$ rustkit routes
route     mode      JS bytes CSS bytes
/counter  request       8178       541
/hello    request          0       432
/         static           0       573
```

`static` routes are prerendered once; `request` routes render per request. JS bytes are the page module plus the shared runtime, uncompressed. CSS bytes leave out the Tailwind stylesheet. See [Performance](/docs/performance/#javascript-per-route).

### rustkit build and rustkit serve

```bash
rustkit build dist
```

```bash
rustkit serve 0.0.0.0:8080
```

These build the app with `--release` and run its own `build [dir]` and `serve [addr]` commands. Pass `--debug` to skip the release profile. They work with any app whose `main` has those two commands, which includes apps made by `rustkit new` and the account demo.

## The app's own serve and build

Your app's `main.rs` decides what commands it has. This is the one `rustkit new` writes:

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
            println!("{:<12} {:<8} {:>10} {:>10}", "route", "mode", "JS bytes", "CSS bytes");
            for r in app.routes() {
                let mode = if r.prerender { "static" } else { "request" };
                println!("{:<12} {:<8} {:>10} {:>10}", r.path, mode, r.js_bytes, r.css_bytes);
            }
            println!("\nwrote {} files to {}", report.files.len(), out.display());
        }
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            // Under `rustkit dev` this adopts the dev server's socket instead.
            let listener = rustkit::dev::listener(addr).await.expect("bind");
            let local = listener.local_addr().expect("local address");
            let app = my_app::app(!local.ip().is_loopback());
            let router = app.into_router().await.expect("prerender failed");
            println!("listening on http://{local}");
            axum::serve(listener, router).await.unwrap();
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; use `serve [addr]` or `build [dir]`");
            std::process::exit(2);
        }
    }
}
```

`serve` marks the session cookie `Secure` unless it listens on a loopback address. Browsers refuse `Secure` cookies over plain HTTP, so on a LAN address signing in appears not to work; see [Troubleshooting](/docs/troubleshooting/#signing-in-does-nothing-on-plain-http).

### What the two commands rely on

| Call | Behaviour to know |
|---|---|
| `App::into_router()` | Prerenders every `prerender` route first and returns `Err` if any of them fails, for example because it read the session. |
| `App::build(dir)` | Writes prerendered routes as `<path>/index.html` (`/` becomes `index.html`) and every asset under `_rk/assets/`. Existing files are overwritten, not removed. Returns a [`BuildReport`](/docs/runtime-api/#compiled-output-types) listing the relative paths. |
| `App::routes()` | The compiled routes with `path`, `prerender`, `js_bytes` and `css_bytes`. |

The binary built by `cargo build --release` contains every page and asset, so `serve` needs nothing else at run time. The output of `build` is useful when a CDN or another web server should serve the static pages and assets directly; see [Deployment](/docs/deployment/).

## Commands in the repository

All of these run from the repository root.

| Command | What it does |
|---|---|
| `cargo test` | Runs the compiler, runtime and CLI unit tests, and the examples' acceptance and parity tests. |
| `cargo run -p account-demo -- serve [addr]` | Serves the demo app. It imitates a 25 ms round trip to its accounts service. Sign in at `/login` as Alice or Bob. |
| `cargo run -p rustkit-cli -- dev -p account-demo` | The demo under `rustkit dev`. |
| `cargo run -p site -- build [dir]` | Builds this documentation site, compiling every `rk` sample and checking every link. |

## Things that do not exist yet

- Flags for minification, source maps, a base path or environment-specific config.
- Windows support. The `rustkit` command builds only on Unix (macOS and Linux), because `rustkit dev` hands its socket to the app in a way that needs Unix.

---
title: Project structure
description: The files a Rust-kit app is made of, which names are fixed, and where the generated code and assets end up.
section: Start here
order: 3
---

A Rust-kit app is an ordinary Cargo crate with a build script. The only optional config file is `rustkit.toml` in the crate root. This page describes each piece, using the layout from [Getting started](/docs/getting-started/) and the bundled `examples/account-demo`.

## The layout

```text
my-app/
├── Cargo.toml          rustkit + rustkit-compiler (build) dependencies
├── rustkit.toml        optional: css_scope = false turns CSS scoping off
├── build.rs            calls rustkit_compiler::build_script()
├── src/
│   ├── main.rs         server entry point (or lib.rs + main.rs)
│   ├── sources.rs      crate::sources: data source resolvers and views
│   ├── actions.rs      form action handlers (any module name)
│   ├── lib/            shared components, imported as $lib/….rk
│   │   └── Header.rk
│   └── routes/         one .rk file per page
│       ├── index.rk        → /
│       ├── login.rk        → /login
│       └── account/
│           └── index.rk    → /account
└── tests/
    └── acceptance.rs   tests against the real router
```

Only three names are fixed: the `src/routes` directory, `crate::AppState`, and `crate::sources`. Everything else is your choice.

## `Cargo.toml`

Two dependencies matter:

```toml
[dependencies]
rustkit = { git = "https://github.com/nicholas-johnson/rust-kit" }

[build-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

`rustkit-compiler` only runs at build time and has no dependencies of its own. `rustkit` is linked into your binary. The generated code reaches `serde_json` and `tokio::try_join!` through `rustkit`, so you do not need `serde_json` yourself unless your own code uses it. You will want `serde` (for views), `tokio`, and `axum` (to serve the router).

## `build.rs`

```rust title=build.rs
fn main() {
    rustkit_compiler::build_script();
}
```

`build_script()` is a convenience wrapper. It reads `src/routes` relative to `CARGO_MANIFEST_DIR`, writes into `OUT_DIR`, and assumes `crate::AppState` and `crate::sources`. On a compile error it prints every page's diagnostics and exits, which fails the Cargo build.

If you need different paths or names, `rustkit_compiler::compile(&Config { … })` takes them explicitly. See [Compiler API](/docs/compiler-api/).

## `rustkit.toml`

Optional. `build_script()` reads it from the crate root and tells Cargo to rebuild when it changes. The only key is `css_scope`:

```toml
css_scope = false
```

Scoping stays on when the file is missing, when the key is omitted, or when the value is `true`. Unknown keys and any other value fail the build. See [Styling](/docs/styling/#styles-are-scoped-to-the-file).

## `src/routes/`

Every `.rk` file under this directory, at any depth, becomes one route. Files with any other extension are ignored, so you can keep notes or fixtures next to pages without affecting the build. The mapping from file path to URL is covered in [Pages and routes](/docs/pages-and-routes/).

Each page is a single file with up to four parts, in any order at the top level:

```rk title=anatomy.rk partial
<script context="module">
  export const prerender = true;    // optional: build-time rendering
</script>

<script>
  import { account } from "$sources"; // data sources this page uses
  let open = $state(false);           // browser state
</script>

<title>Page title</title>              <!-- hoisted into <head> -->

<h1>Markup with {expressions}</h1>     <!-- the template -->

<style>
  h1 { color: rebeccapurple; }        /* one stylesheet per page */
</style>
```

A `layout.rk` in a folder wraps the pages there; see [Pages and routes](/docs/pages-and-routes/#layouts). Shared components live in `src/lib` and are imported with `import Header from "$lib/Header.rk"`; see [Components](/docs/pages-and-routes/#components). `$lib` is the directory next to `src/routes`. It is not a route.

## `crate::AppState`

The generated renderers are typed as `fn render(ctx: &RequestContext<crate::AppState>, param: Option<&str>)`. `AppState` is your type, holding whatever your pages and actions need: service clients, connection pools, configuration. `param` is the route's single dynamic segment, or `None` on a fixed path.

```rust
pub struct AppState {
    pub accounts_service: upstream::AccountsService,
}
```

`RequestContext<AppState>` dereferences to `AppState`, so a resolver writes `ctx.accounts_service.get(…)` directly. The `App` holds the state in an `Arc` and shares it with every request.

## `crate::sources`

This module holds one async function per data source. The function name is the source name that pages import:

```rust title=src/sources.rs
use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub display_name: String,
}

pub async fn account(ctx: &Ctx) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(&user.id).await?;
    Ok(AccountView { display_name: record.display_name })
}
```

A page that says `import { account } from "$sources"` and uses it in a `<DataSource>` causes the generated code to call `crate::sources::account(ctx)`. If the function is missing or has the wrong signature, `rustc` reports it in the generated file.

Views must serialize with `#[serde(rename_all = "camelCase")]`. Templates read JSON-style names like `data.displayName`, and the compiler converts each segment to snake case (`display_name`) to access the Rust field. The browser sees the serialized JSON, so the two only agree if the serde names are camelCase. Read more in [Data sources](/docs/data-sources/).

The generated code only refers to `crate::sources` for sources that pages actually use, so an app with no sources compiles without the module. Keeping an empty `pub mod sources {}` is harmless and saves a confusing error the day you add your first `<DataSource>`.

## Where the generated code goes

`include!` brings the compiler's output into your crate. You can put it anywhere; the demo wraps it in a module:

```rust title=src/lib.rs
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}
```

Inside `OUT_DIR` the compiler writes:

| Path | Contents |
| --- | --- |
| `rustkit_generated.rs` | One `page_<name>` module per route with `render` and `render_boxed`, the `CLIENT_SOURCES` list, a `source_api` dispatcher, and `manifest()`. |
| `rk_assets/<name>.<hash>.js` | The browser module for each interactive page. |
| `rk_assets/runtime.<hash>.js` | The shared browser runtime, if any page is interactive. |
| `rk_assets/<name>.<hash>.css` | The stylesheet for each page with a non-empty `<style>`. |

Assets are embedded into your binary with `include_str!`, so a release build is a single executable with no asset directory to deploy. They are served under `/_rk/assets/` with a year-long immutable cache header; the hash in the name changes whenever the content does. See [Generated code](/docs/generated-code/) for a walk through the generated file.

## Library plus binary

The demo splits its crate into `lib.rs` (state, sources, actions, and an `app()` function that returns a configured `App`) and a thin `main.rs` that parses arguments and serves. That split lets integration tests in `tests/` build the same app and drive the router in-process:

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

`crate::AppState` then refers to the library's root, which is where the generated code expects it. This layout is recommended for anything beyond a toy. See [Testing](/docs/testing/).

## Actions

Form actions are plain async functions registered with `App::action(path, handler)`. They do not have to live in any particular module; the demo uses `src/actions.rs`. Each takes a `RequestContext` and the submitted `Form` and returns a `Redirect` or an `Error`. See [Form actions](/docs/form-actions/).

## What is not part of a project yet

- No config file, environment variable, or CLI flag changes how pages compile.
- No `public/` or `static/` directory is served automatically. If you need static files, add an axum route or a `tower-http` service to the router yourself.
- No generator creates this layout for you.

All of these are tracked on the [roadmap](/docs/roadmap/).

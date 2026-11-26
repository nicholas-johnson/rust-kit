---
title: Your first page
description: Set up a Rust-kit project by hand, write one static page, run it, and confirm it ships no JavaScript.
order: 1
level: Beginner
duration: 15 min
---

## What you'll build

You'll create a small Rust crate called `hello-rk` that serves one page at `http://127.0.0.1:3000/`. The page is written as a `.rk` file: a template, plus an optional `<script>` and `<style>`. At build time the Rust-kit compiler turns it into a native Rust render function. When you view the page source in the browser, you'll see plain HTML, a stylesheet link, and no `<script>` tag at all.

Along the way you'll see every piece a Rust-kit app needs. You'll write each file yourself. There are only five of them, and each one is short. (Once you know the pieces, [`rustkit new`](/docs/cli/#rustkit-new) writes them for you.)

## Prerequisites

- Rust 1.85 or newer (the project uses the 2024 edition). Check with `rustc --version`.
- A terminal and a browser.
- `curl` is handy for looking at raw responses, but optional.

You don't need to know Axum or Tokio. You'll use them for about five lines.

## Step 1: Create the crate

Rust-kit apps are ordinary Cargo packages. Create one:

```bash
cargo new hello-rk
cd hello-rk
```

You'll end up with this layout:

```text
hello-rk/
├── Cargo.toml
├── build.rs              compiles the pages at build time
└── src/
    ├── lib.rs            app state, data sources, and the generated routes
    ├── main.rs           starts the HTTP server
    └── routes/
        └── index.rk      the page at /
```

Splitting the app into a library (`lib.rs`) and a small binary (`main.rs`) is a habit worth starting now. Later tutorials reuse the library from tests and from extra commands.

## Step 2: Add the dependencies

Replace `Cargo.toml` with this:

```toml title=Cargo.toml
[package]
name = "hello-rk"
version = "0.1.0"
edition = "2024"

[dependencies]
rustkit = { git = "https://github.com/nicholas-johnson/rust-kit" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

`rustkit` is the runtime your app links against: request context, sessions, CSRF checks, and the router. `rustkit-compiler` is only needed while building, so it goes under `[build-dependencies]`. Axum and Tokio serve HTTP. You won't use Serde in this tutorial, but every later tutorial does, so it's here from the start.

## Step 3: Compile pages in a build script

Create `build.rs` next to `Cargo.toml`:

```rust title=build.rs
fn main() {
    rustkit_compiler::build_script();
}
```

Cargo runs this before it compiles your crate. `build_script()` finds every `.rk` file under `src/routes/`, compiles each one, and writes the results into Cargo's `OUT_DIR`:

- `rustkit_generated.rs`: one Rust module per page with a `render` function, plus a `manifest()` function that lists the routes.
- `rk_assets/`: content-hashed CSS and JavaScript files.

If a page has an error, the build stops and prints a diagnostic that points at the line in your `.rk` file.

## Step 4: Write the library

Replace the contents of `src/lib.rs` (create it if `cargo new` didn't) with this:

```rust title=src/lib.rs
//! hello-rk: a Rust-kit app.

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

/// Data sources live here. There are none yet.
pub mod sources {}

/// Shared application state. Every request can read it.
pub struct AppState {}

pub fn app() -> rustkit::App<AppState> {
    rustkit::App::new(generated::manifest(), AppState {})
        // Plain http on localhost: don't mark the session cookie `Secure`.
        .secure_cookies(false)
}
```

Three names here are fixed, because the generated code refers to them by path:

- `crate::AppState`: your application state. Every render function receives a request context that dereferences to it.
- `crate::sources`: the module that holds data-source resolvers. It's empty for now; you'll fill it in [Loading data on the server](/tutorials/server-data/).
- `generated`: the module that pulls in the compiler's output. `generated::manifest()` returns every route and asset.

`App::new` turns the manifest plus your state into an application. Session cookies are marked `Secure` by default, which browsers ignore over plain `http://`. While you develop locally, `.secure_cookies(false)` turns that off. This page doesn't use a session, but later ones do, so it's simplest to set it now. The [Deploying](/tutorials/deploying/) tutorial turns it back on.

## Step 5: Start the server

Replace `src/main.rs`:

```rust title=src/main.rs
#[tokio::main]
async fn main() {
    let router = hello_rk::app().into_router().await.expect("failed to build the router");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.expect("port 3000 is in use");
    println!("listening on http://127.0.0.1:3000");
    axum::serve(listener, router).await.unwrap();
}
```

`into_router()` returns an ordinary `axum::Router`. It's `async` because it renders any prerendered pages up front (you'll meet those in [A static blog](/tutorials/static-blog/)). From there it's standard Axum: bind a listener and serve.

## Step 6: Write the page

Create `src/routes/index.rk`:

```rk title=index.rk
<title>Hello, Rust-kit</title>

<main>
  <h1>Hello, Rust-kit</h1>
  <p>This page was rendered by a Rust function that the compiler generated from <code>src/routes/index.rk</code>.</p>
  <p>It has no state and no event handlers, so it ships no JavaScript.</p>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  h1 { color: #b7410e; }
</style>
```

A page file is mostly HTML. The `<title>` element becomes the document title. The `<style>` block becomes a stylesheet. Everything else is the page body. You don't write `<html>`, `<head>` or `<body>`; Rust-kit wraps every page in a standard document for you.

Expand **Generated Rust** below the sample to see what the compiler makes of it: an `async fn render` that pushes your markup into a `String` and wraps it in a document. Notice that there is no **Generated JavaScript** section. This page has nothing interactive, so the compiler emits no browser module.

The file's path decides its URL. `src/routes/index.rk` is served at `/`. A file at `src/routes/about.rk` would be `/about`, and `src/routes/docs/intro.rk` would be `/docs/intro`.

## Step 7: Run it

```bash
cargo run
```

The first build downloads and compiles dependencies, which takes a minute. When you see `listening on http://127.0.0.1:3000`, open [http://127.0.0.1:3000/](http://127.0.0.1:3000/) in your browser. You should see the heading in rust orange and two paragraphs.

> [!NOTE]
> While your app has no client-loaded data sources, `cargo` prints `warning: unused variable: ctx` from inside `rustkit_generated.rs`. It's harmless: a generated function doesn't use one of its parameters yet. It goes away once a page loads data in the browser, which you'll do in [An activity feed](/tutorials/activity-feed/).

Now look at the raw response:

```bash
curl -i http://127.0.0.1:3000/
```

You should get something like this (the hash in the stylesheet name depends on your CSS):

```html
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
server-timing: sources;dur=0.000, render;dur=0.009
cache-control: no-cache
content-length: 496

<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Hello, Rust-kit</title>
<link rel="stylesheet" href="/_rk/assets/index.553c446e.css">
</head>
<body>

<main>
  <h1>Hello, Rust-kit</h1>
  ...
</main>

</body>
</html>
```

Try a path that doesn't exist, such as `http://127.0.0.1:3000/nope`. Rust-kit answers with a small `404` page.

## Step 8: View the source

In the browser, open **View Page Source** (Ctrl+U or Cmd+Option+U). Search for `<script`. There isn't one. The page is complete HTML the moment it arrives, and the only other request the browser makes is for the stylesheet.

That's the default in Rust-kit. JavaScript appears only when a page needs it: when it has `$state`, an event handler, or data it loads in the browser. You'll add the first two in the next tutorial and watch a small module appear.

## What just happened

When you ran `cargo run`, four things happened in order:

1. **Cargo ran `build.rs`.** The compiler parsed `index.rk`, checked it, and generated a Rust module named `page_index` with a `render` function. It also wrote `index.<hash>.css` with your styles, and a `manifest()` function that lists the route `/` and the asset. If you're curious, find the generated file with `find target -name rustkit_generated.rs`. It's plain Rust, so you can read it.
2. **Cargo compiled your crate.** `include!` pasted the generated code into `hello_rk::generated`, so the page is compiled and type-checked with the rest of your program. There's no template interpreter at run time.
3. **`into_router()` built an Axum router.** It has a `GET` route for each page, a route for each asset under `/_rk/assets/`, and a 404 fallback. Assets get `Cache-Control: public, max-age=31536000, immutable`, which is safe because their names change whenever their content does.
4. **Each request called `render`.** The response carries two headers worth knowing about. `Server-Timing` splits the time spent waiting for data (`sources`) from the time spent writing HTML (`render`); you'll use it in [Measuring performance](/tutorials/measuring-performance/). `Cache-Control: no-cache` tells caches to revalidate, because a page rendered per request might change.

### When the compiler says no

The compiler checks your markup before Rust ever sees it. For example, if you close the wrong element, the build fails and points at the problem:

```rk fail title=index.rk
<main>
  <p>Hello, Rust-kit
</main>
```

`cargo build` prints the same diagnostic, with the file name and line, and stops. Fix the file and build again. The [diagnostics reference](/docs/diagnostics/) lists every error the compiler can report.

## Add a second page (optional)

To check that you understand the routing rule, create `src/routes/about.rk`:

```rk title=about.rk
<title>About</title>

<main>
  <h1>About</h1>
  <p>This page lives at <code>/about</code> because its file is <code>src/routes/about.rk</code>.</p>
  <p><a href="/">Back home</a></p>
</main>
```

Stop the server with Ctrl+C, run `cargo run` again, and open [http://127.0.0.1:3000/about](http://127.0.0.1:3000/about). Every page is its own file. To share the navigation, add a `layout.rk` beside them; see [Pages and routes](/docs/pages-and-routes/#layouts).

## Next steps

- [A counter](/tutorials/counter/) adds `$state` and a click handler to a page, and shows you the small browser module the compiler generates.
- [Project structure](/docs/project-structure/) explains each file in a Rust-kit project in more detail.
- [Template syntax](/docs/template-syntax/) covers everything you can write in a page.
- [How it works](/docs/how-it-works/) follows a page from `.rk` file to HTTP response.

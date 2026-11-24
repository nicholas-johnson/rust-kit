---
title: A static blog
description: Prerender pages at build time, add a build subcommand that writes them to a directory with App::build, and serve the output from any static file server.
order: 7
level: Beginner
duration: 25 min
---

## What you'll build

A small blog with a post list, two posts and an about page. Every page is *prerendered*: rendered once, ahead of time, instead of on each request. You'll add a `build` command that writes the whole site to a `dist/` folder, then serve that folder with a plain static file server. No Rust process needs to run in production.

The post list comes from a data source, so you'll also see that prerendered pages can still load data, as long as the data doesn't depend on who's asking.

## Prerequisites

- [Your first page](/tutorials/your-first-page/), for project setup.
- [Loading data on the server](/tutorials/server-data/), for sources.
- Python 3 for the static server in Step 7. Any static file server works.

## Step 1: Create the project

```bash
cargo new blog
cd blog
mkdir -p src/routes/posts
```

```toml title=Cargo.toml
[package]
name = "blog"
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

```rust title=build.rs
fn main() {
    rustkit_compiler::build_script();
}
```

## Step 2: Keep the post list in AppState

```rust title=src/lib.rs
//! blog: a prerendered site.

pub mod sources;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

pub struct Post {
    pub slug: &'static str,
    pub title: &'static str,
    pub date: &'static str,
    pub summary: &'static str,
}

pub struct AppState {
    pub posts: Vec<Post>,
}

pub fn app() -> rustkit::App<AppState> {
    let state = AppState {
        posts: vec![
            Post {
                slug: "second-post",
                title: "Prerendering in one line",
                date: "2026-11-20",
                summary: "Why the module script is all it takes.",
            },
            Post {
                slug: "hello-world",
                title: "Hello, world",
                date: "2026-11-12",
                summary: "The first post on a site with no JavaScript.",
            },
        ],
    };
    rustkit::App::new(generated::manifest(), state).secure_cookies(false)
}
```

The list is the blog's index: titles, dates and summaries, newest first. The posts themselves are pages, which you'll write in Step 4.

## Step 3: A source for the index

```rust title=src/sources.rs
//! Data sources. These run at build time for prerendered pages, so they read
//! only application state, never the session.

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostListView {
    pub posts: Vec<PostSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostSummary {
    pub href: String,
    pub title: String,
    pub date: String,
    pub summary: String,
}

pub async fn posts(ctx: &Ctx) -> Result<PostListView, Error> {
    Ok(PostListView {
        posts: ctx
            .posts
            .iter()
            .map(|p| PostSummary {
                href: format!("/posts/{}", p.slug),
                title: p.title.to_owned(),
                date: p.date.to_owned(),
                summary: p.summary.to_owned(),
            })
            .collect(),
    })
}
```

The resolver builds each post's URL (`href`), since the template can't join strings.

## Step 4: Write the pages

A page opts into prerendering with a *module script*, a second `<script>` element with `context="module"`. Create `src/routes/index.rk`:

```rk title=index.rk
<script context="module">
  export const prerender = true;
</script>

<script>
  import { posts } from "$sources";
</script>

<title>My blog</title>

<header><a href="/">My blog</a> · <a href="/about">About</a></header>

<main>
  <h1>Posts</h1>
  <DataSource source={posts} load="server" let:data>
    {#each data.posts as post}
      <article>
        <h2><a href={post.href}>{post.title}</a></h2>
        <p><time datetime={post.date}>{post.date}</time> · {post.summary}</p>
      </article>
    {/each}
  </DataSource>
</main>

<style>
  body { font-family: Georgia, serif; max-width: 38rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.6; }
  header { font-family: system-ui, sans-serif; margin-bottom: 2rem; }
  time { color: #777; }
</style>
```

Write the line exactly as `export const prerender = true;`. The compiler looks for that text, so a variation such as `prerender = !false` won't count.

There are no route parameters yet, so each post is its own file. The folder becomes part of the URL: `src/routes/posts/hello-world.rk` is served at `/posts/hello-world`.

```rk title=posts/hello-world.rk
<script context="module">
  export const prerender = true;
</script>

<title>Hello, world · My blog</title>

<header><a href="/">My blog</a> · <a href="/about">About</a></header>

<article>
  <h1>Hello, world</h1>
  <p><time datetime="2026-11-12">2026-11-12</time></p>
  <p>This is the first post. It was rendered to HTML once, at build time, by a Rust function the compiler generated from this file.</p>
  <p>It has no state and no event handlers, so the page loads no JavaScript at all.</p>
</article>

<style>
  body { font-family: Georgia, serif; max-width: 38rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.6; }
  header { font-family: system-ui, sans-serif; margin-bottom: 2rem; }
  time { color: #777; }
</style>
```

```rk title=posts/second-post.rk
<script context="module">
  export const prerender = true;
</script>

<title>Prerendering in one line · My blog</title>

<header><a href="/">My blog</a> · <a href="/about">About</a></header>

<article>
  <h1>Prerendering in one line</h1>
  <p><time datetime="2026-11-20">2026-11-20</time></p>
  <p>A page becomes static when its module script says <code>export const prerender = true;</code>.</p>
  <p>The server renders it once when it starts, and <code>build</code> writes it to a file.</p>
</article>

<style>
  body { font-family: Georgia, serif; max-width: 38rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.6; }
  header { font-family: system-ui, sans-serif; margin-bottom: 2rem; }
  time { color: #777; }
</style>
```

Without layouts, the header and styles repeat in every page. Shared layouts and components are on the [roadmap](/docs/roadmap/).

Finally, `src/routes/about.rk` shows that prerendered doesn't mean static behaviour. This page has a toggle:

```rk title=about.rk
<script context="module">
  export const prerender = true;
</script>

<script>
  let showMore = $state(false);
</script>

<title>About · My blog</title>

<header><a href="/">My blog</a> · <a href="/about">About</a></header>

<main>
  <h1>About</h1>
  <p>A small blog built with Rust-kit.</p>
  <button onclick={() => showMore = !showMore}>
    {#if showMore}Less{:else}More{/if}
  </button>
  {#if showMore}
    <p>This paragraph is rendered in the browser. The page is still prerendered; only this toggle needs JavaScript.</p>
  {/if}
</main>

<style>
  body { font-family: Georgia, serif; max-width: 38rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.6; }
  header { font-family: system-ui, sans-serif; margin-bottom: 2rem; }
  button { font: inherit; }
</style>
```

## Step 5: Add a build command

Replace `src/main.rs` with a small command-line interface: `build [dir]` writes the site, and `serve [addr]` runs it as before.

```rust title=src/main.rs
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("build") => {
            let out = PathBuf::from(args.get(1).map_or("dist", String::as_str));
            let app = blog::app();
            let report = app.build(&out).await.expect("build failed");
            for file in &report.files {
                println!("  {file}");
            }
            println!("wrote {} files to {}", report.files.len(), out.display());
        }
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            let router = blog::app().into_router().await.expect("prerender failed");
            let listener = tokio::net::TcpListener::bind(addr).await.expect("could not bind");
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

`App::build(dir)` renders every prerendered route and writes it to `dir`, along with every CSS and JavaScript asset. It returns a `BuildReport` whose `files` lists what it wrote, relative to `dir`.

## Step 6: Build the site

```bash
cargo run -- build dist
```

You should see:

```text
  about/index.html
  index.html
  posts/hello-world/index.html
  posts/second-post/index.html
  _rk/assets/about.933fa345.js
  _rk/assets/about.3b33ceed.css
  _rk/assets/index.c4e93a66.css
  _rk/assets/posts_hello_world.c4e93a66.css
  _rk/assets/posts_second_post.c4e93a66.css
  _rk/assets/runtime.70c435b7.js
wrote 10 files to dist
```

The hashes come from the file contents, so yours may differ. Each route `/x/y` becomes `x/y/index.html`, so any static host can serve it at a clean URL. Assets keep the `/_rk/assets/` paths the pages already link to. The three pages with identical CSS each get their own copy; that costs a few bytes, not correctness.

Open `dist/posts/hello-world/index.html` in an editor. It's the finished page: no `<script>` tag, a stylesheet link, and your text. `dist/about/index.html` has a `<script type="module">` for the toggle, and the module and shared runtime are in `_rk/assets/`.

## Step 7: Serve the output

Any static file server will do. Python ships one:

```bash
python3 -m http.server 8000 --directory dist
```

Open [http://127.0.0.1:8000/](http://127.0.0.1:8000/). Click through to a post and to **About**, and try the **More** button. Everything works with no Rust process running. When you follow a link to `/posts/hello-world`, Python redirects to `/posts/hello-world/` and serves `index.html`. Most static hosts behave the same way.

You can also run the Rust server, with `cargo run` (or `cargo run -- serve`), and visit [http://127.0.0.1:3000/](http://127.0.0.1:3000/). Prerendered routes are rendered once when `into_router()` runs, held in memory, and served with `Cache-Control: public, max-age=300`. They never call your resolvers again until you restart.

## Step 8: Know what can't be prerendered

A prerendered page is rendered once, for nobody in particular. Anything that depends on the visitor won't work. Rust-kit enforces part of that for you. Make the `posts` resolver call `ctx.require_user()?;` and run `cargo run -- build dist`. The build stops with:

```text
build failed: Custom { kind: Other, error: Internal("a prerendered route read the session; remove `prerender` or load this source on the client") }
```

That's the `expect` in your `build` command printing the error. `cargo run` without arguments fails the same way at startup, because `into_router()` prerenders too. Remove the `require_user` line before you continue.

> [!WARNING]
> Don't put a `<form method="post">` on a prerendered page. Rust-kit still inserts a CSRF field, but its token is generated at build time and isn't tied to any visitor's session, so every submission is rejected with "This form has expired". The compiler doesn't catch this yet. Keep forms on pages rendered per request.

Some other good rules of thumb:

- Prerender pages whose content changes when you deploy, not when your data changes.
- Pages that need fresh data, a signed-in user, or a form should be rendered per request. In a mixed app, `build` writes only the prerendered pages and the assets. The other routes still need the server.

## What just happened

The module script is read at compile time only. The compiler records `prerender: true` in the route's manifest entry and nothing from it reaches the browser. From there:

- **`App::build`** calls each prerendered route's `render` with a special build-time request context. That context has your `AppState` but no request and no session, which is why resolvers can read `ctx.posts` and why `require_user` fails loudly.
- **`into_router()`** does the same rendering at startup and serves the stored HTML. Because the page is identical for everyone, it's safe to mark it publicly cacheable.
- **JavaScript is still per page.** The index and posts ship none. The about page ships its module and the shared runtime, exactly as a request-time page would.

## Next steps

- [Prerendering](/docs/prerendering/) is the full reference for module scripts and `App::build`.
- [Caching](/docs/caching/) explains the `Cache-Control` header each kind of response gets.
- [Testing your app](/tutorials/testing-your-app/) shows how to assert that a page is prerendered and ships no JavaScript.
- [Deploying](/tutorials/deploying/) covers serving a mix of static and per-request pages in production.

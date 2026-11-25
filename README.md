<p align="center"><img src="site/static/logo.svg" alt="Rust-kit" width="360"></p>

# Rust-kit

**Rust renders. The browser responds.** Docs and tutorials: [rust-kit.com](https://rust-kit.com).

A working first milestone of the Rust-kit design proposal (November 2026).
You write a page as a template, a script, and a stylesheet. At build time,
`rustkit-compiler` turns it into a native Rust render function plus a small
browser module for the interactive parts. The server sends HTML that reads
correctly before any JavaScript runs.

```
crates/rustkit-compiler   .rk parser, expression checker, Rust + JS codegen, browser runtime
crates/rustkit            request context, sessions, CSRF, router, JS value semantics
examples/account-demo     the proposal's static article and account page
examples/test-pages       small single-feature pages, with server and Rust-vs-browser parity tests
site                      generator for rust-kit.com (docs, tutorials, landing page)
```

## Test pages

`examples/test-pages` has a small page for each feature: escaping,
number formatting, attributes, truthiness, comparisons (including UTF-16 string
order), loops, state, nested blocks, the handoff payload, client loading and
forms, plus a blog with nested layouts and a `[slug]` segment. Browse them with:

```bash
cargo run -p test-pages
```

`tests/pages.rs` checks what the server sends. `tests/parity.rs` checks the
framework's central promise: it loads each page's real browser module and the
real runtime in Node, renders every block, and requires the same HTML Rust
produced, including after state changes and for client-loaded data. It needs
`node` on `PATH`; it is skipped locally without it and required in CI.

The parity tests found one real divergence, now fixed: `NaN` and `Infinity`
used to render in Rust but reach the browser as `null`, because the handoff
payload is JSON. Rust now renders non-finite floats as `null` too, and the
`/nan` page keeps it covered.

## The website

`site/` turns `site/content/{docs,tutorials}/*.md` into rust-kit.com. Every
`rk` code block is compiled by the real compiler during the build; the
generated Rust and JavaScript appear under the sample, and a sample that stops
compiling fails the build. Internal links and anchors are checked too.

```bash
cargo run -p site -- build
```

```bash
python3 -m http.server 4180 --directory site/dist
```

`.github/workflows/pages.yml` runs the tests, builds the site and deploys it to
GitHub Pages on every push to `main`.

## Run it

```bash
cargo test
```

```bash
cargo run -p account-demo -- serve 127.0.0.1:3000
```

```bash
cargo run -p account-demo -- build dist
```

For development, run it under `rustkit dev` instead:

```bash
cargo run -p rustkit-cli -- dev -p account-demo --addr 127.0.0.1:3000
```

Then save a `.rk` page or any Rust file in the workspace. `rustkit dev`
rebuilds the app and restarts it, and open pages update in place:

- **Stylesheet-only changes** swap the `<link>`. Nothing else on the page is
  touched, so focus, typed text and open panels stay as they were.
- **Other changes** fetch the page again and update the body node by node, so
  unchanged elements keep focus and typed text. The running page module is
  stopped and the new one starts with the old `$state` and the data the page
  had loaded in the browser. That data shows until a fresh copy arrives,
  because the resolver may have changed. A block that re-renders because of
  restored state is rebuilt, so text typed inside it is lost.
- **When an update can't be done in place** (the page now redirects or
  errors, say), the page reloads and restores its scroll position and
  `$state`.

- **No dropped requests.** The dev process owns the listening socket and hands
  it to each new copy of the app, so requests made during a restart wait
  instead of failing. Apps adopt the socket with `rustkit::dev::listener(addr)`.
- **You stay signed in.** Sessions are saved under `target/rustkit-dev/`, so a
  restart doesn't sign you out.
- **Errors show in the browser.** If a build fails, the last good build keeps
  running and open pages show the compiler's diagnostic over the content. If
  nothing is running yet, the dev process serves a page with the error
  itself. Either way, the page updates once the build succeeds.
- Saving a file that doesn't change the binary doesn't restart anything.

Dev mode adds `/_rk/dev.js` and `/_rk/dev/events`, and it serves prerendered
pages with `no-cache`. It is on only when `rustkit dev` starts the app
(`RUSTKIT_DEV=1`); `serve` and `build` are unchanged.
`rustkit dev` also builds with `RUSTKIT_DEV=1`, which adds the hooks the dev
client needs to the page runtime. A plain `cargo build` leaves them out, and a
page whose runtime lacks them falls back to a full reload.

`serve` imitates a 25 ms round trip to the accounts service. Sign in at
`/login` as Alice or Bob (demo accounts with no password). `build` writes the
prerendered routes and assets to `dist/` and prints JavaScript and CSS bytes per route.

## How a page compiles

`examples/account-demo/src/routes/account.rk` uses the proposal's account page
exactly as written. For that page the compiler emits:

- **Rust**: `render(ctx, param)` loads every `load="server"` source at once with
  `try_join!` (each source once per request), escapes every bound value, and
  writes the HTML.
- **Blocks**: anything that reads `$state` or loads on the client becomes a
  block. Rust renders its initial state between `<!--rk:bN-->` markers. The
  browser module gets a render function for it, plus its dependencies.
- **Handoff payload**: if a browser-rendered block reads server data, the
  compiler serializes only the fields it reads. Here that is
  `{"s":{"account":{"displayName":"…"}}}`. The JSON is escaped so it can sit
  safely inside an HTML `<script>`.
- **Endpoints**: only sources some page loads with `load="client"` get
  `GET /_rk/source/{name}`. That endpoint runs the same resolver with the same
  session. A client source that takes a route parameter also receives it as a
  query string (`?slug=notes`). `account` is server-only, so it has no endpoint.
- **Assets**: a content-hashed page module (662 B gzipped for the account page),
  the shared runtime (2.8 KB gzipped), and a stylesheet. A page with no state
  and no handlers gets no module and no `<script>` tag.

### Tailwind CSS

Tailwind is off by default. To turn it on, change the crate's `build.rs` to:

```rust
fn main() {
    rustkit_compiler::Build::new().tailwind().run();
}
```

Then use utility classes in any `.rk` template. The build runs the Tailwind v4
CLI once and writes one minified, content-hashed stylesheet. Every page links
that stylesheet before its own `<style>` block, so page styles still win.
`build` reports the stylesheet in each route's CSS bytes.

- **CLI**: the build uses the first of these that it finds:
  `$RUSTKIT_TAILWIND`, the crate's `node_modules/.bin/tailwindcss` (from
  `npm install -D @tailwindcss/cli`), or the
  [standalone `tailwindcss`](https://tailwindcss.com/blog/standalone-cli) on
  `PATH`. If none is found, the build fails and says how to install one.
  An npm-installed CLI outside the crate works for the default input. A
  `src/tailwind.css` that writes `@import "tailwindcss"` needs the CLI
  installed in the crate, or the standalone CLI.
- **Input**: with no `src/tailwind.css`, the build uses
  `@import "tailwindcss" source(none); @source "./src/routes";`. Add
  `src/tailwind.css` to use `@theme`, plugins, or more `@source` paths.
- Class names must appear whole in the template, as Tailwind requires. Use
  `"bg-red-500"`, not `"bg-" + color`.

### The language boundary

Expressions in templates are limited to field reads, `.length`, literals,
`!`, `&&`, `||`, `===`, `!==`, `<`, `<=`, `>`, and `>=`. In Rust they are
evaluated through `rustkit::js::Val`, which follows JavaScript's rules for
truthiness, strict equality, comparing UTF-16 strings, `.length`, and number
formatting. Both targets therefore produce the same text. A call is rejected at
build time:

```
error: `formatDate(…)` cannot be rendered by the Rust server renderer
 --> src/routes/account.rk:3:15
  |
3 |   <h1>Joined {formatDate(data.joined)}</h1>
  |               ^
  = help: Rust has to produce exactly the same text as the browser. Compute this value in the source's resolver and expose it as a field on the view …
```

Event handlers (`onclick={…}`) can be any JavaScript, because they run only in
the browser. Assigning to a `$state` variable re-renders the blocks that read it.

## Acceptance tests

`examples/account-demo/tests/acceptance.rs` runs against the real router:

1. The article prerenders to `article/index.html` with no `<script>`, and
   `js_bytes == 0`.
2. A signed-in `GET /account` contains `<h1>Hello, Alice Liddell</h1>` in raw
   HTML and makes exactly one upstream read. The payload holds only
   `displayName`, never the upstream email. The module has no way to fetch the
   account again.
3. Alice and Bob each get only their own data, through both the HTML path and
   the API path. An anonymous request gets 401 and a cross-site request gets 403.
4. A profile form post with a CSRF token redirects to `/account`, and the next
   render shows the new name. Posts with a missing token, a foreign `Origin`, or
   an empty name are rejected.

The browser half was checked by hand in Chromium:

- Attaching the handlers made no `/_rk/source/account` request.
- The editor opened with the payload's name.
- The first "Show recent activity" click made exactly one `/_rk/source/activity`
  request.
- Saving through the real form updated the greeting.

## Security defaults

- Sessions use an opaque 256-bit cookie (`HttpOnly`, `SameSite=Lax`, and `Secure`
  unless the app turns it off; the demo does so on loopback) with server-side
  state.
- The session id rotates at sign-in.
- Every action checks the CSRF token with a constant-time comparison and
  checks `Origin`.
- Every `<form method="post">` gets its CSRF token automatically, whether Rust
  or the browser renders the form.
- Personalised responses are `private, no-store` with `Vary: Cookie`.
- Every request-time page carries a `Server-Timing` header with `sources;dur=…` and
  `render;dur=…`, which separates upstream latency from render CPU time.

### Layouts and one route parameter

`layout.rk` in a folder wraps the pages in that folder and is not itself a
URL. Nested folders nest layouts, outermost first. The layout marks where the
page goes with `<slot />`. Its `<style>` is concatenated ahead of the page's,
so the page's rules win. The document title is the page's, or the nearest
layout's when the page has none.

`blog/[slug].rk` is `/blog/{slug}`. The parameter is part of the page: the
generated module has `struct Params { slug: String }`, `{slug}` is in scope,
and a resolver takes `slug: &str` when every file that imports it sits on that
same segment. A source also imported from a fixed path stays `(ctx)`; mixing
the two is a compile error. One `[param]` per route. A dynamic page cannot be
prerendered.

## Not in this milestone

- **Upstream service**: the accounts service is an in-memory stand-in with
  latency and call counters. A real HTTP client would replace
  `examples/account-demo/src/upstream.rs` and nothing else.
- **Script parsing**: `<script>` is parsed line by line. `import … from "$sources"`
  and `let x = $state(literal)` must each be on their own line.
- **State**: only primitive literals (string, number, boolean, null), so no
  arrays. `{#each}` over state is diagnosed.
- **Granularity**: an update re-renders a whole block. There are no
  per-attribute patches and no keyed lists, so focus inside a re-rendered block
  is lost.
- **Handlers in server-rendered loops**: a handler inside a server-rendered
  `{#each}` can't read the loop item yet. This is diagnosed.
- **Sources**: the only input a resolver can take is the route's one parameter.
  There is no query string, no argument from the page, and no client refresh.
  A failed client load shows a Retry button.
- **Not built yet**: CSS scoping, rest parameters (`[...slug]`), optional
  segments (`[[slug]]`), more than one `[param]` per route, content
  collections, persistent session storage, and component imports.
- **Views**: they must serialize with `#[serde(rename_all = "camelCase")]` so
  that their JSON names match what templates read.

# Rust-kit

A working first milestone of the Rust-kit design proposal (November 2026).
You write a page as a template, a script, and a stylesheet. At build time,
`rustkit-compiler` turns it into a native Rust render function plus a small
browser module for the interactive parts. The server sends HTML that reads
correctly before any JavaScript runs.

```
crates/rustkit-compiler   .rk parser, expression checker, Rust + JS codegen, browser runtime
crates/rustkit            request context, sessions, CSRF, router, JS value semantics
examples/account-demo     the proposal's static article and account page
```

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

`serve` imitates a 25 ms round trip to the accounts service. Sign in at
`/login` as Alice or Bob (demo accounts with no password). `build` writes the
prerendered routes and assets to `dist/` and prints JavaScript and CSS bytes per route.

## How a page compiles

`examples/account-demo/src/routes/account.rk` uses the proposal's account page
exactly as written. For that page the compiler emits:

- **Rust**: `render(ctx)` loads every `load="server"` source at once with
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
  session. `account` is server-only, so it has no endpoint.
- **Assets**: a content-hashed page module (662 B gzipped for the account page),
  the shared runtime (2.7 KB gzipped), and a stylesheet. A page with no state
  and no handlers gets no module and no `<script>` tag.

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

- Sessions use an opaque 256-bit cookie (`HttpOnly`, `SameSite=Lax`, `Secure`
  except on loopback) with server-side state.
- The session id rotates at sign-in.
- Every action checks the CSRF token with a constant-time comparison and
  checks `Origin`.
- Every `<form method="post">` gets its CSRF token automatically, whether Rust
  or the browser renders the form.
- Personalised responses are `private, no-store` with `Vary: Cookie`.
- Every response carries a `Server-Timing` header with `sources;dur=…` and
  `render;dur=…`, which separates upstream latency from render CPU time.

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
- **Sources**: they take no inputs, and there is no client refresh. A failed
  client load shows a Retry button and keeps earlier rows.
- **Not built yet**: CSS scoping, layouts, dynamic route parameters,
  persistent session storage, and component imports.
- **Views**: they must serialize with `#[serde(rename_all = "camelCase")]` so
  that their JSON names match what templates read.

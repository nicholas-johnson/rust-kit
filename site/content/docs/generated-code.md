---
title: Generated code
description: An annotated walk through what the compiler emits for a page, the manifest, the source API and asset names.
section: Reference
order: 5
---

The compiler's output is ordinary Rust and JavaScript that you can read. Reading it is the fastest way to answer questions such as "how many times does this page call the accounts service?" or "what does the browser receive?". This page walks through a real compiled page, then the application-wide parts of `rustkit_generated.rs`.

## Where to find it

`build.rs` writes everything into Cargo's `OUT_DIR` for your crate, which lives under `target/`:

```bash
find target -name rustkit_generated.rs
ls "$(dirname "$(find target -name rustkit_generated.rs | head -1)")/rk_assets"
```

The Rust file is a single module tree with no formatting applied; run it through `rustfmt` if you want to read it comfortably. The `rk_assets` directory holds the browser modules, the runtime and the stylesheets. You can also call [`compile_page`](/docs/compiler-api/#compile-page) on one page's source and print the result.

## A page to compile

Here is a cut-down account page: a greeting from a server-loaded source, a button that toggles an editor, and the editor form. Open the two panels under the sample to see the output this site's build produced from it with the real compiler. The rest of this page quotes from that output.

```rk title=profile.rk
<script>
  import { account } from "$sources";
  let editing = $state(false);
</script>

<title>Profile</title>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
  <button onclick={() => editing = !editing}>Edit</button>
  {#if editing}
    <form method="post" action="/account/profile">
      <input name="name" value={data.displayName} />
      <button>Save</button>
    </form>
  {/if}
</DataSource>

<style>
  h1 { margin: 0; }
</style>
```

## The Rust render function

Each page becomes a module, `page_<stem>`, holding `render` and `render_boxed`. When the whole application is compiled, the module also gets two constants with the hashed asset URLs:

```rust partial
pub mod page_profile {
const CSS: Option<&str> = Some("/_rk/assets/profile.1a2b3c4d.css");
const JS: Option<&str> = Some("/_rk/assets/profile.5e6f7a8b.js");
pub async fn render(ctx: &::rustkit::RequestContext<crate::AppState>, __param: Option<&str>)
    -> ::std::result::Result<::rustkit::Page, ::rustkit::Error> { /* … */ }
pub fn render_boxed<'a>(ctx: &'a ::rustkit::RequestContext<crate::AppState>, __param: Option<&'a str>)
    -> ::rustkit::BoxFut<'a, ::std::result::Result<::rustkit::Page, ::rustkit::Error>> { Box::pin(render(ctx, __param)) }
}
```

The hashes above are illustrative. `render` runs in five steps.

### 1. Load server sources

```rust partial
let __t0 = ::std::time::Instant::now();
let (src_account,) = ::rustkit::__private::try_join!(crate::sources::account(ctx))?;
let __sources_time = __t0.elapsed();
```

Every source the page loads with `load="server"` is listed once, in the order the page first uses it, and called through `try_join!`. Independent reads start together, each source runs exactly once per request however many times the template reads it, and the first error aborts the render. The resolver path comes from the `sources_module` setting, `crate::sources` by default. Pages with no server sources skip the `try_join!` line.

### 2. Write the HTML

```rust partial
let mut out = ::std::string::String::with_capacity(4096);
{ let v_data = &src_account;
    out.push_str("\n  <h1>Hello, ");
    ::rustkit::js::text(&mut out, &::rustkit::js::ToJs::to_js(&v_data.display_name));
    out.push_str("</h1>\n  <button data-rk-on=\"click:h0\">Edit</button>\n  <!--rk:b5-->");
```

Things to notice:

- **Bindings.** `let:data` becomes a local `v_data` borrowing the loaded value. Loop items become `v_item` and so on.
- **Field names.** `data.displayName` becomes `v_data.display_name`: the compiler converts camelCase segments to snake_case, and prefixes Rust keywords with `r#`, so `data.type` reads `v_data.r#type`.
- **Escaping.** Every interpolation goes through `rustkit::js::text`, every bound attribute through `rustkit::js::attr`. Both apply the rules in [JS value semantics](/docs/js-value-semantics/).
- **Literal coalescing.** Adjacent literal writes are merged into one `push_str`, so the closing `</h1>`, the whole button and the block marker are a single call.
- **Handlers.** `onclick={…}` never runs in Rust. It becomes `data-rk-on="click:h0"`, naming handler `h0` in the browser module.
- **Blocks.** The `{#if editing}` reads state, so it is a [block](/docs/blocks-and-updates/). Its initial HTML is written between `<!--rk:b5-->` and `<!--/rk:b5-->`, where `b5` is the node's id.

### 3. Render blocks with their initial state

```rust partial
if (::rustkit::js::Val::Bool(false)).truthy() {
    out.push_str("\n    <form method=\"post\" action=\"/account/profile\"><input type=\"hidden\" name=\"_csrf\" value=\"");
    ::rustkit::js::escape_into(&mut out, &ctx.csrf_token());
```

On the server, `editing` is always its initial value, so the condition is the literal `false`. The branch is still compiled, behind a condition that is always false. Every `<form method="post">` gets a hidden `_csrf` field straight after its start tag.

### 4. Build the handoff payload

```rust partial
let __payload = {
    use ::rustkit::__private::serde_json::{Map, Value, to_value};
    let mut s = Map::new();
    { let mut m = Map::new();
        m.insert("displayName".into(), to_value(&src_account.display_name)?);
        s.insert("account".into(), Value::Object(m)); }
    let mut p = Map::new();
    p.insert("s".into(), Value::Object(s));
    p.insert("csrf".into(), Value::String(ctx.csrf_token()));
    Some(::rustkit::js::html_safe_json(&Value::Object(p)))
};
```

Block `b5` reads `data.displayName`, and the browser will render it when `editing` becomes true. The compiler therefore sends that field, and only that field, under `s.account`. If a browser-rendered region read `data` as a whole, the entire view would be serialised instead. Because the block contains a `POST` form, the CSRF token travels too, as `csrf`. A page with no browser-rendered reads of server data gets `let __payload: Option<String> = None;`. See [Handoff payload](/docs/handoff-payload/).

### 5. Wrap the document and report timings

```rust partial
let html = ::rustkit::document("Profile", CSS, &out, __payload.as_deref(), JS);
Ok(::rustkit::Page { html, sources_time: __sources_time, render_time: __t1.elapsed() })
```

The `<title>` was lifted out of the template and passed to [`document`](/docs/runtime-api/#document). The two durations become the `Server-Timing` header; see [Performance](/docs/performance/).

## The browser module

A page gets a module only if it has a block, a handler, or leftover script code. Otherwise `JS` is `None` and the page has no `<script>` tag at all.

```js partial
import { state, start } from "./runtime.70c435b7.js";
const $s = state({ "editing": false });
start($s, {
  events: ["click"],
  blocks: {
    b5: { deps: ["editing"], render: (c, r) => { const { data } = c; let h = ""; /* … */ return h; } },
  },
  handlers: {
    h0: (c) => { const { data } = c; return (() => $s.editing = !$s.editing); },
  },
  ssr: {
    b5: (P) => ({ data: P.s["account"] }),
    h0: (P) => ({  }),
  },
});
```

| Part | Purpose |
|---|---|
| `state({…})` | The reactive store, a `Proxy` seeded with each `$state` initial value. Assigning a different value schedules an update. |
| page script | Any script lines other than `$sources` imports, component imports and `$state` declarations, copied after the store with state names rewritten to `$s.name`. |
| `events` | Every event type a handler uses. The runtime adds one capturing listener per type on `document` and dispatches by `data-rk-on`. |
| `blocks` | One entry per block: `deps` lists the state it reads; `render(c, r)` returns its HTML from scope `c`. A client source block also has `source` and `bind`. |
| `handlers` | Factories: given a scope, return the handler function. Your handler code is copied as written, with state names rewritten. |
| `ssr` | For each server-rendered block or handler, how to rebuild its scope from the payload `P`. This is why `b5` can render without fetching `account`. |

At start-up the runtime walks the comment markers, adopts each server-rendered block with its `ssr` scope, and fetches any client sources the server left a loading placeholder for. When `editing` changes, it re-renders every block whose `deps` include it. Blocks created by the browser get runtime ids such as `i3`, and their handlers ids such as `#4`, so they never collide with the compiler's `b`/`h` keys.

## The rest of rustkit_generated.rs

After the page modules come three application-wide items. This is the account demo's, from `examples/account-demo`:

```rust partial
/// Sources some page loads in the browser. Only these are reachable over HTTP.
pub const CLIENT_SOURCES: &[&str] = &["activity"];

pub fn source_api<'a>(name: &'a str, ctx: &'a ::rustkit::RequestContext<crate::AppState>, params: &'a [(&'a str, &'a str)])
    -> Option<::rustkit::BoxFut<'a, ::std::result::Result<::rustkit::__private::serde_json::Value, ::rustkit::Error>>> {
    match name {
        "activity" => Some(Box::pin(async move {
            let v = crate::sources::activity(ctx).await?;
            Ok(::rustkit::__private::serde_json::to_value(&v)?)
        })),
        _ => { let _ = params; None }
    }
}

pub fn manifest() -> ::rustkit::Manifest<crate::AppState> {
    ::rustkit::Manifest {
        routes: vec![
            ::rustkit::Route { path: "/account", param: None, prerender: false, render: page_account::render_boxed, js_bytes: 7998, css_bytes: 689 },
            ::rustkit::Route { path: "/article", param: None, prerender: true, render: page_article::render_boxed, js_bytes: 0, css_bytes: 546 },
            // …
        ],
        assets: vec![
            ::rustkit::Asset { path: "/_rk/assets/account.a8714854.js", content_type: "text/javascript; charset=utf-8",
                body: include_str!(concat!(env!("OUT_DIR"), "/rk_assets/account.a8714854.js")) },
            // …
        ],
        client_sources: CLIENT_SOURCES,
        source_api,
    }
}
```

- **`CLIENT_SOURCES` and `source_api`** cover only sources some page loads with `load="client"`. The account demo's `account` source is server-only, so it has no match arm and `GET /_rk/source/account` returns 404. A client source returns the resolver's whole view serialised with serde, which is why view types must derive `Serialize` with camelCase names.
- **`manifest()`** is what you pass to [`App::new`](/docs/runtime-api/#app). `js_bytes` counts the page module plus the runtime, uncompressed.
- **Assets** are embedded with `include_str!`, so the binary carries them and serves them from memory.

## Asset names

Asset files are named `<stem>.<hash>.<ext>`. The stem is the page path relative to the routes directory with every non-alphanumeric character replaced by `_`, so `docs/getting-started.rk` gives `docs_getting_started`. The hash is the top 32 bits of a 64-bit FNV-1a hash of the final file contents, as 8 hex digits. The runtime is `runtime.<hash>.js`, shared by every page. Since a name changes whenever the contents do, the router can serve `/_rk/assets/` with a one-year `immutable` cache lifetime.

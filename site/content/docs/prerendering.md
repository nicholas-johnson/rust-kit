---
title: Prerendering
description: Render public pages once instead of on every request, write them to static files, and know what prerendered pages cannot do.
section: Server
order: 4
---

A page whose content is the same for every visitor does not need to be rendered on every request. Mark it for prerendering and Rust-kit renders it once, with no request and no session, then serves that HTML for every request. The same HTML can also be written to disk and served from any static host.

## Marking a page

Add a module script with `export const prerender = true`:

```rk title=article.rk
<script context="module">
  export const prerender = true;
</script>

<title>Why render on the server?</title>

<article>
  <h1>Why render on the server?</h1>
  <p>A visitor should be able to read a page before any script runs.</p>
</article>
```

The `<script context="module">` block is separate from the ordinary page `<script>`. In v0.1 the compiler reads exactly one thing from it: whether it contains the text `export const prerender = true`. Write it on one line, exactly like that. Anything else in the module script is ignored, and it is not included in the browser module.

This article has no state and no handlers, so the compiler emits no JavaScript for it. The test `article_builds_to_static_html_without_javascript` checks that the built `article/index.html` contains no `<script` at all, and that the route reports `js_bytes == 0`.

## Two ways to use prerendered pages

### Served by the app

When you call `App::into_router()`, it first calls `App::prerender()`, which renders every route marked `prerender`. The router then serves the stored HTML for those paths, with no rendering per request:

```http
GET /article HTTP/1.1

HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
cache-control: public, max-age=300
```

Rendering happens once per router, at startup. If you change data a prerendered page depends on, restart the server to see it.

`public, max-age=300` lets browsers and shared caches keep the page for five minutes. That is safe because a prerendered page cannot depend on the session (see below). Prerendered responses have no `Server-Timing` header, because there is nothing to time.

If any prerendered page fails to render, `into_router` returns the error, and the app does not start. That is deliberate: a broken public page should stop a deploy, not show up as a `500` later.

### Written to disk

`App::build(dir)` prerenders the same routes and writes them, together with every compiled asset, into a directory:

```rust
let app = account_demo::app(Duration::ZERO, true);
let report = app.build(&PathBuf::from("dist")).await?;
for file in &report.files {
    println!("{file}");
}
```

Route paths map to files like this:

| Route | File |
| --- | --- |
| `/` | `index.html` |
| `/article` | `article/index.html` |
| `/docs/intro` | `docs/intro/index.html` |

Assets go under `_rk/assets/`, for example `_rk/assets/article.3f2a9c10.css`. The paths in the HTML are root-relative (`/_rk/assets/…`), so serve the directory from the root of a domain.

`App::build` writes all assets, including those of routes that are not prerendered. It does not write anything for request-time routes; they still need the server.

The reference app wires this to a subcommand:

```bash
cargo run -p account-demo -- build dist
```

This is the demo's own `main.rs`, not a Rust-kit command. Your app decides how to expose `build`. See [CLI](/docs/cli/).

## What a prerendered page cannot do

A prerendered page is rendered with a context that has no request, no cookie and an empty, throwaway session store.

### Reading the session fails loudly

If a resolver used by a prerendered page calls `ctx.require_user()`, it gets an error instead of `Unauthorized`:

```text
internal error: a prerendered route read the session; remove `prerender` or load this source on the client
```

So `into_router` and `build` fail instead of producing a page that was rendered for nobody. The fixes are in the message:

- **Remove `prerender`** if the whole page is personal. It becomes a normal request-time page.
- **Load the personal part on the client.** Keep the page prerendered and wrap the personal data in `<DataSource load="client">`. The static HTML contains a loading placeholder, and the browser fetches the data with the visitor's cookie from `GET /_rk/source/{name}`. See [Client loading](/docs/client-loading/).

```rk title=home.rk
<script context="module">
  export const prerender = true;
</script>
<script>
  import { account } from "$sources";
</script>

<title>Home</title>

<h1>Welcome</h1>
<DataSource source={account} load="client" let:data>
  <p>Signed in as {data.displayName}.</p>
</DataSource>
```

> [!NOTE]
> On a page like this, an anonymous visitor's request for `account` gets `401`, and the region shows the error message and a Retry button. There is no way yet to show a friendlier signed-out state.

### `ctx.user()` does not fail

Only `require_user` has the build-time check. `ctx.user()` simply returns `None` during prerendering, so a resolver that uses it renders the anonymous version of the page, and every visitor gets that version, signed in or not. Avoid `ctx.user()` in sources used by prerendered pages.

### POST forms do not work

A `<form method="post">` on a prerendered page gets a CSRF token, as every form does. But that token belongs to a session created in the throwaway build-time store, which no visitor will ever have. Every submission is rejected with "This form has expired". Keep forms on request-time pages.

## Choosing what to prerender

| Prerender | Render per request |
| --- | --- |
| Articles, documentation, marketing pages | Account pages, dashboards |
| Pages whose data changes only on deploy | Pages showing data that changes between requests |
| Pages that should work from a static host or CDN | Pages with POST forms |

Prerendering and interactivity are independent. A prerendered page can still have `$state` and event handlers; the browser module and runtime are separate content-hashed assets, served with long-lived cache headers.

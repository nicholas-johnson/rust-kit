---
title: How it works
description: The build pipeline and request lifecycle end to end, from build.rs to a click in the browser.
section: Start here
order: 4
---

This page follows one page through its whole life: compiled by `build.rs`, registered as a route, rendered by Rust for a request, adopted by the browser, and updated after a click. You do not need any of this to write pages, but it makes the rules in the rest of the docs feel less arbitrary.

We will use this page as the running example:

```rk title=account.rk
<script>
  import { account, activity } from "$sources";
  let editing = $state(false);
  let showActivity = $state(false);
</script>

<title>Your account</title>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
  <button onclick={() => editing = !editing}>Edit profile</button>
  {#if editing}
    <form method="post" action="/account/profile">
      <input name="name" value={data.displayName} />
      <button>Save</button>
    </form>
  {/if}
</DataSource>

<button onclick={() => showActivity = true}>Show recent activity</button>
{#if showActivity}
  <DataSource source={activity} load="client" let:data>
    {#each data.items as item}
      <p>{item.description}</p>
    {/each}
  </DataSource>
{/if}
```

## At build time

### 1. Parse

`build.rs` calls `rustkit_compiler::build_script()`, which finds every `.rk` file under `src/routes` and parses each one into:

- the **page script**: `import { … } from "$sources"` lines, `let x = $state(literal)` lines, and whatever JavaScript is left over;
- the **module script** (`<script context="module">`), which is only checked for `export const prerender = true`;
- the **stylesheet** from `<style>`;
- the **title**, hoisted out of a top-level `<title>`;
- the **template**: a tree of text, elements, `{expr}`, `{#if}`, `{#each}` and `<DataSource>` nodes.

Template expressions are parsed by a deliberately small expression parser. A call like `formatDate(x)` fails here, with a diagnostic pointing at the exact column. See [Expressions](/docs/expressions/).

### 2. Find the blocks

The compiler then walks the tree and asks, for each node: *can this change after the page loads?* A node can change if its own expressions read `$state`, or if it is a `<DataSource load="client">`. Such a node becomes a **block**.

In the example there are three blocks: `{#if editing}`, `{#if showActivity}`, and the client-loaded `<DataSource>` nested inside it. Everything else, including the greeting, is static once rendered. See [Blocks and updates](/docs/blocks-and-updates/).

### 3. Generate two renderers from one tree

The same tree is walked twice.

The **Rust walk** produces an `async fn render(ctx)` that:

1. starts every `load="server"` source at once with `try_join!`, so independent reads run concurrently and each source is called once per request;
2. writes static HTML with `push_str`, coalescing adjacent literals into single writes;
3. evaluates each `{expr}` through `rustkit::js::Val`, which follows JavaScript's rules for truthiness, equality and number formatting, and escapes the result;
4. wraps each block's initial HTML in `<!--rk:bN-->` … `<!--/rk:bN-->` comment markers;
5. builds the handoff payload, if one is needed, and wraps everything in the HTML document.

The **JavaScript walk** produces, for each block, a function that returns the block's HTML as a string, plus its list of state dependencies. Event handlers are collected into a table and the set of event types the page uses is recorded.

### 4. Decide what crosses to the browser

A block the browser may re-render might read server data: in the example, the form inside `{#if editing}` reads `data.displayName`. The browser has no way to call the `account` resolver (it is server-only), so the compiler records which fields of which source each browser-rendered block or handler reads. Only those fields are serialized into the page. Here that is:

```json
{"s":{"account":{"displayName":"Alice Liddell"}}}
```

Any other field on the view, and anything the resolver read upstream, stays on the server. If a block reads the whole binding (`data` with no field), the whole view is sent. See [The handoff payload](/docs/handoff-payload/).

### 5. Write the outputs

For the whole app, the compiler writes one Rust file, `rustkit_generated.rs`, into `OUT_DIR`, plus content-hashed assets: a module per interactive page, one shared runtime, and a stylesheet per page with a `<style>`. A page with no blocks, no handlers and no leftover script gets **no module and no `<script>` tag**.

The generated file ends with `manifest()`, which returns a `rustkit::Manifest`: the routes, the embedded assets, the list of sources some page loads in the browser, and a dispatcher that calls those sources by name. See [Generated code](/docs/generated-code/).

## At startup

Your `main` calls `App::new(manifest(), state)` and then `into_router()`. That:

1. renders every route marked `prerender` once, with a context that has no request and no session;
2. registers the endpoints below on an `axum::Router`;
3. adds a fallback that returns a plain 404 page.

| Endpoint | Method | Registered when | Caching |
| --- | --- | --- | --- |
| Each page path, e.g. `/account` | GET | Always | Prerendered: `public, max-age=300`. Otherwise `private, no-store` with `Vary: Cookie` if the render touched the session, else `no-cache`. |
| `/_rk/source/{name}` | GET | At least one source is loaded with `load="client"` somewhere | `private, no-store` |
| Each action path | POST | You called `.action(path, f)` | `no-store` redirect |
| `/_rk/assets/<file>` | GET | One per asset | `public, max-age=31536000, immutable` |

Only sources that some page loads with `load="client"` are reachable through `/_rk/source/{name}`. In the example, `/_rk/source/activity` works and `/_rk/source/account` is a 404, because `account` is only ever loaded on the server. That keeps the HTTP surface as small as the pages require.

## During a request

For `GET /account`:

1. **Context.** The router reads the `rk_session` cookie and builds a `RequestContext` that holds your `AppState`, the session (if any), and a flag that records whether anything read the session.
2. **Sources.** `render` calls `crate::sources::account(&ctx)`. The resolver calls `ctx.require_user()`. If nobody is signed in, that returns `Error::Unauthorized` and the page responds with a `303` redirect to `/login?next=/account`. Other errors render a small error page with the right status; see [Errors](/docs/errors/).
3. **HTML.** With the data in hand, the renderer writes the page. The `<h1>` contains `Hello, Alice Liddell` as plain text. Because the `{#if editing}` block contains a `<form method="post">`, the browser may need to render that form itself, so the CSRF token is included in the payload as well as in any server-rendered form.
4. **Document.** `rustkit::document()` wraps the body: doctype, charset and viewport meta, the hoisted `<title>`, the stylesheet link, a `modulepreload` hint, the payload in `<script type="application/json" id="rk-payload">`, and the module `<script>`.
5. **Headers.** The response gets `Server-Timing: sources;dur=…, render;dur=…`, separating the time spent waiting on resolvers from the time spent producing HTML, plus the cache headers from the table above.

## In the browser

The visitor can read the page as soon as the HTML arrives. Then the page module loads. It:

1. imports the shared runtime and creates the reactive state object, `$s`, from the initial values in the page script;
2. runs any leftover page script code;
3. calls `start()`, which parses the payload, attaches one listener per event type to `document` (event delegation), and walks the DOM for `<!--rk:…-->` markers to adopt the server-rendered blocks;
4. starts fetching any client-loaded source whose block is already visible.

Attaching handlers makes no network requests. Server data the browser needs is already in the payload.

## After a click

When the visitor clicks **Edit profile**:

1. The delegated `click` listener finds `data-rk-on="click:h0"` on the button and calls the handler. Its body was compiled from `editing = !editing` to `$s.editing = !$s.editing`.
2. Assigning to `$s.editing` schedules an update in a microtask. Several assignments in one handler are batched into a single update.
3. The runtime re-renders every block whose dependencies include `editing`. For each one, it builds the new HTML string, parses it in a `<template>`, and replaces the nodes between the block's markers.
4. The form appears, with `value` filled from the payload's `displayName` and a hidden `_csrf` field from the payload's token.

When the visitor clicks **Show recent activity**, the `{#if showActivity}` block renders the client `<DataSource>`. It shows a `.rk-loading` placeholder and fetches `/_rk/source/activity` once. The endpoint runs the same resolver with the same session cookie. When the JSON arrives, the block re-renders with the list. If the request fails, the block shows a `.rk-error` message with a Retry button. See [Loading in the browser](/docs/client-loading/).

## Submitting the form

The form posts to `/account/profile`. Before your action runs, the framework checks the `Origin` header against `Host`, refuses cross-site requests, and compares the `_csrf` field with the session's token in constant time. Your action updates the upstream service and returns `Redirect::to("/account")`, which becomes a `303 See Other`. The browser loads `/account` again and Rust renders the new name. See [Form actions](/docs/form-actions/) and [Security](/docs/security/).

## Where to read more

- [Blocks and updates](/docs/blocks-and-updates/) for what exactly re-renders, and its limits.
- [State and events](/docs/state-and-events/) for handlers and the `$s` rewrite.
- [JS value semantics](/docs/js-value-semantics/) for how Rust matches the browser's output.

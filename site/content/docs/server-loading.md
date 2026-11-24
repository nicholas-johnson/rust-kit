---
title: Server loading
description: How load="server" fetches every source before rendering, once per request and concurrently, and where it is allowed to appear.
section: Data
order: 2
---

`load="server"` tells the compiler that a source's data belongs in the first HTTP response. The Rust renderer calls the resolver, waits for it, and writes the HTML with the values already in it. A visitor can read the result before any JavaScript runs, and search engines and link previews see the same thing.

Use it for anything the page is about: the greeting on an account page, the title of a document, the rows of a table someone came to read.

## A server-loaded page

```rk title=account.rk
<script>
  import { account } from "$sources";
</script>

<title>Your account</title>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
  <p>Signed in as {data.displayName}.</p>
</DataSource>
```

A signed-in request gets this in the raw HTML:

```html
<h1>Hello, Alice Liddell</h1>
<p>Signed in as Alice Liddell.</p>
```

This page has no state and no event handlers, so the compiler emits no JavaScript for it. Server loading on its own never adds a script to a page.

## What the renderer does

For each page, the compiler generates one async `render` function. Before it writes any HTML, it collects every distinct source the page loads with `load="server"` and starts all of them together with `tokio::try_join!`:

```rust
let (src_account, src_activity,) = ::rustkit::__private::try_join!(
    crate::sources::account(ctx),
    crate::sources::activity(ctx)
)?;
```

Three things follow from this.

### Sources run concurrently

Independent reads start at the same time. If `account` and `activity` each take 25 ms upstream, the page waits about 25 ms, not 50 ms. You do not have to arrange this; it happens for every page.

### Each source runs once per request

The list of sources is de-duplicated. If two `<DataSource source={account} load="server">` elements appear on the same page, both read the same `src_account` value and the resolver runs once. The acceptance test `account_greeting_is_in_raw_html_and_read_once` checks that a signed-in `GET /account` makes exactly one upstream account read.

Sources are not shared across requests. The next request calls the resolver again.

### Errors happen before rendering

Because every source is awaited before the first byte of HTML is written, a failing resolver never produces half a page. `try_join!` returns the first error, the remaining futures are dropped, and the framework turns the error into a response:

| Resolver returns | Page response |
| --- | --- |
| `Error::Unauthorized` | `303` to `/login?next=/account` |
| `Error::Forbidden` | `403` error page |
| `Error::NotFound` | `404` error page |
| `Error::BadRequest(msg)` | `400` error page showing `msg` |
| `Error::Upstream(_)` | `502` error page; detail logged |
| `Error::Internal(_)` | `500` error page; detail logged |

See [Errors](/docs/errors/) for the exact pages and what is logged.

> [!NOTE]
> All server sources load up front, even ones inside an `{#if}` branch that ends up not rendering. The condition is evaluated after the data arrives, so the renderer cannot know in advance which branches it will need. If a source is expensive and often unused, load it on the client instead.

## Timing

Each server-rendered response carries a `Server-Timing` header that separates waiting from rendering:

```http
HTTP/1.1 200 OK
content-type: text/html; charset=utf-8
server-timing: sources;dur=25.412, render;dur=0.031
cache-control: private, no-store
vary: Cookie
```

`sources` is the wall time spent in `try_join!`, mostly upstream latency. `render` is the time spent writing HTML once the data was available. Browser developer tools show both in the network panel. [Performance](/docs/performance/) has more on reading these numbers.

## Where `load="server"` may appear

A server `<DataSource>` must sit in the part of the page that only the server renders. It may appear:

- at the top level of the page;
- inside ordinary elements;
- inside another server `<DataSource>`;
- inside `{#if}` and `{#each}` blocks whose expressions do not read `$state`.

It may **not** appear anywhere the browser renders: inside an `{#if}` or `{#each}` that depends on `$state`, inside an element whose attributes read `$state`, or inside a `load="client"` `<DataSource>`. Those regions are rebuilt by the browser module, possibly long after the HTTP response, and the browser cannot call a resolver directly.

The compiler stops the build when it finds one:

```rk fail title=account.rk
<script>
  import { account } from "$sources";
  let open = $state(false);
</script>

<button onclick={() => open = !open}>Show account</button>
{#if open}
  <DataSource source={account} load="server" let:data>
    <p>{data.displayName}</p>
  </DataSource>
{/if}
```

There are two ways to fix it, depending on what you want:

1. **The data should be in the first response.** Move the `<DataSource>` outside the state-dependent block and put the `{#if}` inside it. The browser can still show and hide the content, and the fields it needs travel in the [handoff payload](/docs/handoff-payload/).
2. **The data is only needed after the click.** Change it to `load="client"`. See [Client loading](/docs/client-loading/).

The first fix looks like this:

```rk title=account.rk
<script>
  import { account } from "$sources";
  let open = $state(false);
</script>

<DataSource source={account} load="server" let:data>
  <button onclick={() => open = !open}>Show account</button>
  {#if open}
    <p>{data.displayName}</p>
  {/if}
</DataSource>
```

Here `{#if open}` is a block the browser re-renders, and it reads `data.displayName`. The compiler notices that and puts `{"displayName": "…"}` into the page's payload, so the browser can render the paragraph without fetching the account again.

## Prerendered pages

A page marked `export const prerender = true` is rendered once, when the router is built, with no request. Its server sources run with a context that has no session. If such a resolver calls `ctx.require_user()`, it gets an `Error::Internal` instead of a user, so `App::into_router` and `App::build` both fail. The error displays as:

```text
internal error: a prerendered route read the session; remove `prerender` or load this source on the client
```

Sources that read only public data are fine to use on prerendered pages. See [Prerendering](/docs/prerendering/).

## When to choose server loading

| Choose `load="server"` when | Choose `load="client"` when |
| --- | --- |
| The data is the main content of the page | The data appears only after an interaction |
| It must be visible without JavaScript | A slow source should not hold up the page |
| Search engines or previews should see it | The page is prerendered but needs personal data |

Most pages use server loading for almost everything. It costs no JavaScript, makes no extra request from the browser, and keeps the source off HTTP entirely unless another page loads it on the client.

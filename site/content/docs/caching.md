---
title: Caching
description: Every Cache-Control and Vary header the runtime sets, why each one is chosen, and how to keep personal pages out of shared caches.
section: Server
order: 5
---

Rust-kit sets caching headers on every response it produces. You do not configure them; they follow from what the response is and whether it read the session. This page lists every case, so you can predict what a browser, CDN or proxy will do with each response.

## Summary

| Response | `Cache-Control` | `Vary` |
| --- | --- | --- |
| Prerendered page | `public, max-age=300` | none |
| Page that read the session | `private, no-store` | `Cookie` |
| Page that did not read the session | `no-cache` | none |
| Compiled asset (`/_rk/assets/…`) | `public, max-age=31536000, immutable` | none |
| Source endpoint, success | `private, no-store` | `Cookie` |
| Redirect (`303`) | `no-store` | none |
| Error page (HTML) | `no-store` | none |
| Error (JSON, source endpoint) | `no-store` | none |

The rest of this page explains each row.

## Prerendered pages

```http
cache-control: public, max-age=300
```

A [prerendered](/docs/prerendering/) page is rendered once, without a session, so every visitor receives identical bytes. Any cache may store it for five minutes. After that, a browser or CDN asks again and gets the same HTML until the server restarts.

Five minutes is a compromise: long enough to take load off the server and let a CDN absorb traffic, short enough that a deploy shows up quickly. It is fixed in v0.1.

## Pages that read the session

```http
cache-control: private, no-store
vary: Cookie
```

Every request-time page has a `RequestContext` that tracks a *personalised* flag. The flag is set by any call that reads or changes the session:

- `ctx.user()` and `ctx.require_user()`;
- `ctx.csrf_token()`, which the compiler calls for every page that renders a POST form;
- `ctx.login(…)` and `ctx.logout()`.

If the flag is set when rendering finishes, the response is marked `private, no-store`. `private` forbids shared caches (CDNs, corporate proxies) from storing it. `no-store` also stops the browser from keeping a copy, so the back button and a shared computer do not show someone else's account after sign-out. `Vary: Cookie` is a second signal for any cache that ignores `private`: the response depends on the cookie, so it must not be reused for a different one.

Because the flag is set by the same call that reads the session, you cannot read the user and forget to mark the page private. The acceptance test `account_greeting_is_in_raw_html_and_read_once` checks both headers on `GET /account`.

> [!TIP]
> `ctx.user()` sets the flag even when it returns `None`. A page that greets anonymous visitors differently from signed-in ones is personalised for everyone, which is correct: the HTML depends on the cookie either way.

## Pages that did not read the session

```http
cache-control: no-cache
```

A request-time page that never touched the session, for example one that shows public data from a server source, is the same for every visitor at a given moment. `no-cache` lets caches store it but requires them to check with the server before every reuse. Rust-kit does not send `ETag` or `Last-Modified`, so in practice every request reaches your server and renders the page again.

If such a page could safely be cached for longer, consider [prerendering](/docs/prerendering/) it. If it should not depend on a session but does, look for a resolver that calls `ctx.user()`.

## Compiled assets

```http
cache-control: public, max-age=31536000, immutable
content-type: text/javascript; charset=utf-8
```

Page modules, the shared runtime and stylesheets are served from `/_rk/assets/`. Each file name contains a hash of its content:

```text
/_rk/assets/runtime.9c1d4e2a.js
/_rk/assets/account.5b7f0a13.js
/_rk/assets/account.e04c88d1.css
```

When the content changes, the name changes, and pages built from the new code link to the new name. So an asset URL always refers to the same bytes, and browsers and CDNs may keep it for a year without checking (`immutable`). The shared runtime is the same file for every page, so a visitor downloads it once for the whole site.

Assets are compiled into the server binary. Pages link the stylesheet with `<link rel="stylesheet">`, and pages with a browser module preload it with `<link rel="modulepreload">`.

## Source endpoints

```http
cache-control: private, no-store
vary: Cookie
```

`GET /_rk/source/{name}` responses are always treated as personalised, whether or not the resolver read the session. The endpoint exists to deliver data on behalf of a session, and treating every response as private removes the chance of a public cache holding one user's data. The browser runtime does not rely on HTTP caching either: it keeps its own per-page cache and fetches each source at most once per page load. See [Client loading](/docs/client-loading/#one-fetch-per-source-per-page).

## Redirects and errors

Redirects from actions and from sign-in checks are `303 See Other` with `cache-control: no-store`. A redirect usually follows a change, or depends on whether you are signed in, so it must not be replayed from a cache.

Error responses, HTML or JSON, are also `no-store`. A `502` caused by a brief upstream failure should not be cached and shown again after the service recovers.

## Server-Timing

Request-time pages also carry a timing header:

```http
server-timing: sources;dur=25.412, render;dur=0.031
```

It does not affect caching, but it is useful when you are deciding what to cache: `sources` is the time spent waiting for resolvers, `render` the time spent writing HTML. If `sources` dominates, caching inside your resolvers or upstream services is the lever; Rust-kit has no resolver result cache of its own. See [Performance](/docs/performance/).

## Working with a CDN

The defaults are designed to be safe behind a CDN that respects standard headers:

- prerendered pages and assets are cached at the edge;
- personalised pages and source data are never stored;
- non-personalised dynamic pages are revalidated, which, without validators, means passed through.

Configure your CDN to honour the origin's `Cache-Control` rather than override it. In particular, do not set a rule that caches all HTML: it would ignore `private` and serve one visitor's account page to another. See [Deployment](/docs/deployment/).

## Not available yet

- Changing `max-age` for prerendered pages, or setting headers per page.
- `ETag` or `Last-Modified` validators.
- Caching resolver results across requests.

See the [roadmap](/docs/roadmap/).

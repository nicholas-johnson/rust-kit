---
title: Deployment
description: Ship a Rust-kit app as one binary behind a TLS reverse proxy, with the settings and limits that matter in production.
section: Guides
order: 3
---

A Rust-kit app deploys as a single native binary. It contains every compiled page, the browser modules, the runtime and the stylesheets, and it serves them over plain HTTP with axum. Put it behind a reverse proxy that terminates TLS, keep secure cookies on, and run exactly one instance. This guide covers each of those, plus the optional static export.

There are no hosting integrations or deploy commands. Anything that can run a Linux binary and forward HTTP to it can run a Rust-kit app.

## What you ship

```bash
cargo build --release
```

The release binary is all you need. At build time, `build.rs` compiled the pages into Rust and the generated code embeds every asset with `include_str!`, so nothing from `target/` or `OUT_DIR` is read at run time. When the binary starts and calls [`App::into_router`](/docs/runtime-api/#app), it renders every prerendered route once and keeps the HTML in memory.

Build on the same platform you deploy to, or cross-compile as you would for any Rust program. Rust-kit's crates come from git, so the build machine needs access to the repository the first time it fetches dependencies.

### Configuration

Rust-kit reads no environment variables or configuration files. Your `main` decides everything: the listen address, whether cookies are `Secure`, the login path, upstream URLs and credentials. Read them however you like, for example from command-line arguments as the demo does, or from environment variables in your own `main`. The [CLI page](/docs/cli/#the-app-s-own-serve-and-build) has a complete `main.rs` to start from.

## Behind a reverse proxy

The server speaks plain HTTP/1.1 through `axum::serve`. Terminate TLS in a reverse proxy or load balancer and forward requests to the app over a private interface, for example `127.0.0.1:3000`.

### Preserve the Host header

Form actions check that the request's `Origin` header matches its `Host` header, as `http://{host}` or `https://{host}`. If your proxy rewrites `Host` to the upstream address, every form post from a real browser fails with **403** "You don't have access to this.", because the browser's `Origin` is your public domain. Forward the original host. In nginx, for example:

```text
location / {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
}
```

The app does not read `X-Forwarded-Host`, `X-Forwarded-Proto` or `Forwarded`. Only `Host` matters. If your public URL includes a non-default port, `Host` must include it too, since `Origin` does.

### Keep secure cookies on

`App::new` sets `secure_cookies(true)` by default, so the `rk_session` cookie is sent as:

```text
rk_session=<id>; Path=/; HttpOnly; SameSite=Lax; Secure
```

Leave it on in production. The app never sees TLS itself, so it cannot tell whether the visitor used HTTPS; you are promising that it did. Turn it off only for local development over plain HTTP.

> [!WARNING]
> The account demo's `serve` command turns `Secure` off whenever the listen address starts with `127.0.0.1`. That is convenient locally, but it is wrong behind a proxy that forwards to a loopback address. In your own `main`, decide `secure_cookies` from an explicit setting, not from the bind address.

### Compression, logging and limits

The router adds none of these. Configure them in the proxy, or add axum or tower middleware to the router returned by `into_router` before you serve it:

- **Compression.** Responses are not compressed. The runtime is 6,658 bytes uncompressed and about 2.7 KB gzipped.
- **Access logs.** The app logs nothing per request. It prints `rustkit: upstream error: …` or `rustkit: internal error: …` to stderr when a page, source or action fails with `Upstream` or `Internal`; other errors are not logged.
- **Request size limits and timeouts.** Not configured by Rust-kit.
- **Graceful shutdown.** The demo calls `axum::serve(listener, router)` without a shutdown signal. Add axum's `with_graceful_shutdown` in your own `main` if in-flight requests should finish.

There is no built-in health check endpoint. A prerendered route such as `/` is cheap to probe, because it is served from memory.

## Run one instance

Sessions live in an in-memory map inside the process. This has consequences you must plan around:

- **One instance only.** A second instance would not know the first one's sessions. Rust-kit has no shared session store and no support for sticky sessions yet, so do not run replicas behind a load balancer.
- **A restart signs everyone out.** Every session and CSRF token is lost. Visitors who submit a form they loaded before the restart see **400** "This form has expired. Reload the page and try again."
- **Memory grows with sessions.** A session is created at sign-in, and also for an anonymous visitor the first time a request-time page renders a `<form method="post">`, because the form needs a CSRF token. Expired sessions (12 hours idle) are removed only when that id is looked up again, so the map is not swept in the background. A restart clears it.

Persistent and shared session storage is on the [roadmap](/docs/roadmap/). See [Sessions](/docs/sessions/) for the details of the current store.

## Caching and CDNs

The app sets `Cache-Control` on every response, so a CDN in front of it can follow the headers:

| Response | `Cache-Control` | Notes |
|---|---|---|
| Prerendered page | `public, max-age=300` | Safe to cache at the edge. |
| Request-time page that read the session | `private, no-store` | Also `Vary: Cookie`. Must not be cached by a shared cache. |
| Request-time page that did not | `no-cache` | Caches must revalidate; the app has no validators, so in practice it re-renders. |
| `/_rk/source/{name}` | `private, no-store` | Always personalised. |
| `/_rk/assets/…` | `public, max-age=31536000, immutable` | Names change when contents do. |
| Redirects and error pages | `no-store` | |

Make sure your CDN forwards the `Cookie` header to the app and respects `private` and `no-store`; otherwise one visitor's account page could be served to another. See [Caching](/docs/caching/).

## Optional: export static files

[`App::build`](/docs/runtime-api/#app) writes every prerendered page and every asset to a directory:

```text
dist/index.html
dist/article/index.html
dist/_rk/assets/account.a8714854.js
dist/_rk/assets/runtime.70c435b7.js
dist/_rk/assets/article.2886a9f3.css
…
```

Use it when you want a web server or CDN to serve the public pages and assets without touching the binary. Keep the `/_rk/assets/` path exactly as written, because pages reference assets by that absolute path, and serve the assets with a long cache lifetime.

Request-time pages, `/_rk/source/…` endpoints and form actions are not in the export. Route those paths to the running binary. If every page in your app is prerendered and has no forms, the export is a complete static site.

> [!IMPORTANT]
> A prerendered page renders without a request, so it has no session. Do not put a `<form method="post">` on a prerendered page: the CSRF token baked into the static HTML belongs to no real session, and posting the form fails with "This form has expired". Leave such pages request-time, as the demo does with `/login`.

## Checklist

- [ ] `cargo build --release` on the target platform.
- [ ] `secure_cookies(true)` (the default), set explicitly in your `main`.
- [ ] TLS terminated at the proxy, `Host` forwarded unchanged.
- [ ] Exactly one instance; restarts planned for low-traffic times.
- [ ] Compression and access logging configured in the proxy or middleware.
- [ ] CDN, if any, respects `private`, `no-store` and `Vary: Cookie`.
- [ ] No session reads or `POST` forms on prerendered pages.
- [ ] Upstream service URLs and credentials read by your own code, never placed in a view.

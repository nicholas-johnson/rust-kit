---
title: Pages and routes
description: How .rk files map to URLs, how the page title and document shell are produced, and how to prerender a page.
section: Core concepts
order: 1
---

Every `.rk` file under `src/routes` is a page, and every page is a route. There is no router configuration: the file's path *is* the URL. This page covers that mapping, the HTML document Rust-kit wraps around your markup, and the `prerender` flag.

## File paths become URLs

The compiler takes the path relative to `src/routes`, drops the `.rk` extension, and drops a final `index` segment.

| File | URL |
| --- | --- |
| `src/routes/index.rk` | `/` |
| `src/routes/login.rk` | `/login` |
| `src/routes/article.rk` | `/article` |
| `src/routes/account/index.rk` | `/account` |
| `src/routes/blog/first-post.rk` | `/blog/first-post` |
| `src/routes/docs/guides/index.rk` | `/docs/guides` |

Directories nest as deeply as you like. Files are discovered in sorted order, and anything without the `.rk` extension is ignored.

A few details worth knowing:

- **No trailing slash.** Routes are registered exactly as shown. `/blog/first-post` matches; `/blog/first-post/` falls through to the 404 page. Link to the slash-less form.
- **`account.rk` and `account/index.rk` both mean `/account`.** Do not create both: axum panics when `into_router()` registers the same path twice.
- **Keep file names to letters, digits, `-` and `_`.** The compiler turns each page's path into a Rust module name by replacing every other character with `_`. That means `blog/first-post.rk` and `blog/first_post.rk` would produce the same module name and fail to compile.
- **The URL keeps the file name as written**, hyphens included, so `first-post.rk` serves `/first-post`.

> [!NOTE]
> Dynamic route parameters (a file like `blog/[slug].rk` serving every post) are not available yet. Every route is a fixed path, and data sources take no inputs, so a page cannot vary by URL segment or query string today. See the [roadmap](/docs/roadmap/).

## What a page renders

A page file has up to four top-level parts: a `<script>`, a `<script context="module">`, a `<style>`, and the template, which is everything else. Order does not matter, but `<script>` and `<style>` must be at the top level, not inside an element, and a page can have at most one script of each kind.

```rk title=about.rk
<title>About us</title>

<nav><a href="/">Home</a> <a href="/about">About</a></nav>
<h1>About us</h1>
<p>We build small, fast pages.</p>

<style>
  nav { display: flex; gap: 1rem; }
</style>
```

The template is the contents of `<body>`. You do not write `<html>`, `<head>` or `<body>` yourself. Rust-kit supplies them.

### The document shell

Every page, prerendered or not, is wrapped by `rustkit::document()` into this shape:

```html
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>About us</title>
<link rel="stylesheet" href="/_rk/assets/about.1a2b3c4d.css">
<link rel="modulepreload" href="/_rk/assets/about.5e6f7a8b.js">
</head>
<body>
…your template…
<script type="application/json" id="rk-payload">…</script>
<script type="module" src="/_rk/assets/about.5e6f7a8b.js"></script>
</body>
</html>
```

The stylesheet link appears only if the page has a non-empty `<style>`. The `modulepreload` hint and module script appear only if the page needs JavaScript. The payload appears only if a browser-rendered part of the page reads server data or contains a post form (see [The handoff payload](/docs/handoff-payload/)).

The shell is fixed in this release. You cannot change `lang`, add `<meta>` tags, link a favicon, or add anything else to `<head>`. Layouts that would allow this are on the [roadmap](/docs/roadmap/).

### Title hoisting

A `<title>` element **at the top level of the page** is removed from the body and its text becomes the document title. Its text is HTML-escaped when written into `<head>`, so write characters literally: `<title>Q&A</title>` is correct, while `<title>Q&amp;A</title>` would show `Q&amp;A` in the browser tab.

Two limits apply:

- **Only literal text counts.** Expressions inside `<title>` are dropped, so `<title>Hello {name}</title>` produces the title `Hello `. A title that depends on data is not possible yet.
- **Only a top-level `<title>` is hoisted.** One nested inside another element stays where it is in the body, which is almost never what you want.

A page without a `<title>` gets an empty one.

## Request-rendered pages

By default a page is rendered by Rust **for every request**. Its renderer runs the page's server-loaded sources, then writes the HTML. This is what you want for anything personalised or frequently changing.

Request-rendered responses get:

- `Content-Type: text/html; charset=utf-8`
- `Server-Timing: sources;dur=…, render;dur=…`
- `Cache-Control: private, no-store` and `Vary: Cookie` if rendering read the session in any way (for example a resolver called `ctx.user()`, or the page contains a post form, which needs the session's CSRF token). Otherwise `Cache-Control: no-cache`.
- `Set-Cookie` if rendering created a session.

If a resolver returns `Error::Unauthorized`, the visitor is redirected with `303 See Other` to the login path (by default `/login`) with `?next=` set to the page's path. See [Errors](/docs/errors/) and [Caching](/docs/caching/).

## Prerendered pages

A page that is the same for every visitor can be rendered once instead. Add a module script:

```rk title=article.rk
<script context="module">
  export const prerender = true;
</script>

<title>Why render on the server?</title>

<article>
  <h1>Why render on the server?</h1>
  <p>This page was rendered once and is served as a static file.</p>
</article>
```

A prerendered page is rendered:

- by `App::into_router()`, once at startup, and then served from memory with `Cache-Control: public, max-age=300`; and
- by `App::build(dir)`, which writes it to `<dir>/<path>/index.html` (or `<dir>/index.html` for `/`), along with every asset.

Prerendered pages use a context with no request and no session. A resolver that calls `ctx.require_user()` fails with an internal error that tells you to remove `prerender` or load that source in the browser. Server-loaded sources that do not depend on the visitor work fine.

> [!WARNING]
> The compiler looks for the exact text `export const prerender = true` in the module script. A different spelling, such as `prerender=true` without spaces, is silently ignored and the page is rendered per request. Nothing else in the module script is used.

> [!WARNING]
> Avoid `<form method="post">` on a prerendered page. The CSRF token is rendered once at build time from a session no visitor has, so the form will be rejected when submitted. Put forms on request-rendered pages.

See [Prerendering](/docs/prerendering/) for building a static site and deploying the output.

## Not found and other errors

Any path that is not a page, an action, an asset, or a source endpoint gets a minimal 404 HTML page. Custom error pages are not supported yet. See [Errors](/docs/errors/).

## Summary

- One `.rk` file per route; the path is the URL; `index.rk` means the directory.
- Rust-kit writes the document shell; you write the body.
- A top-level `<title>` with literal text becomes the page title.
- `export const prerender = true` in `<script context="module">` renders a page once.

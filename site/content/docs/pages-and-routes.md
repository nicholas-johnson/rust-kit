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
| `src/routes/layout.rk` | not a route; wraps every page |
| `src/routes/blog/[slug].rk` | `/blog/{slug}` |
| `src/lib/Header.rk` | not a route; a component imported from `$lib` |

Directories nest as deeply as you like. Files are discovered in sorted order, and anything without the `.rk` extension is ignored.

A few details worth knowing:

- **No trailing slash.** Routes are registered exactly as shown. `/blog/first-post` matches; `/blog/first-post/` falls through to the 404 page. Link to the slash-less form.
- **`account.rk` and `account/index.rk` both mean `/account`.** Do not create both: axum panics when `into_router()` registers the same path twice.
- **Keep file names to letters, digits, `-` and `_`, plus one `[param]` segment.** The compiler turns each page's path into a Rust module name by replacing every other character with `_`. That means `blog/first-post.rk` and `blog/first_post.rk` would produce the same module name and fail to compile, and so would `blog/[slug].rk` and `blog/_slug_.rk`.
- **The URL keeps the file name as written**, hyphens included, so `first-post.rk` serves `/first-post`. A `[slug]` segment serves `{slug}`.

## Layouts

`layout.rk` in a folder is not a route. It wraps every page in that folder. Nested folders nest layouts, outermost first: a root `layout.rk` wraps `blog/layout.rk`, which wraps `blog/index.rk`.

```rk partial title=src/routes/layout.rk
<header><a href="/">Home</a></header>
<slot />

<style>
  header { font-weight: 600; }
</style>
```

`<slot />` (or an empty `<slot></slot>`) is where the page is rendered. A layout needs exactly one. It takes no attributes and no fallback content. A `<slot>` in a page is a compile error. A component may have one `<slot />` for the tag's children; see [Components](#components).

A layout may use the same template constructs as a page. Its `<style>` is concatenated ahead of the page's stylesheet and [scoped to the layout](/docs/styling/#styles-are-scoped-to-the-file). The page's rules win where both match the same element. A `$state` name declared in a layout cannot be declared again in a nested layout or the page. `export const prerender = true` belongs on the page, not the layout.

The document `<title>` is the page's when it has one, otherwise the nearest layout's. Layouts wrap the body. They do not add `<meta>` tags or change `lang`.

## Components

A page or a layout can import a shared `.rk` file and render it by its import name.

```rk partial title=src/routes/index.rk
<script>
  import Header from "$lib/Header.rk";
  import Card from "$lib/Card.rk";
</script>

<Header label="Notes" />
<Card title="Today">
  <p>Three things to do.</p>
</Card>
```

`$lib` is the `lib` directory next to `src/routes`, so `src/lib` when routes live in `src/routes`. A relative path that ends in `.rk` resolves from the importing file: `./Badge.rk` or `../ui/Card.rk`. That is how one component imports another. Keep component files out of `src/routes`, or the compiler will also publish them as pages.

The import is one line. The name starts with an uppercase letter, and that name is the tag.

```rk partial title=src/lib/Card.rk
<section class="card">
  <h2>{title}</h2>
  <slot />
</section>

<style>
  .card { border: 1px solid #ccc; padding: 0.75rem; }
</style>
```

The compiler inlines the component into the page, the same way it inlines a layout. Both renderers walk one tree. There is no component runtime.

What gets merged into the page, once per component file:

- `$state` declarations. Two `<Card>` tags share that state. A `$state` name cannot also be declared in the page, a layout, or another component on the same page.
- `import { … } from "$sources"` lines. A source imported by a component counts as an import from every page and layout that uses the component. The [one-parameter rule](#one-dynamic-segment) still applies.
- Any other script, copied into the page module.
- `<style>`, concatenated ahead of the importing file, nested components first, and [scoped to that file](/docs/styling/#styles-are-scoped-to-the-file). Layout composition still puts the outer layout first and the page last. Slot children stay in the parent's scope.

A prop is a name the component's template reads. Every such name is required. Pass a static string (`title="Today"`) or one expression (`title={data.title}`). There are no defaults, no bare attributes, and no mixed strings. The tag does not take event handlers: write `onclick` inside the component. A prop has no fields; pass `name={data.name}` rather than reading `user.name` where `user` is the prop.

One `<slot />` receives the tag's children. Those children stay in the parent's scope, so they can read the page's state and they cannot read the component's props. A component with no slot cannot have children. Whitespace between the tags is not children.

A component cannot contain `<title>`, and `export const prerender = true` belongs on the page.

## One dynamic segment

A directory or file named `[param]` is one URL segment. `blog/[slug].rk` and `blog/[slug]/index.rk` are both `/blog/{slug}`; do not create both. The name must be a Rust identifier, and not `self`, `super`, `crate` or `Self`.

```rk partial title=src/routes/blog/[slug].rk
<script>
  import { post } from "$sources";
</script>

<DataSource source={post} load="server" let:data>
  <h1>{data.title}</h1>
</DataSource>
<p>Slug: {slug}</p>
```

`{slug}` is in scope in that page and in a `layout.rk` that sits inside the `[slug]` directory. The generated page module has `struct Params { pub slug: String }`.

A resolver takes the parameter when every file that imports that source sits on the same `[param]`:

```rust partial
pub async fn post(ctx: &RequestContext<AppState>, slug: &str) -> Result<PostView, Error>
```

The generated call is `post(ctx, params.slug.as_str())`. A source imported from a fixed path stays `fn post(ctx: &RequestContext<AppState>)`. Importing one source from both, or from two different parameter names, is a compile error. A source imported inside a component counts as an import from every page and layout that renders that component. Move the import under the dynamic segment, or split the source.

A client-loaded source that takes the parameter is fetched as `GET /_rk/source/post?slug=notes`. A missing or empty parameter is `400`. When the browser module reads `{slug}`, the handoff payload also carries `params.slug`.

A route may contain one `[param]`, anywhere in the path: `blog/[slug]/open.rk` is `/blog/{slug}/open`. A second dynamic segment, a rest parameter (`[...slug]`), and an optional segment (`[[slug]]`) are compile errors. A dynamic page cannot be prerendered, because there is no content collection to enumerate the values.

If rendering a dynamic page returns `Error::Unauthorized`, `next` is the path that was requested, with the captured segment percent-encoded (`/blog/a%20b`), not the pattern `/blog/{slug}`.

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

The shell is fixed in this release. You cannot change `lang`, add `<meta>` tags, link a favicon, or add anything else to `<head>`. A [layout](#layouts) wraps the body and can contribute a `<title>` and a stylesheet; it does not change the shell. Custom head content is on the [roadmap](/docs/roadmap/).

### Title hoisting

A `<title>` element **at the top level of the page** is removed from the body and its text becomes the document title. Its text is HTML-escaped when written into `<head>`, so write characters literally: `<title>Q&A</title>` is correct, while `<title>Q&amp;A</title>` would show `Q&amp;A` in the browser tab.

Two limits apply:

- **Only literal text counts.** Expressions inside `<title>` are dropped, so `<title>Hello {name}</title>` produces the title `Hello `. A title that depends on data is not possible yet.
- **Only a top-level `<title>` is hoisted.** One nested inside another element stays where it is in the body, which is almost never what you want.

A page's `<title>` wins over a layout's. When the page has none, the nearest layout title is used. When nobody has one, the document title is empty.

## Request-rendered pages

By default a page is rendered by Rust **for every request**. Its renderer runs the page's server-loaded sources, then writes the HTML. This is what you want for anything personalised or frequently changing.

Request-rendered responses get:

- `Content-Type: text/html; charset=utf-8`
- `Server-Timing: sources;dur=…, render;dur=…`
- `Cache-Control: private, no-store` and `Vary: Cookie` if rendering read the session in any way (for example a resolver called `ctx.user()`, or the page contains a post form, which needs the session's CSRF token). Otherwise `Cache-Control: no-cache`.
- `Set-Cookie` if rendering created a session.

If a resolver returns `Error::Unauthorized`, the visitor is redirected with `303 See Other` to the login path (by default `/login`) with `?next=` set to the path that was requested. On a dynamic route that is the concrete path, with the segment percent-encoded. See [Errors](/docs/errors/) and [Caching](/docs/caching/).

## Prerendered pages

A page that is the same for every visitor can be rendered once instead. A dynamic page cannot: `[slug]` has no single page to prerender, and content collections are not in this release. Add a module script:

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

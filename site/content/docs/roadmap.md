---
title: Roadmap
description: What the first milestone includes, what is not built yet, and the known limits of what exists.
section: Project
order: 2
---

Rust-kit 0.1 is the first milestone of the [design proposal](/docs/design/). Its job was to prove the core model on two real pages: a static article that ships no JavaScript, and a personalised account page rendered by Rust with a small browser module. The four acceptance properties from the proposal are automated tests in `examples/account-demo/tests/acceptance.rs`.

This page lists what exists, what does not exist yet, and the rough edges in what does. It is the page other docs link to when they say "not available yet". It describes the code as it is; it is not a schedule.

## What exists today

| Area | Available in 0.1 |
| --- | --- |
| Pages | `.rk` files under `src/routes`, file-path routing, `index.rk`, nested directories, `layout.rk`, one `[param]` segment |
| Components | `import Name from "$lib/….rk"` (or a relative `.rk` path), inlined into the page or layout, with required props and one `<slot />` |
| Template | Text, `{expr}`, static/bound/mixed attributes, `{#if}`/`{:else if}`/`{:else}`, `{#each list as item, i}`, comments |
| Expressions | Field reads, `.length`, literals, `!`, `&&`, `\|\|`, `===`, `!==`, `<`, `<=`, `>`, `>=`, evaluated with JavaScript semantics in Rust |
| Interactivity | `$state` with primitive literals, `on<event>` handlers with any JavaScript, page script code, delegated events |
| Updates | Block-level re-rendering with batched microtask updates |
| Data | `<DataSource>` with `load="server"` (concurrent, once per request) and `load="client"` (fetched once per page load, with Retry) |
| Handoff | A minimal, HTML-safe JSON payload with only the fields browser code reads |
| Server | `App` on axum, form actions, `Redirect`, typed `Error` responses, `Server-Timing` |
| Security | Server-side sessions with opaque cookies, session rotation at sign-in, automatic CSRF tokens, origin checks, cache headers for personalised pages |
| Static output | `prerender = true`, `App::build(dir)` |
| Styling | One hashed stylesheet per page, scoped to the file that wrote it. `:global` and `rustkit.toml` `css_scope = false` opt out |
| Diagnostics | Compile errors with file, line, column and a help line |

## Not built yet

These are absent from the code. Where a page in the docs mentions one, it links here.

### Authoring

- **Custom `<head>` content.** The document shell is fixed: no extra `<meta>`, `<link>` or favicon tags, and `lang="en"` always.
- **Dynamic titles.** `<title>` takes literal text only.
- **A separate shared stylesheet.** A layout's `<style>` is prepended to each wrapped page's sheet. There is no one CSS file shared by URL.
- **Directives.** No `bind:`, `class:`, `use:` or spread attributes.
- **More block types.** No `{@html}`, `{#await}`, `{#key}`, or `{:else}` for empty `{#each}`.

### State and updates

- **Array and object state.** `$state` accepts string, number, boolean or `null` only, so `{#each}` over state is diagnosed.
- **Keyed lists and fine-grained patches.** An update re-renders a whole block, so focus and uncommitted input inside it are lost.
- **Handlers reading server-rendered loop items.** A handler inside a `{#each}` that Rust renders cannot read the loop item. This is diagnosed.

### Routing and data

- **More than one dynamic segment.** A route has at most one `[param]`. Rest parameters (`[...slug]`) and optional segments (`[[slug]]`) are rejected, and a dynamic page cannot be prerendered. There are no content collections.
- **Source inputs beyond the route parameter.** A resolver takes `slug: &str` only when every file that imports it sits on that same `[param]`. It does not receive a query string, or an argument from the page.
- **Client refresh.** A client-loaded source is fetched once per page load. There is no way to refetch it except the Retry button after a failure.
- **Renamed imports.** `import { account as acct } from "$sources"` is rejected.

### Server and operations

- **Persistent sessions.** Sessions live in process memory. Restarting the server signs everyone out, and multiple instances do not share sessions.
- **Session configuration.** The idle timeout (12 hours) and cookie name (`rk_session`) are fixed when using `App`.
- **Custom error pages.** Error responses use a fixed minimal template.
- **Static file serving.** There is no `public/` directory; add your own axum route.
- **A real upstream client in the demo.** The demo's accounts service is an in-memory stand-in with latency and call counters. A real HTTP client would replace `examples/account-demo/src/upstream.rs` and nothing else.

### Tooling

- **Project generator.** There is no `cargo rustkit new` or any other CLI. Projects are set up by hand; see [Getting started](/docs/getting-started/).
- **Dev server and live reload.** Rebuild and restart with `cargo run`.
- **crates.io releases.** Depend on the Git repository.
- **Editor support.** No syntax highlighting or language server for `.rk` files.

## Known limits in what exists

These work, with constraints worth knowing:

- **Line-based script parsing.** Each `import { … } from "$sources";`, each `import Name from "$lib/….rk";`, and each `let x = $state(literal);` must be on its own line. Declarations must use `let`.
- **Shared component state.** A component's `$state` is merged into the page once, so two uses share it. Props have no defaults and no fields. The component tag does not take event handlers. See [Components](/docs/pages-and-routes/#components).
- **The state rewrite is not scope-aware.** A local variable or parameter with the same name as a state variable is rewritten too. Regular-expression literals are not recognised by the scanner.
- **`&&` and `||` produce booleans** in templates, not operand values. See [Expressions](/docs/expressions/#and-return-booleans).
- **Views must use camelCase.** They need `#[serde(rename_all = "camelCase")]` so that JSON names match what templates read.
- **Prerender detection is textual.** The module script must contain exactly `export const prerender = true`.
- **Forms on prerendered pages.** A `<form method="post">` on a prerendered page gets a build-time CSRF token that will not validate.
- **One `<style>` per page.** A second one silently replaces the first.
- **No trailing-slash handling.** `/about/` does not match the `/about` route.
- **Delegated `stopPropagation`.** Stopping propagation in a Rust-kit handler does not stop Rust-kit handlers on ancestor elements.

## How this list changes

The proposal describes later milestones in prose; the items above are the gaps between it and the code today. If you want to help close one, read [Contributing](/docs/contributing/). If something you need is missing from both lists, the [FAQ](/docs/faq/) covers the most common questions about fit.

---
title: FAQ
description: Short answers to common questions about Rust-kit, its design choices, and whether it fits your project.
section: Project
order: 3
---

## Is Rust-kit ready for production?

Not yet, for most teams. Version 0.1 is the first milestone of a [design proposal](/docs/design/). The code is tested and its security defaults are deliberate, but sessions live in memory, a component's `$state` is shared by every use on the page, a route can take only one parameter, and the crates are not on crates.io. Use it to evaluate the model, build internal tools, or contribute. The [roadmap](/docs/roadmap/) lists every gap.

## Do I need Node.js or a bundler?

No. The compiler is a Rust build dependency and runs inside `cargo build`. The browser JavaScript it produces is plain ES modules served by your Rust binary. There is no npm step, no `node_modules`, and no JavaScript engine on the server.

## Does the server run my JavaScript?

Never. Your page script and event handlers run only in the browser. The server runs the template, compiled to Rust, and your Rust resolvers. That split is the core idea; see [Introduction](/docs/introduction/).

## Does it use WebAssembly?

No. Rust runs on the server as native code. The browser runs JavaScript. Nothing is compiled to Wasm.

## Why can't I call a function in a template?

Because the template is rendered twice: once by Rust, then again by the browser for any part that changes. Both must produce identical text, and Rust cannot run an arbitrary JavaScript function, nor guarantee the browser's locale, time zone or engine behave the same way. Compute the value in your resolver and expose it as a field. See [Expressions](/docs/expressions/#why-the-language-is-small).

## Why does `{name || "Anonymous"}` show `true`?

In Rust-kit templates, `&&` and `||` produce booleans in both renderers. Use `{#if name}{name}{:else}Anonymous{/if}`. See [Expressions](/docs/expressions/#and-return-booleans).

## How is this different from SvelteKit?

The authoring model is inspired by Svelte: a template, a script and a style in one file, with `$state` and `{#if}`/`{#each}` blocks. The differences are where code runs and how much ships:

| | SvelteKit | Rust-kit 0.1 |
| --- | --- | --- |
| Server rendering | JavaScript on Node or an edge runtime | Native Rust, compiled from the template |
| Data loading | JavaScript `load` functions | Rust resolvers in `crate::sources` |
| Browser code | Hydrates the whole page | Adopts only the blocks that can change |
| Static pages | Ship the framework unless configured otherwise | Ship no JavaScript at all |
| Template expressions | Any JavaScript | A small, portable subset |
| Maturity | Mature ecosystem | First milestone |

## How is this different from htmx or Leptos?

Briefly: htmx keeps all rendering on the server and swaps HTML fragments over the network; Rust-kit renders updates in the browser from state, without a round trip. Leptos and similar Rust frameworks compile Rust components to WebAssembly for the browser; Rust-kit keeps Rust on the server and writes the browser half in JavaScript. Pick whichever matches where you want your logic to live.

## Is there hydration?

No. The browser does not re-render the page on load. It finds the `<!--rk:…-->` markers around the blocks Rust rendered, attaches delegated event listeners, and leaves the DOM alone until state changes. See [Blocks and updates](/docs/blocks-and-updates/#markers).

## How much JavaScript does a page ship?

A page with no state, handlers or page script ships none: not even a `<script>` tag. An interactive page loads its own module (the account demo's is 662 bytes gzipped) plus the shared runtime (about 2.7 KB gzipped, cached across pages).

## What does the browser learn about my data?

Only what browser code reads. If a block or handler the browser runs reads `data.displayName`, the payload contains `{"displayName": …}` for that source and nothing else. Fields the page renders only on the server never leave it. Client-loaded sources return the whole view through `/_rk/source/{name}`, so keep those views lean. See [The handoff payload](/docs/handoff-payload/).

## Can I use TypeScript?

Not in this release. Page scripts and handlers are copied into the browser module as written, without a compile step, so they must be JavaScript the browser understands.

## Can I import npm packages or other modules in a page script?

No. `import { … } from "$sources"` names a data source, and `import Name from "$lib/….rk"` inlines a [component](/docs/pages-and-routes/#components). Other imports are not resolved or bundled.

## Why are arrays not allowed in `$state`?

The server renders the page with each state variable's initial value, so that value must mean the same thing in Rust and JavaScript. Primitive literals do. Arrays and objects need more design (and keyed list updates to be pleasant), which is on the [roadmap](/docs/roadmap/).

## Why do my view fields need camelCase?

Templates read JSON-style names like `data.displayName`. The browser sees your view serialized by serde, and the Rust renderer converts each name to snake case to read the field. Both agree only if the view uses `#[serde(rename_all = "camelCase")]`.

## Can a page read the URL or query string?

A route can take one parameter. `blog/[slug].rk` is `/blog/{slug}`, `{slug}` is in the template, and a resolver takes `slug: &str` when every file that imports it sits on that segment. See [Pages and routes](/docs/pages-and-routes/#one-dynamic-segment). Query strings are not available to pages or sources, so `?next=` still cannot be read in a template. Rest parameters, optional segments and a second dynamic segment are on the [roadmap](/docs/roadmap/).

## Why did I lose focus while typing?

The input is inside a block that re-renders on every keystroke, usually because its `value` reads the state its own handler sets. Leave the input uncontrolled. See [Blocks and updates](/docs/blocks-and-updates/#granularity-and-its-costs).

## Can I add my own axum routes or middleware?

Yes. `App::into_router()` returns a normal `axum::Router`. Merge it with your own routes, nest it, or wrap it in `tower` layers before passing it to `axum::serve`.

## Where do sessions live?

In memory, in the server process, with a 12-hour idle timeout. A restart signs everyone out, and multiple instances do not share sessions. Persistent session storage is not built yet. See [Sessions](/docs/sessions/).

## Is there a CLI?

No framework CLI exists. `cargo build`, `cargo run` and `cargo test` are all you need. The `serve` and `build` commands in the demo are implemented in its own `main.rs`. See [CLI](/docs/cli/).

## How do I report a bug or contribute?

Read [Contributing](/docs/contributing/). Small, focused changes with a test are the easiest to review.

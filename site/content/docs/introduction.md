---
title: Introduction
description: What Rust-kit is, the idea behind it, who it is for, and what state the first release is in.
section: Start here
order: 1
---

Rust-kit is a small web framework for writing pages as a template, a script, and a stylesheet in one `.rk` file. At build time the compiler turns each page into a **native Rust render function** plus, only when the page needs one, a **small browser module** for its interactive parts. The server sends complete HTML, so a visitor can read the page before any JavaScript runs.

If you have used Svelte or SvelteKit, the authoring model will feel familiar. What is different is *where each language runs*.

## The core idea: Rust renders, the browser responds

Most component frameworks render the first view of a page by running the same JavaScript components on the server that later run in the browser. That means production needs a JavaScript engine on the server, or the page waits for a bundle before it shows anything useful.

Rust-kit splits the work differently:

| Work | Runs in | Written in |
| --- | --- | --- |
| Routing, sessions, CSRF checks | Rust server | Framework code |
| Loading data from your services | Rust server | Your Rust resolvers |
| The first HTML of every page | Rust server | Your template, compiled to Rust |
| Click handlers and page logic | Browser | Your JavaScript |
| Re-rendering after state changes | Browser | Your template, compiled to JavaScript |

The template is the one thing that runs in both places. The compiler reads it once and emits two renderers from the same tree: Rust for the first response, JavaScript for updates. To make sure both produce the *same text*, template expressions are deliberately limited to a small language that has defined behaviour in both targets (see [Expressions](/docs/expressions/)). Anything that needs real logic, like formatting a date, belongs in a Rust resolver, where you compute it once and expose it as a field.

Event handlers are the opposite. They only ever run in the browser, so they can be any JavaScript you like.

## A taste

Here is a complete page. It is compiled by the real compiler every time this site is built; expand the panels under it to see the generated Rust and JavaScript.

```rk title=greeting.rk
<script>
  import { account } from "$sources";
  let editing = $state(false);
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
```

What happens with this page:

- `account` is a **data source**: a Rust function in your crate that returns a typed view. Because it is loaded with `load="server"`, Rust calls it during the request and writes `Hello, …` straight into the HTML.
- `editing` is **state**. The `{#if editing}` region depends on it, so the compiler makes that region a *block*: Rust renders its initial contents, and the browser module gets a function to re-render it when `editing` changes.
- The form inside the block reads `data.displayName`, and the browser may need to re-render it. So the compiler passes exactly that one field to the browser in a small JSON payload. Nothing else from the view crosses over.
- The `<form method="post">` gets a CSRF token automatically, whether Rust or the browser renders it.

A page with no state and no handlers gets no JavaScript at all, not even a `<script>` tag.

## Who it is for

Rust-kit is aimed at teams who:

- already run Rust services and want server-rendered pages without adding a Node.js tier,
- care about pages that are readable and usable before JavaScript loads,
- want interactivity in the places that need it, written as ordinary JavaScript, rather than a client-side application for every page,
- like knowing exactly which data reaches the browser.

It is probably not what you want today if you need a large single-page application, a rich component library, or a mature ecosystem. See the [FAQ](/docs/faq/) for more on fit.

## Status: v0.1

This is version 0.1, the **first milestone** of a [design proposal](/docs/design/) written in November 2026. It is real, tested code, and it is deliberately narrow. The milestone's goal was to prove the core model end to end on two pages: a static article and a personalised account page.

What works today:

- `.rk` pages with `{expr}`, `{#if}`, `{#each}`, `<DataSource>`, event handlers, `$state`, and a per-page stylesheet.
- Server-loaded and client-loaded data sources, with a minimal handoff payload.
- In-memory sessions, CSRF-checked form actions, and sensible cache headers.
- Prerendering of public pages to static HTML.

What does not exist yet includes layouts, components other than `<DataSource>`, dynamic route parameters, CSS scoping, array or object state, keyed lists, source inputs, persistent sessions, and a project generator. Each is listed with its current status on the [roadmap](/docs/roadmap/).

> [!IMPORTANT]
> The crates are not published to crates.io. You depend on them from the Git repository, and APIs may change between commits. Pin a revision if you build anything you care about.

## Where to go next

- [Getting started](/docs/getting-started/) walks you through creating a crate from scratch and serving your first page.
- [How it works](/docs/how-it-works/) follows one request from `build.rs` to a click in the browser.
- [Your first page](/tutorials/your-first-page/) is a hands-on tutorial if you prefer learning by building.
- [Template syntax](/docs/template-syntax/) is the complete reference for what you can write in a page.

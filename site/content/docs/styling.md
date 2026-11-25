---
title: Styling
description: How a page's style block becomes a hashed stylesheet, why styles are global, and the classes Rust-kit renders for loading and error states.
section: Core concepts
order: 6
---

Each page can have one `<style>` block. The compiler turns it into a separate, content-hashed CSS file and links it from the page's `<head>`. There is no preprocessing, no scoping, and no CSS-in-JS: what you write is what the browser gets.

## One stylesheet per page

```rk title=profile.rk
<title>Profile</title>

<main class="profile">
  <h1>Your profile</h1>
  <p class="hint">Changes are saved when you press Save.</p>
</main>

<style>
  :root { color-scheme: light dark; font-family: system-ui, sans-serif; }
  .profile { max-width: 40rem; margin: 2rem auto; padding: 0 1rem; }
  .hint { color: GrayText; }
</style>
```

What the compiler does with it:

1. Takes the text between `<style>` and `</style>` exactly as written.
2. Names the file after the page and a hash of its contents, for example `profile.3f9a0c12.css`.
3. Embeds it in your binary and serves it at `/_rk/assets/profile.3f9a0c12.css` with `Cache-Control: public, max-age=31536000, immutable`.
4. Adds a `<link rel="stylesheet">` element pointing at that file to the page's `<head>`.

Because the file name changes whenever the CSS changes, browsers can cache it for a year and still never see a stale version.

A page with no `<style>`, or one that contains only whitespace, gets no stylesheet and no `<link>`.

### Rules for the `<style>` element

- It must be at the top level of the page, not inside another element.
- Its position does not matter. By convention it goes at the end of the file.
- Use **one** per page. If a page has two, only the last one is used and the earlier one is silently discarded.
- Attributes on `<style>` (such as `lang` or `media`) are ignored.
- The content is not parsed by Rust-kit, so `{` and `}` in CSS are fine and template expressions do not work inside it.

## Styles are global

The stylesheet is not scoped to the page's markup. A rule like `h1 { … }` applies to every `<h1>` in the document, and class names are used exactly as written.

In practice this matters less than it would in a component framework, because each page's stylesheet only loads on that page. Two pages can both style `h1` differently without conflict. Within a page, though, the cascade works as plain CSS always does.

Scoped styles are not available yet. If you need isolation within a page, use a naming convention (a class on the page's root element, as `.profile` above, with selectors under it). See the [roadmap](/docs/roadmap/).

## Sharing styles between pages

A [`layout.rk`](/docs/pages-and-routes/#layouts) `<style>` is concatenated ahead of each wrapped page's `<style>`, in one hashed file per page, so the page's rules win. A page cannot import another CSS file through the compiler, and there is no single stylesheet URL shared across pages. Your options today:

- **Repeat the shared rules in each page's `<style>`.** The bundled demo does this for its navigation and base typography. Each page's CSS file is cached separately, so the cost is a little duplicated download, not a slower page.
- **Add a stylesheet link by hand.** You cannot add to `<head>`, but a `<link rel="stylesheet">` in the template body works in every current browser. You would serve that file yourself, for example with an extra axum route.
- **Use `@import`** at the top of a page's `<style>` to pull in a stylesheet you serve yourself. This costs an extra round trip before rendering, so prefer one of the first two.

A separate shared stylesheet file is on the [roadmap](/docs/roadmap/).

## Loading and error states

Rust-kit renders two pieces of markup of its own, both for `<DataSource load="client">`. Style them with these classes.

### `.rk-loading`

While a client-loaded source is being fetched, its region contains:

```html
<p class="rk-loading" aria-busy="true">Loading…</p>
```

Rust renders this placeholder in the initial HTML when the source is visible on page load, and the browser renders the same markup when the source first appears later. A common treatment is to dim it or reserve space so the page does not jump:

```css
.rk-loading { opacity: 0.6; min-height: 3rem; }
```

### `.rk-error`

If the fetch fails (a network error, or any non-2xx response), the region shows:

```html
<p class="rk-error" role="alert">Couldn't load this. <button type="button" data-rk-retry="activity">Retry</button></p>
```

If the source had loaded successfully earlier in the page's life, the previous content stays visible above this message. Clicking Retry fetches the source again.

```css
.rk-error { color: #b00020; }
.rk-error button { margin-inline-start: 0.5rem; }
```

The text of both messages is fixed and in English in this release.

## Styling blocks

Blocks are invisible to CSS. The `<!--rk:…-->` markers are comments, so they do not create elements, do not affect selectors like `:first-child`, and cannot be styled. A block re-render replaces DOM nodes, so:

- CSS transitions on elements inside a block restart from their initial state after each update.
- `:focus` and `:hover` states on replaced elements are lost until the pointer moves or focus returns.

This means a CSS transition will not animate a change made through state: the element with the old value is removed and a new one with the new value is inserted. If you need an animated toggle, change the class directly from a handler on an element that is **not** inside a block, for example `onclick={function () { this.classList.toggle("open") }}`. The runtime never re-renders static HTML, so that change sticks. See [Blocks and updates](/docs/blocks-and-updates/).

## Bound classes and inline styles

Attribute rules apply to `class` and `style` like any other attribute:

```rk title=status.rk
<script>
  let status = $state("idle");
</script>

<p class="status status-{status}">Status: {status}</p>
<div style="width: 40%" hidden={status === "idle"}>Working…</div>
<button onclick={() => status = "busy"}>Start</button>
```

- `class="status status-{status}"` is a mixed attribute; the value is joined as text.
- `hidden={…}` is a bound boolean: present when true, omitted when false.
- There is no `class:name={…}` directive. Use a mixed attribute as above, or an `{#if}`.

## Summary

- One `<style>` per page becomes one hashed, immutable CSS file linked in `<head>`.
- Styles are global within the page; nothing is scoped or rewritten.
- `.rk-loading` and `.rk-error` mark client-loading states.
- A layout's CSS is copied into each page's sheet. Scoping, and a stylesheet shared by URL, are not available yet.

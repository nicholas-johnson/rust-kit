---
title: Styling
description: How a page's style block becomes a hashed stylesheet, how selectors are scoped to the file that wrote them, and the classes Rust-kit renders for loading and error states.
section: Core concepts
order: 6
---

Each page can have one `<style>` block. The compiler scopes its selectors to that file, turns the result into a content-hashed CSS file, and links it from the page's `<head>`. There is no preprocessing and no CSS-in-JS.

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

1. Scopes the text between `<style>` and `</style>` to this file, as described below. `<style global>` is copied unchanged.
2. Names the file after the page and a hash of the CSS it emits, for example `profile.3f9a0c12.css`.
3. Embeds it in your binary and serves it at `/_rk/assets/profile.3f9a0c12.css` with `Cache-Control: public, max-age=31536000, immutable`.
4. Adds a `<link rel="stylesheet">` element pointing at that file to the page's `<head>`.

Because the file name changes whenever the CSS changes, browsers can cache it for a year and still never see a stale version.

A page with no `<style>`, or one that contains only whitespace, gets no stylesheet and no `<link>`.

### Rules for the `<style>` element

- It must be at the top level of the page, not inside another element.
- Its position does not matter. By convention it goes at the end of the file.
- Use **one** per page. If a page has two, only the last one is used and the earlier one is silently discarded.
- The attribute `global` (`<style global>`) leaves the block unscoped. Other attributes, such as `lang` or `media`, are ignored.
- Selectors are rewritten. `{` and `}` in CSS are fine. Template expressions do not work inside a style block.

## Styles are scoped to the file

Scoping is on unless [`rustkit.toml`](/docs/project-structure/#rustkit-toml) sets `css_scope = false`. A page, a layout, and a component each get their own scope.

The compiler adds `data-rk="…"` to every element that file owns. The value is eight hex digits from a hash of that file's style text, so two files that write the same CSS share a marker and two files that write different CSS do not. Each selector then matches only those elements. The attribute goes on the last compound, before a pseudo-element:

| You write | The browser receives |
| --- | --- |
| `h1 { margin: 0; }` | `h1[data-rk="3f9a0c12"] { margin: 0; }` |
| `.profile a:hover { color: inherit; }` | `.profile a:hover[data-rk="3f9a0c12"] { color: inherit; }` |
| `a::before { content: ""; }` | `a[data-rk="3f9a0c12"]::before { content: ""; }` |

A declaration block is copied as written, so a nested rule inside it is not rewritten. Write that selector as its own rule.

A component's elements keep the component's marker after it is inlined. Children passed through `<slot />` keep the parent's marker, because they were written in the parent. A file with no `<style>`, or with `<style global>`, is not marked, and a parent's selectors do not reach into it.

`html`, `body`, and `:root` are left as written, including their pseudo-classes and pseudo-elements. The document shell is not part of the template, so a scope attribute on those selectors would match nothing. That is why `body { font: 16px system-ui; }` in a page still styles the page.

### A rule that should apply everywhere

`:global(selector)` scopes the rest of the rule and emits `selector` unchanged. A `:global { … }` block emits its rules unchanged. `<style global>` does both for the whole block, and does not mark the file's elements.

```rk title=banner.rk
<p class="copy">This page.</p>
<p class="banner">Everywhere this sheet loads.</p>

<style>
  .copy { color: #036; }
  :global(.banner) { font-weight: 700; }
  :global {
    .rk-loading { opacity: 0.6; }
  }
</style>
```

`.rk-loading` and `.rk-error` are inserted by the runtime, not by the template, so they have no `data-rk`. Style them with `:global`.

### Turning scoping off

`rustkit.toml` in the crate root, next to `Cargo.toml`:

```toml
css_scope = false
```

Missing file, or a file that does not set `css_scope`, means scoping stays on. The only value is `true` or `false`. `build.rs` recompiles when this file changes. `compile` takes the same choice as `Config.css_scope`.

## Sharing styles between pages

A [`layout.rk`](/docs/pages-and-routes/#layouts) `<style>` is concatenated ahead of each wrapped page's `<style>`, in one hashed file per page. A [component's](/docs/pages-and-routes/#components) `<style>` is concatenated ahead of the file that imports it, nested components first. Each file's selectors carry that file's `data-rk`, so a page rule does not restyle a component, and the page's rules win only where both match the same element. The same component's CSS is included once per page. There is still no single stylesheet URL shared across pages. Your options today:

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
:global(.rk-loading) { opacity: 0.6; min-height: 3rem; }
```

### `.rk-error`

If the fetch fails (a network error, or any non-2xx response), the region shows:

```html
<p class="rk-error" role="alert">Couldn't load this. <button type="button" data-rk-retry="activity">Retry</button></p>
```

If the source had loaded successfully earlier in the page's life, the previous content stays visible above this message. Clicking Retry fetches the source again.

```css
:global(.rk-error) { color: #b00020; }
:global(.rk-error button) { margin-inline-start: 0.5rem; }
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
- Selectors are scoped to the file that wrote them, with `data-rk`. `:global`, `<style global>`, and `css_scope = false` opt out.
- `.rk-loading` and `.rk-error` mark client-loading states. Style them with `:global`.
- A layout's CSS is copied into each page's sheet, still scoped to the layout. A stylesheet shared by URL is not available yet.

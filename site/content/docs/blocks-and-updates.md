---
title: Blocks and updates
description: Which parts of a page become blocks, how the browser re-renders them, and the limits of whole-block updates.
section: Core concepts
order: 5
---

A **block** is the unit of update in Rust-kit. The compiler finds the smallest template nodes that can change after the page loads, Rust renders their initial HTML between comment markers, and the browser gets a function that re-renders each one. Everything outside a block is static HTML that the browser never touches.

Knowing what becomes a block helps you predict what re-renders, what the browser downloads, and where focus can be lost.

## What becomes a block

A node becomes a block if **its own expressions** read `$state`, or if it loads data in the browser:

| Node | Becomes a block when | What re-renders |
| --- | --- | --- |
| `{expr}` | The expression reads state | Just that text |
| An element | One of its bound or mixed attributes reads state | The whole element, including its children |
| `{#if}` … `{/if}` | Any condition in the chain reads state | The whole `{#if}`/`{:else}` chain |
| `<DataSource load="client">` | Always | Its contents, when the data arrives |
| `{#each}` | Never: looping over state is not supported | — |
| Text, static elements | Never | — |

"Its own expressions" matters. In `<p>Clicked {count} times</p>`, the `<p>` has no state-reading attributes, so it is not a block. Only the `{count}` text is.

```rk title=blocks.rk
<script>
  let open = $state(false);
  let count = $state(0);
</script>

<h1>Static heading</h1>
<p>Clicked {count} times</p>
<button aria-pressed={open} onclick={() => open = !open}>Toggle</button>
{#if open}
  <p>Open. The count is {count}.</p>
{/if}
<button onclick={() => count += 1}>Add one</button>
```

This page has four blocks:

1. `{count}` in the first paragraph, depending on `count`.
2. The first `<button>`, because `aria-pressed` reads `open`. Its handler does not make it a block; handlers never do.
3. The `{#if open}` chain, depending on `open`.
4. `{count}` inside the `{#if}`, a **nested** block depending on `count`.

The heading, the second button, and both `<p>` elements outside the `{#if}` are plain HTML. Expand "Generated JavaScript" under the sample to see one render function per block, each with its `deps`.

## Markers

Rust writes each block's initial HTML between a pair of comments:

```html
<p>Clicked <!--rk:b3-->0<!--/rk:b3--> times</p>
```

The `b3` key identifies the block's render function in the page module. When the page module starts, the runtime walks the document for these markers and *adopts* the existing HTML. There is no hydration pass: nothing is re-rendered on load, and the DOM Rust produced is the DOM the visitor keeps until something changes.

Blocks that the browser creates later (for example, the nested `{count}` after `{#if open}` first turns true) get runtime ids like `<!--rk:i7-->`.

> [!WARNING]
> Do not move, remove or insert nodes between markers from your own scripts. The runtime replaces everything between a block's markers when it re-renders, and it relies on the markers being where it left them.

## What happens on an update

1. A handler assigns to state, for example `$s.count = 1`.
2. The runtime records which state keys changed and schedules one update in a microtask. Further assignments before then join the same update.
3. It finds every live block whose dependencies include a changed key, in document order.
4. For each block, it calls the block's render function to produce an HTML string, parses it in a `<template>` element, discovers any nested blocks in the new HTML, removes the old nodes between the markers, and inserts the new ones.
5. If an outer block and a nested block both depend on the changed state, only the outer one runs; it re-creates the nested block as part of its own output.

Event handlers inside a re-rendered block keep working without being re-attached, because events are delegated from `document` (see [State and events](/docs/state-and-events/)).

## Granularity and its costs

An update replaces a block's DOM nodes wholesale. There are no per-attribute patches and no keyed lists in this release. That is simple and predictable, but it has consequences for anything inside a re-rendered block:

- **Focus is lost.** If the focused element is inside the block, the new copy is not focused.
- **Uncommitted input is lost.** Text typed into an `<input>` inside the block, a checkbox the visitor ticked, a `<details>` they opened: all revert to what the template says.
- **Scroll position, selection, and running CSS transitions** inside the block reset.
- **DOM changes you made by hand** inside the block disappear.

The practical rule: **keep form controls the visitor is typing into outside blocks that change while they type.** In particular, do not bind an input's `value` to the state its own `oninput` sets:

```rk title=search-bad.rk
<script>
  let query = $state("");
</script>

<input value={query} oninput={(e) => query = e.target.value} />
```

Here the `<input>` is a block, because `value` reads `query`. Every keystroke re-renders it, and the visitor loses focus after one character. Leave the input uncontrolled and render the state elsewhere:

```rk title=search-good.rk
<script>
  let query = $state("");
</script>

<input oninput={(e) => query = e.target.value} />
{#if query}
  <p>Searching for “{query}”</p>
{/if}
```

Larger blocks cost more to re-render and lose more local DOM state. If an `{#if}` wraps a large region but only a small part depends on fast-changing state, the nested part becomes its own block and updates alone; the outer `{#if}` re-renders only when its own condition's state changes.

Finer-grained updates and keyed lists are on the [roadmap](/docs/roadmap/).

## Client-loaded data blocks

A `<DataSource load="client">` is always a block, with no state dependencies. It re-renders when its data arrives.

- **If it is visible in the server HTML**, Rust renders a `<p class="rk-loading" aria-busy="true">Loading…</p>` placeholder, and the runtime starts fetching as soon as the page module starts.
- **If it is inside a state-dependent block**, nothing is fetched until that block first renders it. In the account demo, the activity feed is not requested until the visitor clicks "Show recent activity".
- Each source is fetched **once per page load**. If a block is hidden and shown again, it reuses the data it already has.
- If the request fails, the block shows a `.rk-error` message with a Retry button, and keeps showing any data it had before.

See [Loading in the browser](/docs/client-loading/) and [Styling](/docs/styling/) for the placeholder classes.

## Server data inside blocks

A block re-rendered by the browser cannot call a server-only resolver. So when a block reads a `load="server"` binding, the compiler serializes **only the fields it reads** into the [handoff payload](/docs/handoff-payload/), and the block's render function reads them from there.

For the same reason, a `load="server"` source may not appear *inside* a block. The block might first render long after the response was sent:

```rk title=late.rk fail
<script>
  import { account } from "$sources";
  let open = $state(false);
</script>

{#if open}
  <DataSource source={account} load="server">
    <p>{data.displayName}</p>
  </DataSource>
{/if}
```

Either load it outside the `{#if}` and read it inside, or switch to `load="client"`.

## Looking at the runtime

The browser runtime is about 200 lines of plain JavaScript, shipped once and shared by every interactive page (roughly 2.7 KB gzipped). At a user level it does five things:

1. `state(init)` wraps your initial values in a `Proxy` that reports assignments.
2. `start()` reads the payload, registers one delegated listener per event type, and adopts the server-rendered blocks.
3. It keeps a table of live blocks and handlers, disposing those inside a block before re-rendering it.
4. It fetches client-loaded sources from `/_rk/source/{name}` and re-renders their blocks on arrival.
5. It handles Retry clicks.

See [Runtime API](/docs/runtime-api/) for the details and [Generated code](/docs/generated-code/) for what a page module looks like.

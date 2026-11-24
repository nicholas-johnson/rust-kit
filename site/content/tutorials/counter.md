---
title: A counter
description: Add a page with $state and click handlers, then look at the block markers and the small module the compiler generates.
order: 2
level: Beginner
duration: 20 min
---

## What you'll build

A counter page at `/counter` with three buttons: **Add one**, **Add five** and **Reset**. A message appears once you've clicked a few times, and **Reset** is disabled while the count is zero.

The page is still rendered by Rust on the server, so it reads correctly before any script runs. This time the compiler also emits a small JavaScript module, because the page has state and event handlers. By the end you'll know how to read both halves: the comment markers in the HTML, and the render functions in the module.

## Prerequisites

- The `hello-rk` project from [Your first page](/tutorials/your-first-page/). You'll add one file to it and change no Rust.

## Step 1: Declare some state

Create `src/routes/counter.rk` and start with the script:

```rk partial title=counter.rk
<script>
  let count = $state(0);
</script>
```

`$state(0)` declares a reactive variable named `count` with the initial value `0`. Two rules apply here, because the script is read line by line:

- Each `let x = $state(…);` goes on its own line.
- The initial value must be a literal: a string, a number, a boolean, or `null`.

The second rule exists because the server renders the first version of the page. Rust has to know what `count` is before any JavaScript runs, so the value must mean the same thing in both languages. Arrays and objects in state aren't supported yet; see the [roadmap](/docs/roadmap/).

## Step 2: Show it and change it

Here's the whole page. Replace the file with it:

```rk title=counter.rk
<script>
  let count = $state(0);

  function bump(by) {
    count = count + by;
  }
</script>

<title>Counter</title>

<main>
  <h1>Counter</h1>
  <p>You have clicked <strong>{count}</strong> times.</p>

  <button onclick={() => count = count + 1}>Add one</button>
  <button onclick={() => bump(5)}>Add five</button>
  <button onclick={() => count = 0} disabled={count === 0}>Reset</button>

  {#if count >= 10}
    <p class="note">Ten or more. That is dedication.</p>
  {:else if count > 0}
    <p>Keep going.</p>
  {/if}
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button { font: inherit; padding: 0.4rem 0.9rem; }
  .note { color: #b7410e; font-weight: 600; }
</style>
```

There are four new ideas on this page:

- **`{count}`** interpolates a value into the page. Text is always HTML-escaped.
- **`onclick={…}`** attaches an event handler. The value is a JavaScript function, and it runs only in the browser, so you can write any JavaScript you like. Assigning to a state variable (`count = count + 1`) is what triggers an update.
- **`function bump(by)`** shows that the rest of the script is ordinary JavaScript. Functions you declare there can be called from handlers, and assignments to state inside them work the same way.
- **`disabled={count === 0}`** binds an attribute. When the expression is `true`, you get a bare `disabled`. When it's `false`, the attribute is left out.

`{#if}`, `{:else if}`, `{:else}` and `{/if}` choose what to show. The conditions use the template expression language, which is deliberately small. More on that in a moment.

## Step 3: Run it

The counter needs no Rust changes. The build script picks up the new file automatically:

```bash
cargo run
```

Open [http://127.0.0.1:3000/counter](http://127.0.0.1:3000/counter). You should see "You have clicked 0 times." with **Reset** greyed out. Click **Add one**: the number goes up, **Reset** becomes active, and "Keep going." appears. Click **Add five** twice and the message changes to "Ten or more. That is dedication." Click **Reset** and everything returns to how it started.

## Step 4: Read the server HTML

View the page source, or run `curl -s http://127.0.0.1:3000/counter`. The body looks like this:

```html
<main>
  <h1>Counter</h1>
  <p>You have clicked <strong><!--rk:b3-->0<!--/rk:b3--></strong> times.</p>

  <button data-rk-on="click:h0">Add one</button>
  <button data-rk-on="click:h1">Add five</button>
  <!--rk:b8--><button disabled data-rk-on="click:h2">Reset</button><!--/rk:b8-->

  <!--rk:b9--><!--/rk:b9-->
</main>
```

Rust rendered the page with `count` set to its initial value, `0`. Three details are worth noticing:

- **Block markers.** Every part of the template that reads state became a *block*, wrapped in `<!--rk:bN-->` and `<!--/rk:bN-->` comments. `b3` is the number. `b8` is the **Reset** button, because its `disabled` attribute reads `count`. `b9` is the `{#if}`, which is empty because neither branch is true at `0`. The numbers come from the position of each node in the template, so yours may differ if your page differs.
- **Handlers are names, not code.** Each button has a `data-rk-on` attribute such as `click:h0`. The handler code lives in the module. The HTML only says which one to call.
- **Static parts stay static.** The heading, the "Add one" and "Add five" buttons, and the text around the number aren't blocks. Nothing in them reads state, so the browser never touches them.

The response now ends with a `<script type="module">` tag, and the `<head>` has a `modulepreload` link for the same file.

## Step 5: Read the generated module

The compiled sample in Step 2 has a **Generated JavaScript** section. Expand it, or fetch the real file. Its name is in the page's `<script type="module" src="…">`:

```bash
curl -s http://127.0.0.1:3000/counter | grep -o '/_rk/assets/counter[^"]*\.js'
```

The module starts like this:

```js
import { state, start } from "./runtime.70c435b7.js";
const $s = state({ "count": 0 });
// page script
function bump(by) {
    $s.count = $s.count + by;
  }
// compiled template
start($s, {
events: ["click"],
blocks: {
b3: { deps: ["count"], render: (c, r) => { let h = "";
h += r.esc($s.count);
return h; } },
```

Here's how to read it:

- **`$s` is the state store.** Your `let count = $state(0)` became a property on a reactive object. The compiler rewrote every read and write of `count`, including the ones inside `bump`, to `$s.count`. Assigning to it tells the runtime which state changed.
- **`blocks`** holds one render function per block marker. Each lists its `deps`, the state it reads. When `count` changes, the runtime re-renders every block whose `deps` include `"count"` and replaces the HTML between its markers. Updates are batched in a microtask, so two assignments in one click cause one re-render.
- **`handlers`** maps `h0`, `h1` and `h2` to your arrow functions.
- **`ssr`** tells the runtime how to rebuild each block's scope from data the server sent. This page reads no server data, so every entry is empty. You'll see it fill up in [Editing a profile](/tutorials/profile-form/).

The `runtime.*.js` file is shared by every interactive page, so a browser downloads it once. It finds the markers, listens for events on the document, and calls your handlers.

Compare this with `/`, which still has no `<script>` tag. JavaScript is decided page by page.

## Step 6: Hit the edges of the expression language

Event handlers can be any JavaScript, because they only run in the browser. Template expressions are different. `{count}`, `disabled={…}` and `{#if …}` are evaluated by Rust on the server *and* by JavaScript in the browser, and both have to produce exactly the same text. So expressions are limited to field reads, `.length`, literals, `!`, `&&`, `||`, `===`, `!==`, `<`, `<=`, `>` and `>=`.

Try showing the next value:

```rk fail title=counter.rk
<script>
  let count = $state(0);
</script>

<p>Next: {count + 1}</p>
```

The build fails, pointing at the `+`. Arithmetic is easy to do in a handler (`count = count + 1`), but it isn't part of the template language. The same goes for loose equality: `count == 0` fails with a hint to use `===`, and a function call like `{format(count)}` fails with a hint to compute the value somewhere else. For server data, that somewhere else is the resolver, which you'll meet in the next tutorial.

> [!TIP]
> If you need a derived value in the template, keep it in state and update it in the handler. For example, add `let next = $state(1);` and have the handler set both `count` and `next`.

## What just happened

The compiler walked your template twice from the same tree:

1. **The Rust walk** produced the `render` function. It evaluates `{count}` and `count === 0` using the initial values from your `$state` declarations, through `rustkit::js::Val`. That type follows JavaScript's rules for truthiness, equality and number formatting, which is why `0` prints as `0` and not `0.0`.
2. **The JavaScript walk** produced a render function for each block, plus the handlers. Because both walks start from the same template, the browser's re-render of a block matches what the server would have produced for the same state.

When the module loads, the runtime *adopts* the server's HTML. It doesn't re-render the page or replace anything. It finds the markers, records where each block starts and ends, and waits. Only a state change causes DOM work, and only inside the affected blocks.

Two limits are worth knowing now:

- An update replaces the whole block. If a focused input sits inside a block that re-renders, it loses focus. Keep inputs outside the blocks that change while someone types, where you can.
- There are no keyed lists or per-attribute patches yet. Both are on the [roadmap](/docs/roadmap/).

## Next steps

- [Loading data on the server](/tutorials/server-data/) adds a data source and renders a list from it.
- [State and events](/docs/state-and-events/) covers `$state` and handlers in depth.
- [Blocks and updates](/docs/blocks-and-updates/) explains block markers and re-rendering.
- [Expressions](/docs/expressions/) and [JS value semantics](/docs/js-value-semantics/) describe exactly what a template expression can do.

---
title: Template syntax
description: The complete reference for page markup, including text, expressions, attributes, if and each blocks, DataSource, and event handlers.
section: Core concepts
order: 2
---

The template is everything in a page that is not a `<script>` or `<style>`. It is HTML with a few additions. This page lists every construct the compiler understands. For what may go *inside* the braces, see [Expressions](/docs/expressions/).

## Text

Text is copied to the output exactly as written, including HTML entities. `&amp;`, `&nbsp;` and `&copy;` reach the browser unchanged.

A `{` always starts an expression. To show a literal brace, write the entity: `&#123;` for `{` and `&#125;` for `}`.

```rk title=text.rk
<p>Fish &amp; chips &mdash; &copy; 2026</p>
<p>Write &#123;name&#125; to interpolate.</p>
```

## Interpolation: `{expr}`

`{expr}` inserts the value of an expression as text. The value is always HTML-escaped: `&`, `<`, `>`, `"` and `'` become entities, in Rust and in the browser alike. There is no way to insert raw HTML.

| Value | Renders as |
| --- | --- |
| String | The string, escaped |
| Number | JavaScript formatting: `3`, `0.5`, `1e+21`. `NaN` and `Infinity` fields render as empty text |
| `true` / `false` | `true` / `false` |
| `null`, `undefined`, Rust `None` | Nothing (empty text) |

```rk title=interpolation.rk
<script>
  import { account } from "$sources";
</script>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
  <p>Your name is {data.displayName.length} characters long.</p>
</DataSource>
```

Names you can read are `$state` variables, the binding of an enclosing `<DataSource>`, and the item and index of an enclosing `{#each}`. Anything else is a compile error: `` `x` is not defined here ``.

## Elements

Write HTML elements as usual. Tag names starting with a lowercase letter are emitted as written.

- **Void elements** (`area`, `base`, `br`, `col`, `embed`, `hr`, `img`, `input`, `link`, `meta`, `source`, `track`, `wbr`) need no closing tag. `<br>` and `<br />` are equivalent.
- **Self-closing syntax on other elements** is accepted and produces an empty element: `<div />` renders `<div></div>`.
- **Every other element must be closed**, and closing tags must match: `` `</span>` does not match `<p>` `` is a compile error. The compiler does not apply HTML's implicit closing rules, so close your `<p>` and `<li>` tags.
- **Tags starting with an uppercase letter are components.** The only one available is `<DataSource>`; anything else is an error.

## Attributes

| Form | Example | Output |
| --- | --- | --- |
| Bare | `<input disabled>` | `disabled` |
| Static, quoted | `class="card"` or `class='card'` | `class="card"` (as written, entities kept) |
| Static, unquoted | `width=40` | `width="40"` |
| Bound | `value={data.displayName}` | See boolean rules below |
| Bound, quoted | `value="{data.displayName}"` | Same as bound |
| Mixed | `class="card card-{kind}"` | Text and values joined, values escaped |
| Event handler | `onclick={() => open = true}` | See [Event attributes](#event-attributes) |

### Boolean rules for bound attributes

An attribute whose whole value is one expression follows these rules, identical in Rust and in the browser:

| Value | Output |
| --- | --- |
| `true` | Bare attribute: `disabled` |
| `false`, `null`, `undefined` | Attribute omitted entirely |
| Anything else | `name="escaped text"` |

This makes `disabled={busy}`, `checked={done}` and `hidden={!open}` work as you would expect.

In a **mixed** attribute, each expression is rendered as text, so `true` becomes the word `true` and `null` becomes nothing.

```rk title=attributes.rk
<script>
  let busy = $state(false);
  let size = $state("small");
</script>

<input type="checkbox" checked disabled={busy} />
<button class="btn btn-{size}" disabled={busy}>Save</button>
<button onclick={() => busy = !busy}>Toggle busy</button>
<button onclick={() => size = "large"}>Make it large</button>
```

> [!TIP]
> Because `false` omits the attribute, `aria-expanded={open}` renders nothing when `open` is false instead of `aria-expanded="false"`. For ARIA attributes that need the literal string, keep a string in state (`$state("false")`) or write two branches with `{#if}`.

There are no directives: `class:active={x}`, `bind:value={x}` and `{...spread}` are not supported. A name such as `bind:value` is written out as a plain attribute and does nothing.

## If blocks

```rk title=if.rk
<script>
  let step = $state(1);
</script>

{#if step === 1}
  <p>Choose a plan.</p>
{:else if step === 2}
  <p>Enter your details.</p>
{:else}
  <p>All done.</p>
{/if}

<button onclick={() => step = step + 1}>Next</button>
```

- Conditions use JavaScript truthiness: `""`, `0`, `NaN`, `null`, `undefined` and `false` are false; everything else is true, including `"0"`.
- `{:else if …}` can repeat; `{:else}` must come last.
- Every `{#if}` needs a matching `{/if}`.

If a condition reads `$state`, the whole `{#if}` becomes a block that the browser re-renders when that state changes. See [Blocks and updates](/docs/blocks-and-updates/).

## Each blocks

```rk title=each.rk
<script>
  import { activity } from "$sources";
</script>

<DataSource source={activity} load="server" let:feed>
  {#if feed.items.length === 0}
    <p>No activity yet.</p>
  {:else}
    <ol>
      {#each feed.items as item, i}
        <li class="entry entry-{i}">{item.description}</li>
      {/each}
    </ol>
  {/if}
</DataSource>
```

- The syntax is `{#each list as item}` or `{#each list as item, index}`. The index starts at 0.
- `list` must be a field path such as `data.items` or `item.children`. It cannot be a literal or a comparison.
- On the server, the Rust field must be something you can iterate by reference, such as a `Vec<T>`.
- Looping over `$state` is not supported, because state is limited to primitive values in this release. The compiler says so.
- There is no `{:else}` branch for empty lists; check `.length` with `{#if}` as above.
- There are no keys. When the browser re-renders a list, it re-renders the whole block. See the [roadmap](/docs/roadmap/).

## DataSource

`<DataSource>` loads data from a source in `crate::sources` and makes it available to its children.

```rk title=datasource.rk
<script>
  import { account, activity } from "$sources";
</script>

<DataSource source={account} load="server" let:user>
  <h1>Hello, {user.displayName}</h1>
</DataSource>

<DataSource source={activity} load="client">
  {#each data.items as item}
    <p>{item.description}</p>
  {/each}
</DataSource>
```

| Attribute | Required | Meaning |
| --- | --- | --- |
| `source={name}` | Yes | A source imported from `"$sources"` in the page script. |
| `load="server"` or `load="client"` | Yes | Where the data is loaded: during the request in Rust, or by the browser after the page loads. |
| `let:name` | No | The name children use for the data. Defaults to `data`. |

No other attributes are accepted. A `load="server"` source cannot be placed inside a region the browser renders (for example, inside `{#if}` on state), because that region may first appear long after the response has been sent. Use `load="client"` there.

The details are in [Data sources](/docs/data-sources/), [Loading on the server](/docs/server-loading/), and [Loading in the browser](/docs/client-loading/).

## Event attributes

An attribute whose name starts with `on` and has more letters after it is an event handler. Its value must be a single `{…}` containing JavaScript that evaluates to a function:

```rk title=events.rk partial
<button onclick={() => count = count + 1}>Add one</button>
<input oninput={(e) => query = e.target.value} />
<form onsubmit={confirmSubmit}>…</form>
```

- The event name is the rest of the attribute name, so write it in lowercase: `onclick`, `oninput`, `onkeydown`.
- The value can be any JavaScript. It runs only in the browser, so the expression-language limits do not apply.
- A quoted string like `onclick="doThing()"` is a compile error.

How handlers find state, page functions and server data is covered in [State and events](/docs/state-and-events/).

## Comments

HTML comments (`<!-- … -->`) are removed at compile time and never reach the browser. Use them freely for notes.

## Not supported

These are compile errors or are not recognised in this release:

| Construct | Status |
| --- | --- |
| `{@html …}`, `{#await}`, `{#key}`, other `{#…}` blocks | Compile error: unknown block |
| Components other than `<DataSource>` | Compile error: unknown component |
| Function and method calls in `{…}` | Compile error; see [Expressions](/docs/expressions/) |
| `{#each}` over `$state` | Compile error |
| `class:`, `bind:`, `use:` directives, spread attributes | Not recognised |
| Slots, layouts, `<svelte:head>`-style head content | Not available |

For example, a component import fails like this:

```rk title=card.rk fail
<h1>Products</h1>
<Card title="Lamp" />
```

See the [roadmap](/docs/roadmap/) for what is planned.

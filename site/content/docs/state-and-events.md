---
title: State and events
description: Declaring $state, writing event handlers, how the compiler rewrites state access, and how events are delegated.
section: Core concepts
order: 4
---

Interactivity in Rust-kit has two halves. **State** is a handful of named values declared in the page script. **Event handlers** are JavaScript functions attached with `on…` attributes. When a handler assigns to state, the parts of the template that read that state re-render in the browser.

## Declaring state

```rk title=counter.rk
<script>
  let count = $state(0);
  let step = $state(1);

  function reset() {
    count = 0;
  }
</script>

<p>Count: {count}</p>
<button onclick={() => count += step}>Add {step}</button>
<button onclick={() => step = step === 1 ? 10 : 1}>Toggle step size</button>
<button onclick={reset}>Reset</button>
```

The rules:

- Each declaration is `let name = $state(initial);` on **its own line**. The page script is read line by line; two declarations on one line do not parse.
- Use `let`. A `const` or `var` line is not recognised as state and is passed through as ordinary JavaScript, where `$state` does not exist, so the page fails in the browser.
- The initial value must be a literal **string, number, boolean or `null`**. `$state()` with nothing inside means `null`.
- Arrays, objects and computed initial values are not supported yet:

```rk title=todos.rk fail
<script>
  let todos = $state([]);
</script>

<p>Nothing to do.</p>
```

Why literals only? Rust renders the page with the initial value, and the browser starts from the same value. A literal is the only thing guaranteed to mean the same in both languages. Array and object state, and the keyed lists that go with them, are on the [roadmap](/docs/roadmap/).

## Reading state in the template

State variables can be read in any template expression: `{count}`, `{#if open}`, `disabled={busy}`. On the server, they always have their initial value, so the first HTML shows `Count: 0`.

Any template node whose own expressions read state becomes a **block**. The browser re-renders exactly those blocks when the state they read changes. See [Blocks and updates](/docs/blocks-and-updates/).

State is per page load. It is not persisted, not shared between tabs, and not sent to the server. A reload starts from the initial values again.

## Changing state

Assign to the variable as you would in plain JavaScript:

```js
count = count + 1;
count += 1;
count++;
open = !open;
query = event.target.value;
```

You can do this in a handler or in any function in the page script.

### What the compiler rewrites

State lives in a reactive object called `$s` in the generated module. The compiler rewrites every *read and write* of a state name, both in handlers and in the rest of the page script, to go through it:

| You write | Generated |
| --- | --- |
| `() => editing = !editing` | `() => $s.editing = !$s.editing` |
| `` `${count} items` `` | `` `${$s.count} items` `` |
| `save({ editing })` | `save({ editing: $s.editing })` |
| `form.editing` | unchanged (a property, not the variable) |
| `"editing"` | unchanged (inside a string) |

Assigning a new value to a property of `$s` schedules an update if the value actually changed (`Object.is`). Updates are batched in a microtask, so several assignments in one handler cause one re-render of each affected block.

> [!WARNING]
> The rewrite is a scanner, not a full JavaScript parser. It does not understand scopes, so a parameter or local variable with the same name as a state variable is rewritten too, and the result will not work. Give locals different names. It also does not recognise regular-expression literals; if a regex contains a quote or a state name, move it into a function in the page script that does not mention state.

## Event handlers

Any attribute named `on` plus an event name is a handler:

```rk title=search.rk
<script>
  let query = $state("");
</script>

<label>Search <input oninput={(e) => query = e.target.value} /></label>
{#if query}
  <p>You typed: {query}</p>
{/if}
```

- The value is a `{…}` containing JavaScript that **evaluates to a function**: an arrow function, a function expression, or the name of a function from the page script (`onclick={reset}`).
- The function is called with the DOM event as its argument and with `this` set to the element carrying the attribute.
- Use the lowercase DOM event name: `onclick`, `oninput`, `onchange`, `onsubmit`, `onkeydown`, `onfocus`, and so on. Any event type the browser dispatches works; there is no fixed list.
- Calling `event.preventDefault()` works as usual, for example in `onsubmit`.

Because handlers only run in the browser, they can use any JavaScript: method calls, `fetch`, ternaries, `console.log`. None of the [expression limits](/docs/expressions/) apply.

### What handlers can see

A handler can use:

- **State variables**, rewritten to `$s` as above.
- **Anything declared in the page script**: functions, constants, helper variables.
- **Template bindings around it**: the `<DataSource>` binding and, inside a browser-rendered `{#each}`, the item and index.

When a handler sits in a server-rendered part of the page and reads a server-loaded binding, the compiler adds exactly the fields it reads to the [handoff payload](/docs/handoff-payload/). For example `onclick={() => copy(data.displayName)}` sends `displayName` and nothing else.

One case is not supported yet. A handler inside a `{#each}` that **Rust** renders cannot read the loop item, because the browser has no copy of the list:

```rk title=activity.rk fail
<script>
  import { activity } from "$sources";
</script>

<DataSource source={activity} load="server">
  {#each data.items as item}
    <button onclick={() => alert(item.id)}>Details</button>
  {/each}
</DataSource>
```

The diagnostic suggests the alternatives: load that source with `load="client"` so the browser renders the loop, or move the interaction out of the loop.

## How events are delegated

Rust-kit does not attach a listener to each element. Rendered elements carry a `data-rk-on` attribute naming their handlers, for example `data-rk-on="click:h0"`. The runtime adds **one listener per event type** to `document`, in the capture phase, for each event type the page uses.

When an event fires, the listener walks from the event target up to the document. For every element whose `data-rk-on` lists that event type, it calls the handler. This has a few consequences:

- Handlers keep working after a block re-renders, because nothing was bound to the old elements.
- Non-bubbling events such as `focus` and `blur` still reach your handler, because the listener is in the capture phase.
- If both an element and one of its ancestors have a handler for the same event, both run, innermost first.
- `event.stopPropagation()` inside a Rust-kit handler does **not** stop Rust-kit handlers on ancestors, because they are all called by the same document listener. Check `event.target` in the outer handler if you need to tell them apart.

## Page script code

Anything in `<script>` that is not a `$sources` import, a component import, or a `$state` line is copied into the page module, after the state object is created and before the runtime starts. It runs once per page load, in the browser only. Module scripts run after the document is parsed, so the DOM is available. A component's leftover script is copied the same way, once per page.

```rk title=clock.rk
<script>
  let now = $state("");

  function tick() {
    now = new Date().toLocaleTimeString();
  }
  setInterval(tick, 1000);
</script>

<p>Browser time: {now}</p>
```

Rust renders `now` as an empty string; the browser fills it in on the first tick. This is the right pattern for anything that depends on the visitor's machine, such as local time.

A page whose script contains only imports and state, and whose template has no blocks or handlers, ships no JavaScript at all.

`import Name from "$lib/….rk"` inlines a [component](/docs/pages-and-routes/#components). Other imports are not bundled or resolved. Keep page scripts self-contained for now.

## Related

- [Blocks and updates](/docs/blocks-and-updates/) for what re-renders and what does not.
- [The counter tutorial](/tutorials/counter/) for a guided build.
- [Runtime API](/docs/runtime-api/) for the browser runtime's internals.

---
title: Handoff payload
description: How server-loaded data reaches browser-rendered blocks without a second fetch, and how the compiler decides exactly which fields to send.
section: Data
order: 4
---

The server renders the whole page, including the initial state of every interactive block. When one of those blocks later re-renders in the browser, it may need data the server loaded. The browser cannot call the resolver, and fetching the source again would cost a round trip and an extra upstream read. Instead, the server writes the data the browser will need into the page, in a JSON script tag called the **handoff payload**.

The compiler decides what goes into it, field by field, at build time.

## An example

This is the account page from the reference app, reduced to the part that matters:

```rk title=account.rk
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
      <label>Display name <input name="name" value={data.displayName} /></label>
      <button>Save</button>
    </form>
  {/if}
</DataSource>
```

The `<h1>` is rendered only by the server, so it needs nothing from the payload. The `{#if editing}` block is different: when the visitor clicks "Edit profile", the browser renders the form, and the form's `value` reads `data.displayName`. So the server adds that one field to the page, just before the module script:

```html
<script type="application/json" id="rk-payload">{"csrf":"3f9c…","s":{"account":{"displayName":"Alice Liddell"}}}</script>
<script type="module" src="/_rk/assets/account.1a2b3c4d.js"></script>
```

When the browser module starts, it parses this once. The editor's block is rebuilt from `P.s["account"]`, and no request is made for `account`. The acceptance test checks both: the payload equals `{"displayName":"Alice Liddell"}`, and the upstream account service is read exactly once.

## How the compiler chooses fields

The compiler walks the template. For every block the browser may re-render, and for every event handler on a server-rendered element, it collects the names the block or handler reads from outside itself. Then it looks at what each name is bound to:

| The name is bound to | What happens |
| --- | --- |
| A `load="server"` `<DataSource>` | The field read on it is added to that source's projection. |
| A `load="client"` `<DataSource>` | Nothing. The browser fetches that view itself. |
| A `$state` variable, or a global such as `alert` | Nothing. |
| An item of a server-rendered `{#each}` | Compile error; see below. |

The projection records the **first** field after the binding. For example:

| Read in a browser block or handler | Added to the payload |
| --- | --- |
| `data.displayName` | `displayName` |
| `data.items.length` | the whole `items` list |
| `data.owner.name` | the whole `owner` object |
| `data` on its own, e.g. `save(data)` | the whole view |

Projections from every block and handler on the page are merged per source. If any of them uses the whole view, the whole view is sent.

### Projection versus whole view

For a projected source, the generated Rust builds a JSON object field by field, reading the snake_case Rust field and writing the camelCase name the template used:

```rust
let mut m = Map::new();
m.insert("displayName".into(), to_value(&src_account.display_name)?);
s.insert("account".into(), Value::Object(m));
```

For a whole view, it serializes the value with serde:

```rust
s.insert("account".into(), to_value(&src_account)?);
```

Prefer reading specific fields in handlers and browser blocks. A handler that passes `data` around sends everything the view holds.

> [!NOTE]
> Projection limits the payload, not the view. If another page loads the same source with `load="client"`, the full view is available from `GET /_rk/source/{name}`. The real boundary is the view struct itself; keep it to fields the browser may see. See [Data sources](/docs/data-sources/#keep-views-small-and-explicit).

## When there is no payload

The `#rk-payload` tag is only written when the page has a browser module and something needs handing off. A page with no state and no handlers has no module and no payload. A page with handlers that only touch `$state` has a module but no payload.

## The CSRF token

If a `<form method="post">` sits inside a block the browser renders, the browser needs a CSRF token to put in the form's hidden `_csrf` field. The server adds the session's token to the payload as `csrf`, and the runtime reads it from there when it renders the form.

Forms the server renders get their token written directly into the HTML. Both paths use the same token from `ctx.csrf_token()`. See [Form actions](/docs/form-actions/) and [Security](/docs/security/#csrf-protection).

## Escaping

The payload is JSON inside an HTML `<script>` element. The HTML parser ends a script at the first `</script>`, whatever the JSON means, so a display name like `</script><img src=x onerror=alert(1)>` could otherwise break out of the tag.

`rustkit::js::html_safe_json` serializes the payload and then replaces five characters with JSON escapes:

| Character | Written as |
| --- | --- |
| `<` | `<` |
| `>` | `>` |
| `&` | `&` |
| U+2028 | ` ` |
| U+2029 | ` ` |

The result is still valid JSON that decodes to exactly the same strings, and it can never contain `</script>` or `<!--`. The tag also has `type="application/json"`, so the browser never executes it. The acceptance test `user_text_is_escaped_in_html_and_payload` saves exactly that display name and checks that the page contains only two `</script>` sequences: the payload's own and the module's.

## Loop items cannot cross the boundary

The payload carries sources, not template variables. An item of a server-rendered `{#each}` exists only while the Rust renderer runs the loop. If a handler or a browser block inside that loop reads the item, the browser has no way to reconstruct it, so the compiler rejects the page:

```rk fail title=activity.rk
<script>
  import { activity } from "$sources";
</script>

<DataSource source={activity} load="server" let:data>
  <ul>
    {#each data.items as item}
      <li>{item.description} <button onclick={() => alert(item.description)}>Copy</button></li>
    {/each}
  </ul>
</DataSource>
```

In v0.1 you have two options:

1. **Load the source on the client.** Inside a `load="client"` region, the browser renders the loop, and every handler and block in it can read `item`.
2. **Move the interaction out of the loop.** For example, one button outside the list that acts on the whole view.

The diagnostic's help text also suggests making the loop depend on state. That does not help yet: wrapping the loop in a state-dependent `{#if}` still gives the same error, because the server renders the block's first version, and `{#each}` over `$state` is not supported.

Handlers in server-rendered loops that can read their item are on the [roadmap](/docs/roadmap/).

## What it costs

The payload adds bytes to the HTML, and the browser parses it once on start-up. With projection, that is usually a few dozen bytes per source. Use your browser's view-source to see the exact payload a page sends, or read the generated Rust (see [Generated code](/docs/generated-code/)).

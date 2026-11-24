---
title: Troubleshooting
description: Common problems with Rust-kit pages, sessions and forms, what causes them, and how to fix them.
section: Guides
order: 4
---

This page lists problems people run into, grouped by where they show up: the build, the browser, and HTTP responses. Each entry gives the symptom, the cause in the framework, and the fix. For a complete list of compiler messages, see [Compiler diagnostics](/docs/diagnostics/).

## Build errors

### A $state line reports a strange character

```text
error: `;` is not supported in a template expression
 --> src/routes/page.rk:2:27
  |
2 |   let open = $state(false); let count = $state(0);
  |                           ^
```

**Cause.** The page `<script>` is read line by line. A line that starts with `let` and contains `$state(` is treated as one declaration, and everything after the first `=` is its initialiser. Two declarations on one line produce an initialiser of `false); let count = $state(0`.

**Fix.** Put each `let x = $state(…)` on its own line, and use `let`; a `const x = $state(…)` line is not recognised, is passed to the browser untouched, and any template that reads `x` reports `` `x` is not defined here ``.

### A source "is not imported" although you imported it

**Cause.** The `$sources` import must be on one line. An import split across lines does not match, so it is passed to the browser as ordinary JavaScript and the compiler never registers the names.

**Fix.** Write `import { account, activity } from "$sources";` on a single line. Renaming with `as` is not supported yet.

### A function call or method is rejected

```text
error: `formatDate(…)` cannot be rendered by the Rust server renderer
```

**Cause.** Template expressions run in Rust and in JavaScript, so they cannot call functions. This includes methods such as `.toUpperCase()` and `.toFixed(2)`.

**Fix.** Do the work in the resolver and add a field to the view, for example `joined_label: String`, then read `data.joinedLabel`. Event handlers are different: they only run in the browser and can contain any JavaScript.

### `==` is rejected

**Fix.** Use `===` or `!==`. Loose equality is not implemented on the server.

### Negative numbers and arrays are rejected

`$state(-1)` fails with `` `-` is not supported `` and `$state([])` with `` `[` is not supported ``. State holds only string, number, boolean and `null` literals, and the language has no minus operator. Use `null` or another sentinel, and keep lists in a source.

### `{#each}` over state

```text
error: `{#each}` over `$state` is not supported yet
```

State is limited to primitives, so there is nothing to iterate. Iterate over a field of a source, such as `data.items`.

### A handler cannot see the loop item

```text
error: `item` comes from a server-rendered `{#each}`, so the browser cannot see it here
```

**Cause.** When Rust renders a loop, the browser later attaches handlers using scope rebuilt from the [handoff payload](/docs/handoff-payload/). The payload carries source fields, not individual loop items, so a handler inside the loop has no `item`.

**Fix.** Load the source with `load="client"`, so the browser renders the loop and has each item, or move the interaction outside the loop.

### A server source inside a state-dependent region

```text
error: a `load="server"` source cannot appear inside a region the browser renders
```

A region inside `{#if open}` may first appear after the response has been sent, when no server is rendering. Use `load="client"` for that source, or move it outside the condition.

### rustc errors in rustkit_generated.rs

The page compiled, but Rust did not. Common causes:

| rustc says | Fix |
|---|---|
| no field `display_name` on type … | The template reads `data.displayName`; the view needs a `display_name` field. |
| trait bound `…: ToJs` is not satisfied | You interpolated a `Vec`, a struct or another type with no text form. Expose a `String`, number, `bool` or `Option` of those. |
| trait bound `…: JsLength` is not satisfied | `.length` on an `Option` or another type. Use a `String`, `Vec` or slice. |
| cannot find function `x` in module `crate::sources` | Add the resolver `pub async fn x(ctx: &Ctx) -> Result<View, Error>`. |

[Generated code](/docs/generated-code/) explains how to find and read the file.

## In the browser

### Text disappears after an update

**Symptom.** The server's HTML shows a value, but after you click something the same region shows empty text. Or a list loaded with `load="client"` renders the right number of rows, each one blank.

**Cause.** Browser-rendered regions read JSON, not Rust structs. The template reads `item.description` or `data.displayName`, but the JSON has `display_name`. The browser reads a missing field as `undefined`, which renders as empty text, so there is no error. When the compiler projects a server source into the payload, it names each top-level field exactly as the template wrote it, so a direct read like `data.displayName` works even without the serde attribute. Everything else uses serde's names: fields one level deeper (such as each `item` in `data.items`), whole-object reads, and every client source.

**Fix.** Derive `Serialize` with camelCase names on every view and every struct nested in one:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub description: String,
    pub created_label: String,
}
```

Check what the browser actually receives in the `rk-payload` script tag or the `/_rk/source/{name}` response.

### The page shows "true" instead of a value

`{name || "Anonymous"}` renders `true`. In templates, `&&` and `||` always return a boolean. Use `{#if name}{name}{:else}Anonymous{/if}` or compute the fallback in the resolver. See [JS value semantics](/docs/js-value-semantics/#logical-operators-return-booleans).

### A click handler runs once, or not at all

**Cause.** A handler attribute must evaluate to a function. `onclick={save()}` calls `save` when the runtime first builds the handler and then uses its return value, which is not a function, so nothing happens on later clicks.

**Fix.** Write `onclick={() => save()}` or `onclick={save}`. Functions declared in the page script are in scope for handlers, but not for template expressions.

### Typing in an input loses focus

A state change re-renders every block that reads that state, replacing its DOM. If an input updates state that the block containing the input also reads, the input is replaced and loses focus. Keep the input outside the block that displays the state. Finer-grained updates are on the [roadmap](/docs/roadmap/).

### "Loading…" never goes away

A `load="client"` source renders a "Loading…" placeholder on the server, and the browser module fetches it. If the module fails to load, or JavaScript is off, the placeholder stays. Check the browser console and network panel. If the fetch fails, the region shows "Couldn't load this." with a Retry button instead; the status of the `/_rk/source/{name}` request tells you why.

## HTTP responses

### Signing in does nothing on plain HTTP

**Symptom.** You submit the sign-in form and get **400** "This form has expired. Reload the page and try again.", every time.

**Cause.** The session cookie is marked `Secure`, which is the default. Over plain HTTP the browser refuses to store it, so the form post arrives without a session and its CSRF token cannot match. The demo's `serve` turns `Secure` off only for addresses starting with `127.0.0.1`.

**Fix.** Locally, serve on `127.0.0.1`, or call `.secure_cookies(false)` for development builds. In production, use HTTPS and keep it on. See [Deployment](/docs/deployment/#keep-secure-cookies-on).

### "This form has expired. Reload the page and try again."

This **400** means the `_csrf` field did not match the session's token. Causes, most common first:

- **No session cookie reached the server**: the `Secure` problem above, or cookies blocked.
- **The session changed since the page loaded**: the visitor signed in or out in another tab (sign-in rotates the session), the session sat idle for 12 hours, or the server restarted, which clears all sessions.
- **The form has no token**: a form you build by hand in JavaScript, or a `fetch` post. Only `<form method="post">` elements in templates get the hidden field automatically. A form whose method is written as an expression or omitted gets none.
- **The form is on a prerendered page**: the token in the static HTML belongs to no real session. Make the page request-time.

### 403 on a form post

Actions reject requests whose `Origin` does not match `Host`, or that carry `Sec-Fetch-Site: cross-site`. Check:

- **A reverse proxy rewrote `Host`.** Forward the public host unchanged; see [Deployment](/docs/deployment/#preserve-the-host-header).
- **The form is on a different origin**, such as a separate development server on another port, or a local file (`Origin: null`). Serve the form from the app itself.

Requests without an `Origin` header are allowed, and still need a valid CSRF token.

### 403 or 401 from /_rk/source/…

`/_rk/source/{name}` returns 403 for `Sec-Fetch-Site: cross-site` requests and whatever status the resolver's error maps to otherwise: 401 if it called `require_user` with no one signed in. A 404 means no page loads that source with `load="client"`, so it has no endpoint.

### The router fails to start: a prerendered route read the session

```text
internal error: a prerendered route read the session; remove `prerender` or load this source on the client
```

**Cause.** `into_router` prerenders routes before serving, with no request. A source on that page called `require_user`.

**Fix.** Remove `export const prerender = true`, or load that source with `load="client"`. Note that `ctx.user()` does not fail during a prerender: it returns `None`, and the anonymous version of the page is baked into the HTML. Do not read the session at all from sources used by prerendered pages.

### A page you marked prerender renders per request

The compiler looks for the exact text `export const prerender = true` inside `<script context="module">`. Spacing variants such as `prerender=true`, or the line in the ordinary `<script>`, are not recognised, and the page silently stays request-time. The `build` command's `mode` column shows `static` for pages it found.

### Signed-out visitors land on /login?next=…

A page whose source returns `Error::Unauthorized` redirects to the login path with `?next=` set to the route. The framework does not act on `next`; your sign-in action decides where to go. Change the login path with `.login_path("/sign-in")`.

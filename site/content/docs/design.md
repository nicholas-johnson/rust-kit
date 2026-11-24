---
title: Design proposal
description: The November 2026 proposal Rust-kit was built from, unchanged apart from formatting.
section: Project
order: 1
---

> [!NOTE]
> This is the original design proposal, kept as written. The first release implements its first milestone; the four acceptance properties below are automated tests in `examples/account-demo/tests/acceptance.rs`. See the [roadmap](/docs/roadmap/) for what exists today. The account page sample below is compiled by the real compiler every time this site is built.

Rust-kit is a proposed web framework for writing a page as a template, a script, and a stylesheet. At build time, it compiles the template into a native Rust renderer and generates browser JavaScript for the interactive parts. The server sends ordinary HTML and CSS. A visitor can read a server-rendered account page before the first line of JavaScript runs.

The authoring model is SvelteKit-inspired. The distinguishing feature is where each language executes. Rust handles requests, sessions, service calls, and initial rendering. Author-written JavaScript runs in the browser. Production deployment needs neither a server-side JavaScript engine nor a browser Wasm runtime.

## One account page

Imagine an account page with a greeting, a profile editor, and a recent-activity feed. The greeting belongs in the first response. The editor needs a click handler. Activity can wait until the visitor asks for it. The author should be able to express all three decisions in one place.

The syntax below is illustrative. A source import names a registered data contract. The compiler resolves it separately for the server and the browser.

```rk title=account.rk
<script>
  import { account, activity } from "$sources";
  let editing = $state(false);
  let showActivity = $state(false);
</script>

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

<button onclick={() => showActivity = true}>Show recent activity</button>
{#if showActivity}
  <DataSource source={activity} load="client" let:data>
    {#each data.items as item}
      <p>{item.description}</p>
    {/each}
  </DataSource>
{/if}

<style>
  h1 { margin-block-end: 1rem; }
</style>
```

On a visit to the page, Rust loads the account and renders the greeting and both buttons. The client module attaches handlers to the existing controls. Opening the editor uses the initial display name. Opening recent activity for the first time makes a request and inserts the resulting items into the live page. Saving the form posts to a Rust action, which checks the session, validates the new name, calls the upstream service, and redirects back to the page.

## One source, two ways to call it

A data source has a stable name, typed inputs and output, and a Rust resolver. The resolver translates application concepts into REST calls. A template can ask for the current account without knowing the service URL, the authentication scheme, or the shape of its response.

```rust
// Proposed application code
async fn account(ctx: &RequestContext) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(user.id).await?;
    Ok(AccountView { display_name: record.display_name })
}
```

During SSR, Rust calls this function directly. The accounts service may still be a network hop. For a source exposed to the browser, the compiler also generates a same-origin HTTP handler and a small JavaScript client. Both paths execute the same resolver with the current request's session. Only the declared view fields cross into the page. A display-only, server-loaded source needs neither a public endpoint nor a serialized copy of its entire result.

| Source use | Initial response | Later browser behaviour |
|---|---|---|
| Server-loaded greeting | HTML containing the name | No request needed to display it |
| Client-loaded activity | A button or a loading region | Fetch when opened, then update that region |
| Server-loaded list with refresh | HTML containing the first set of rows | Fetch again on refresh and update the rows |

A failed server read can select a route error response before rendering. A failed client refresh should leave the previous rows visible and offer a retry in that region. Independent source reads can start together. Repeated reads with the same inputs can share a result within the request.

## What the compiler owns

The compiler parses the template once. For initial rendering it emits Rust that escapes bound text and attributes, evaluates conditionals, and expands loops. For browser updates it emits JavaScript for the bindings affected by a state change. CSS becomes a normal asset. A fully static route can produce HTML and CSS alone.

The language boundary needs a clear rule. A field read, comparison, loop, or literal initial state can have defined behaviour in both targets. Arbitrary JavaScript is valid in a click handler. An imported JavaScript formatter used in an initial heading presents a different problem: Rust needs to produce precisely the same text. The first compiler should diagnose that expression at build time and point the author towards a derived field on the resolver. This keeps the initial template language small enough to specify and test.

The compiler also decides what each island needs at startup. In the account example the editor needs the initial display name, so Rust can include that field in an HTML-safe payload. If the page only printed a greeting, the rendered text would suffice. Client-loaded data changes the live DOM after delivery. It cannot retroactively become part of the original HTTP response.

## Rendering and sessions

Public routes with build-time inputs can run the Rust renderer during the build and deploy as static files. Personalised routes use the same renderer at request time. A public article can also be prerendered with a client-loaded account menu if personalisation can wait. The author chooses when fresh data matters.

Rust owns the session lifecycle. An opaque session cookie identifies the server-side session. The request handler supplies a scoped context to resolvers and actions, and upstream credentials stay on the server. The generated API applies the same access checks as SSR. Personalised responses stay out of shared caches, and cookie-authenticated writes need CSRF protection.

## First implementation

A useful first release would support one public article and the account page above. Its acceptance tests would establish four properties:

1. The article builds to a static file and ships zero JavaScript when it has no interactions.
2. An authenticated request returns the correct greeting in raw HTML. Connecting the browser handlers causes no second account read.
3. Opening activity issues one same-origin request. Two signed-in users receive only their own data through both the HTML and API paths.
4. Saving a profile works through an ordinary form action and the next page response reflects the upstream change.

Measure render CPU time separately from REST latency, and measure browser JavaScript bytes per route. Native code is an architectural choice, not yet a performance result. The first milestone succeeds when the page reads correctly before JavaScript, responds correctly afterwards, and the developer has written each data resolver and each piece of markup once.

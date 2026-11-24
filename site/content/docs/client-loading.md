---
title: Client loading
description: How load="client" renders a placeholder, fetches the view from a generated endpoint, caches it for the page, and handles failure.
section: Data
order: 3
---

`load="client"` tells the compiler that a source's data does not belong in the first response. The server renders a loading placeholder, and the browser fetches the view afterwards from an endpoint the compiler generates. The resolver is the same one server loading uses; only the moment and the transport differ.

Use it for data that appears after an interaction, data from a slow service that should not hold up the page, or personal data on a page that is otherwise [prerendered](/docs/prerendering/).

## A client-loaded feed

This is the activity feed from the reference app, trimmed down:

```rk title=activity.rk
<script>
  import { activity } from "$sources";
  let showActivity = $state(false);
</script>

<title>Activity</title>

<button onclick={() => showActivity = true}>Show recent activity</button>
{#if showActivity}
  <DataSource source={activity} load="client" let:data>
    {#each data.items as item}
      <p>{item.description}</p>
    {/each}
  </DataSource>
{/if}
```

The resolver is ordinary:

```rust
pub async fn activity(ctx: &Ctx) -> Result<ActivityView, Error> {
    let user = ctx.require_user()?;
    let records = ctx.accounts_service.activity(&user.id).await?;
    Ok(ActivityView {
        items: records.into_iter().map(|r| ActivityItem { description: r.description }).collect(),
    })
}
```

Nothing is fetched when the page loads, because the `<DataSource>` sits inside `{#if showActivity}` and that starts out false. The first click renders the block, the block asks for `activity`, and the browser makes one request.

## The generated endpoint

For every source that at least one page loads with `load="client"`, the compiler adds the source's name to the manifest's `client_sources` list. The router then serves:

```http
GET /_rk/source/activity HTTP/1.1
Cookie: rk_session=…
Accept: application/json
```

```http
HTTP/1.1 200 OK
content-type: application/json
cache-control: private, no-store
vary: Cookie

{"items":[{"description":"Signed in from a new browser"},{"description":"Changed notification settings"},{"description":"Downloaded March invoice"}]}
```

The endpoint builds a `RequestContext` from the request's cookie, exactly as a page render does, and calls `crate::sources::activity(ctx)`. The body is the whole view serialized with serde, which is why views need `#[serde(rename_all = "camelCase")]` (see [Data sources](/docs/data-sources/#why-views-use-camelcase)).

### Only client sources get endpoints

A source that is only ever loaded with `load="server"` has no endpoint. In the demo, `account` is server-only, so:

```http
GET /_rk/source/account HTTP/1.1

HTTP/1.1 404 Not Found
content-type: application/json

{"error":"Not found."}
```

This keeps your HTTP surface as small as your pages need. If no page in the app uses `load="client"`, the `/_rk/source/{name}` route is not registered at all, and the path falls through to the ordinary HTML 404 page.

> [!IMPORTANT]
> The list is app-wide. If any page loads `account` on the client, `GET /_rk/source/account` exists for every signed-in user, and it returns the entire `AccountView`. Field projection only applies to the [handoff payload](/docs/handoff-payload/), not to this endpoint. Treat every field of a client-loaded view as public to the signed-in user.

## What the visitor sees

The client region goes through three states. The browser runtime renders them; you do not write this markup.

**Loading.** Before data arrives, the region contains:

```html
<p class="rk-loading" aria-busy="true">Loading…</p>
```

If the `<DataSource>` is in the server-rendered part of the page (not inside a state-dependent block), the Rust renderer writes this placeholder into the HTML, and the runtime starts the fetch as soon as the module runs.

**Ready.** The runtime renders the `<DataSource>` body with the fetched view bound to `data` (or whatever name you gave with `let:`).

**Failed.** If the request fails or returns a non-2xx status, the region shows:

```html
<p class="rk-error" role="alert">Couldn't load this. <button type="button" data-rk-retry="activity">Retry</button></p>
```

Clicking Retry fetches the source again and shows the loading placeholder until it settles. If it fails again, the error and the Retry button come back.

The runtime is written to keep previously loaded data on screen during a forced re-fetch. In v0.1 that never comes up, because a source that loaded successfully is never fetched again.

You can style these with the `rk-loading` and `rk-error` classes in your page's `<style>`. The demo dims the placeholder:

```css
.rk-loading { opacity: 0.6; }
```

## One fetch per source per page

The runtime keeps a per-page cache keyed by source name. Once `activity` has loaded, every block that shows `activity` uses the cached view. Hiding and showing the region again does not fetch again. The cache lives as long as the page; a full navigation or reload starts empty.

There is no way to refresh a client source from your own code yet. The only re-fetch is the Retry button after a failure. If the underlying data changes, the visitor sees the new value on the next page load. Client refresh is on the [roadmap](/docs/roadmap/).

## Cross-site requests are refused

Before running the resolver, the endpoint looks at the `Sec-Fetch-Site` header that browsers attach to every request. If it is `cross-site`, the endpoint returns `403` with `{"error":"You don't have access to this."}` and the resolver never runs, even when the request carries a valid session cookie.

The runtime always fetches with `credentials: "same-origin"`, so its own requests are `same-origin` and pass. Requests without the header (for example from `curl` or an older browser) are allowed and rely on the session alone. [Security](/docs/security/) explains how this fits with the rest of the defaults.

## Signed-out visitors

If the resolver calls `ctx.require_user()` and there is no user, the endpoint returns:

```http
HTTP/1.1 401 Unauthorized
content-type: application/json
cache-control: no-store

{"error":"Please sign in."}
```

There is no redirect. In the browser, the region shows the error message and the Retry button. If a session can expire while a page is open, put a sign-in link nearby, or load the data on the server so that the page itself redirects to sign-in.

## Rules and limits

- A client `<DataSource>` may appear anywhere, including inside state-dependent blocks.
- Inside a client region, everything is rendered by the browser. A `load="server"` `<DataSource>` cannot be nested in it; the compiler rejects that (see [Server loading](/docs/server-loading/#where-load-server-may-appear)).
- The client-loaded view is not in the handoff payload and is not projected; the endpoint returns all of it.
- Sources take no parameters, so a client source cannot be filtered or paged from the browser.
- Client loading needs JavaScript. Without it, the visitor sees the loading placeholder, so do not put the main content of a page behind `load="client"`.

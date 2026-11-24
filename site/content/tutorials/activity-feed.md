---
title: An activity feed
description: Load a slow source in the browser behind a button, inspect the endpoint Rust-kit generates for it, and watch the loading and Retry states.
order: 4
level: Intermediate
duration: 25 min
---

## What you'll build

An activity page at `/activity`. The feed behind it is slow, so the page doesn't wait for it: the HTML arrives at once, and the feed loads only when someone clicks **Show recent activity**. While it loads, the page shows "Loading…". If the request fails, it shows an error with a **Retry** button.

You'll write no fetch code. Changing `load="server"` to `load="client"` is enough for the compiler to generate an HTTP endpoint, the browser code that calls it, and the loading and error states.

## Prerequisites

- The `hello-rk` project as you left it at the end of [Loading data on the server](/tutorials/server-data/).
- Some comfort with `$state` and `{#if}` from [A counter](/tutorials/counter/).

## When to load on the client

`load="server"` makes the response wait for the source. That's right for data the page is about: a visitor should see their task list in the first response. It's the wrong choice for data that is slow, optional, or below the fold. Loading it on the server would hold up every response for something many visitors never look at.

`load="client"` sends the page first and fetches the source from the browser. Rust-kit runs the same resolver either way, with the same request context and session. Only the timing and the transport change.

## Step 1: A slow, sometimes failing service

Create `src/feed.rs`:

```rust title=src/feed.rs
//! An in-memory stand-in for an activity feed service.
//!
//! It waits a little before answering, so you can see the loading state, and
//! it can be told to fail every other request, so you can see Retry.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustkit::Error;

#[derive(Clone, Debug)]
pub struct Event {
    pub text: String,
    pub minutes_ago: u32,
}

pub struct FeedService {
    latency: Duration,
    flaky: bool,
    /// How many times the feed has been read. Tests and demos can inspect it.
    pub reads: AtomicUsize,
}

impl FeedService {
    pub fn new(latency: Duration, flaky: bool) -> Self {
        Self { latency, flaky, reads: AtomicUsize::new(0) }
    }

    pub async fn recent(&self) -> Result<Vec<Event>, Error> {
        let n = self.reads.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        if self.flaky && n % 2 == 0 {
            return Err(Error::Upstream(format!("feed read {n} timed out (simulated)")));
        }
        let event = |text: &str, minutes_ago| Event { text: text.to_owned(), minutes_ago };
        Ok(vec![
            event("Ada finished “Write a page”", 3),
            event("Grace commented on “Load data on the server”", 18),
            event("Linus opened a new task", 52),
        ])
    }
}
```

`tokio::time::sleep` stands in for a slow network call without blocking the server's threads. When `flaky` is on, reads 0, 2, 4 and so on fail with `Error::Upstream`, which is the error a resolver should return when a service it depends on lets it down.

## Step 2: Add the source

Replace `src/sources.rs` with the version below. The `tasks` source is unchanged. `activity` is new:

```rust title=src/sources.rs
//! Data sources: a name, a view type, and a resolver.
//!
//! The generated render code calls these functions by name. Views serialize
//! with camelCase names because that is what templates read.

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListView {
    pub items: Vec<TaskItem>,
    pub summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub position: usize,
    pub title: String,
    pub done: bool,
}

pub async fn tasks(ctx: &Ctx) -> Result<TaskListView, Error> {
    let tasks = ctx.tasks.list().await?;
    let done = tasks.iter().filter(|t| t.done).count();
    Ok(TaskListView {
        summary: format!("{done} of {} done", tasks.len()),
        items: tasks
            .into_iter()
            .enumerate()
            .map(|(i, t)| TaskItem { position: i + 1, title: t.title, done: t.done })
            .collect(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub items: Vec<ActivityItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub text: String,
    pub when: String,
}

pub async fn activity(ctx: &Ctx) -> Result<ActivityView, Error> {
    let events = ctx.feed.recent().await?;
    Ok(ActivityView {
        items: events
            .into_iter()
            .map(|e| ActivityItem { text: e.text, when: format!("{} min ago", e.minutes_ago) })
            .collect(),
    })
}
```

The resolver formats `minutes_ago` into text (`"3 min ago"`), for the same reason as before: the template can't. This time the view will be sent to the browser as JSON, so keeping it small and display-ready matters even more.

## Step 3: Wire it into AppState

Replace `src/lib.rs`:

```rust title=src/lib.rs
//! hello-rk: a Rust-kit app.

pub mod feed;
pub mod sources;
pub mod tasks;

use std::time::Duration;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

/// Shared application state. Resolvers reach it through `ctx`.
pub struct AppState {
    pub tasks: tasks::TaskService,
    pub feed: feed::FeedService,
}

pub fn app() -> rustkit::App<AppState> {
    // Set FLAKY_FEED=1 to make every other feed read fail.
    let flaky = std::env::var_os("FLAKY_FEED").is_some();
    let state = AppState {
        tasks: tasks::TaskService::demo(),
        feed: feed::FeedService::new(Duration::from_millis(800), flaky),
    };
    rustkit::App::new(generated::manifest(), state)
        // Plain http on localhost: don't mark the session cookie `Secure`.
        .secure_cookies(false)
}
```

An 800 ms delay is long enough to see "Loading…" with your own eyes.

## Step 4: Write the page

Create `src/routes/activity.rk`:

```rk title=activity.rk
<script>
  import { activity } from "$sources";
  let open = $state(false);
</script>

<title>Activity</title>

<main>
  <h1>Team activity</h1>
  <p>The feed is slow, so this page doesn't wait for it. It loads when you ask.</p>

  <button onclick={() => open = !open}>
    {#if open}Hide{:else}Show{/if} recent activity
  </button>

  {#if open}
    <DataSource source={activity} load="client" let:data>
      <ul>
        {#each data.items as item}
          <li>{item.text} <small>{item.when}</small></li>
        {/each}
      </ul>
    </DataSource>
  {/if}
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button { font: inherit; padding: 0.4rem 0.9rem; }
  small { color: #777; margin-left: 0.5rem; }
  .rk-loading { opacity: 0.6; }
  .rk-error { color: #b00020; }
</style>
```

The only difference from a server source is `load="client"`. Putting the `<DataSource>` inside `{#if open}` means nothing is fetched until the first click. If you placed it at the top level instead, the server would render a "Loading…" placeholder and the browser would fetch the feed as soon as the module started.

The `.rk-loading` and `.rk-error` classes belong to elements the runtime inserts. Style them however you like.

Expand **Generated JavaScript** under the sample. The block for the `<DataSource>` has `source: "activity"` and `bind: "data"` instead of a list of state `deps`. That's how the runtime knows to fetch the source and re-render the block when the data arrives.

## Step 5: Run it

```bash
cargo run
```

Your build should now finish without the `unused variable: ctx` warning from [Your first page](/tutorials/your-first-page/): the generated source API finally has a client source to serve.

Open [http://127.0.0.1:3000/activity](http://127.0.0.1:3000/activity) with your browser's developer tools on the **Network** tab. The page arrives at once with **Show recent activity**. Then:

1. Click the button. Its label changes to **Hide recent activity**, "Loading…" appears, and one request to `/_rk/source/activity` shows up in the Network tab.
2. About 800 ms later the list replaces "Loading…".
3. Click **Hide**, then **Show** again. The list appears immediately and no new request is made. Each page keeps a source's data once it has loaded it. There's no way to refresh a client source yet; that's on the [roadmap](/docs/roadmap/).

## Step 6: Look at the endpoint

The browser fetched JSON from a URL you never wrote. Call it yourself:

```bash
curl -i http://127.0.0.1:3000/_rk/source/activity
```

```text
HTTP/1.1 200 OK
content-type: application/json
cache-control: private, no-store
vary: Cookie

{"items":[{"text":"Ada finished “Write a page”","when":"3 min ago"},…]}
```

The body is your `ActivityView`, serialized with its camelCase names. Notice three things:

- **The response is private.** Source endpoints always send `Cache-Control: private, no-store` and `Vary: Cookie`, because a resolver may read the session and return data for one person.
- **Only client sources get an endpoint.** `curl -i http://127.0.0.1:3000/_rk/source/tasks` returns `404`. The `tasks` source is only ever loaded on the server, so it isn't reachable over HTTP at all.
- **Cross-site requests are refused.** A request that a browser labels `Sec-Fetch-Site: cross-site` gets `403`, even with a valid cookie. Try `curl -i -H 'sec-fetch-site: cross-site' http://127.0.0.1:3000/_rk/source/activity`.

## Step 7: Make it fail

Stop the server and start it again with the flaky feed switched on:

```bash
FLAKY_FEED=1 cargo run
```

Reload the page and click **Show recent activity**. After the delay, instead of the list, you'll see:

> Couldn't load this. **Retry**

Click **Retry**. "Loading…" comes back, a second request appears in the Network tab, and this time the list arrives. The service fails every other read, so a fresh page load fails again on the first try.

Check what the failing response looked like:

```bash
curl -i http://127.0.0.1:3000/_rk/source/activity
```

```text
HTTP/1.1 502 Bad Gateway
content-type: application/json
cache-control: no-store

{"error":"A service this page depends on is unavailable. Please try again."}
```

Run it a second time and you'll get the `200` again. The detailed message, `feed read 0 timed out (simulated)`, never reaches the browser. Rust-kit prints it to the server's standard error instead (look for a line starting `rustkit: upstream error:`) and sends a generic message, because upstream error text can contain internal details.

## What just happened

The compiler split the work for `load="client"` across three places:

1. **The server render** emitted no data for the feed at all, just the empty `{#if}` block. Had the `<DataSource>` been at the top level, it would have written a `Loading…` placeholder in its place.
2. **The generated Rust** added `activity` to `CLIENT_SOURCES` and to a `source_api` function. `into_router()` mounts `GET /_rk/source/{name}`, which builds a request context from the cookie and calls the same `crate::sources::activity` your server pages would call.
3. **The browser module** gave the block a `source` instead of state dependencies. When the block first renders, the runtime starts a `fetch` (same origin, with cookies), shows the loading state, and re-renders the block when the response arrives: with the data on success, or with the error and **Retry** on failure. Retrying keeps any rows it already had, so a failed refresh never blanks a list you could already see.

You wrote one attribute. Rust-kit wrote the endpoint, the fetch, the loading state and the error handling, and kept server-only sources off the network.

> [!TIP]
> Choose per `<DataSource>`, not per page. A page can load its main content with `load="server"` and its sidebar with `load="client"`. Keep in mind that the endpoint is per source: if any page loads a source with `load="client"`, that source is reachable at `/_rk/source/{name}` by anyone who can send the request, so its resolver must check access itself.

## Next steps

- [Editing a profile](/tutorials/profile-form/) mixes server data with browser state and posts a form back.
- [Client loading](/docs/client-loading/) is the reference for `load="client"`, the endpoint, and the runtime's states.
- [Errors](/docs/errors/) lists every `rustkit::Error` variant and the response it produces.
- [Security](/docs/security/) explains why source endpoints refuse cross-site requests.

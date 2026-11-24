---
title: Measuring performance
description: Read the Server-Timing header, simulate upstream latency, see two server sources run concurrently, and report JavaScript bytes per route from app.routes().
order: 9
level: Advanced
duration: 35 min
---

## What you'll build

You'll extend the `members` app from [Signing in](/tutorials/sign-in/) with the tools to answer two questions about any page:

1. **Where does request time go?** Waiting for data, or producing HTML?
2. **What does the browser have to download?** How much JavaScript does each route load?

You'll add a latency knob to the fake accounts service, add a second data source to the notes page, and watch Rust-kit's `Server-Timing` header show the two reads overlapping. Then you'll add a `report` command that prints JavaScript and CSS bytes for every route, straight from the compiled manifest.

This tutorial doesn't hand you numbers to expect. Timing depends on your machine, so you'll measure your own. Where a value comes from the code rather than the clock, such as the latency you configure, the text says what to look for.

## Prerequisites

- The `members` project from [Signing in](/tutorials/sign-in/). [Testing your app](/tutorials/testing-your-app/) is optional; if you did it, its tests keep passing.
- `curl`, and a browser with developer tools.

## Step 1: Read Server-Timing

Every page rendered per request carries a `Server-Timing` header. Start the app and sign in:

```bash
cargo run
```

In a second terminal:

```bash
curl -s -c jar.txt http://127.0.0.1:3000/login > login.html
TOKEN=$(grep -o 'name="_csrf" value="[0-9a-f]*"' login.html | cut -d'"' -f4)
curl -s -b jar.txt -c jar.txt -d "user=alice&_csrf=$TOKEN" -o /dev/null http://127.0.0.1:3000/login
curl -s -b jar.txt -D - -o /dev/null http://127.0.0.1:3000/notes | grep -i server-timing
```

You'll see a line of this shape:

```text
server-timing: sources;dur=…, render;dur=…
```

Both durations are in milliseconds:

- **`sources`** is the wall time from the start of `render` until every `load="server"` source has resolved. It's mostly time spent waiting on other services.
- **`render`** is the time spent writing HTML once the data is in hand. It's CPU time in your process.

The split matters because the fixes are different. Slow `sources` means you need faster upstreams, fewer calls, or `load="client"` for data that can wait. Slow `render` means a very large page or a debug build.

Browsers understand this header too. In Chrome or Edge, open the Network tab, select the `/notes` request, and look at the **Timing** tab. `sources` and `render` appear under "Server Timing". Firefox shows them in the request's **Timings** panel.

`main.rs` currently hard-codes a 20 ms delay, so `sources` should be a little above 20. Prerendered pages such as `/` don't have the header, since they're rendered once at startup and not per request.

## Step 2: Make latency configurable

Replace `src/main.rs`. It now reads the delay from `LATENCY_MS` and has a `report` command, which you'll use in Step 5:

```rust title=src/main.rs
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("report") => report(),
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            serve(addr).await;
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; use `serve [addr]` or `report`");
            std::process::exit(2);
        }
    }
}

/// Simulated upstream latency in milliseconds, from `LATENCY_MS` (default 20).
fn latency() -> Duration {
    let ms = std::env::var("LATENCY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    Duration::from_millis(ms)
}

async fn serve(addr: &str) {
    // Local development over plain http, so no `Secure` cookies.
    let app = members::app(latency(), false);
    let router = app.into_router().await.expect("failed to build the router");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("could not bind");
    println!("listening on http://{addr} with {:?} upstream latency", latency());
    axum::serve(listener, router).await.unwrap();
}

/// Prints how much JavaScript and CSS each route loads.
fn report() {
    let app = members::app(Duration::ZERO, false);
    println!("{:<10} {:<8} {:>9} {:>9}", "route", "mode", "JS bytes", "CSS bytes");
    for r in app.routes() {
        let mode = if r.prerender { "static" } else { "request" };
        println!("{:<10} {:<8} {:>9} {:>9}", r.path, mode, r.js_bytes, r.css_bytes);
    }
}
```

Restart with a deliberately slow upstream:

```bash
LATENCY_MS=50 cargo run
```

Sign in again (restarting cleared the in-memory sessions) and request `/notes` a few times. `sources` should now sit a little above 50: the configured delay plus a small amount of scheduling overhead. `render` doesn't change, because the HTML is the same size. Now you can tell the two apart at a glance.

## Step 3: Add a second source

Real pages often need data from more than one place. Add a second call to the service. In `src/accounts.rs`, add a counter field and a `notices` method. Here's the whole file:

```rust title=src/accounts.rs
//! An in-memory stand-in for an accounts service.
//!
//! It adds a fixed delay to every call to imitate a network hop, and counts
//! reads so tests can check how often a page calls it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustkit::Error;

#[derive(Clone, Debug)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub notes: Vec<String>,
}

pub struct AccountsService {
    accounts: HashMap<String, Account>,
    latency: Duration,
    pub reads: AtomicUsize,
    pub notice_reads: AtomicUsize,
}

impl AccountsService {
    pub fn demo(latency: Duration) -> Self {
        let account = |id: &str, name: &str, notes: &[&str]| {
            let notes = notes.iter().map(|n| n.to_string()).collect();
            (id.to_owned(), Account { id: id.into(), name: name.into(), notes })
        };
        Self {
            accounts: HashMap::from([
                account("alice", "Alice", &["Buy more tea", "Finish the rabbit-hole essay"]),
                account("bob", "Bob", &["Tune the guitar"]),
            ]),
            latency,
            reads: AtomicUsize::new(0),
            notice_reads: AtomicUsize::new(0),
        }
    }

    /// True if `id` names an account. A real service would check a credential.
    pub fn exists(&self, id: &str) -> bool {
        self.accounts.contains_key(id)
    }

    pub async fn get(&self, id: &str) -> Result<Account, Error> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        self.accounts.get(id).cloned().ok_or(Error::NotFound)
    }

    /// A second, independent call with the same latency.
    pub async fn notices(&self, id: &str) -> Result<Vec<String>, Error> {
        self.notice_reads.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.latency).await;
        let account = self.accounts.get(id).ok_or(Error::NotFound)?;
        Ok(vec![
            format!("Welcome back, {}.", account.name),
            "Maintenance is planned for Sunday.".to_owned(),
        ])
    }
}
```

`notices` counts into its own `notice_reads`, so the "one read per page" test from [Testing your app](/tutorials/testing-your-app/) still measures exactly what it did before.

Add the source to `src/sources.rs`:

```rust title=src/sources.rs
//! Data sources. Each resolver decides for itself whether it needs a user.

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeView {
    pub name: String,
    pub notes: Vec<NoteView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub text: String,
}

/// The signed-in user's own account. Anonymous requests get `Unauthorized`,
/// which sends a page visitor to the sign-in page.
pub async fn me(ctx: &Ctx) -> Result<MeView, Error> {
    let user = ctx.require_user()?;
    let account = ctx.accounts.get(&user.id).await?;
    Ok(MeView {
        name: account.name,
        notes: account.notes.into_iter().map(|text| NoteView { text }).collect(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticesView {
    pub count: usize,
    pub items: Vec<NoteView>,
}

pub async fn notices(ctx: &Ctx) -> Result<NoticesView, Error> {
    let user = ctx.require_user()?;
    let notices = ctx.accounts.notices(&user.id).await?;
    Ok(NoticesView {
        count: notices.len(),
        items: notices.into_iter().map(|text| NoteView { text }).collect(),
    })
}
```

Then use both sources on `src/routes/notes.rk`. The notices list sits behind a toggle, so this page now has some JavaScript to measure as well:

```rk title=notes.rk
<script>
  import { me, notices } from "$sources";
  let showNotices = $state(false);
</script>

<title>Your notes</title>

<main>
  <DataSource source={me} load="server" let:data>
    <h1>{data.name}'s notes</h1>
    {#if data.notes.length === 0}
      <p>No notes yet.</p>
    {:else}
      <ul>
        {#each data.notes as note}
          <li>{note.text}</li>
        {/each}
      </ul>
    {/if}
  </DataSource>

  <DataSource source={notices} load="server" let:n>
    <button onclick={() => showNotices = !showNotices}>Notices ({n.count})</button>
    {#if showNotices}
      <ul class="notices">
        {#each n.items as item}
          <li>{item.text}</li>
        {/each}
      </ul>
    {/if}
  </DataSource>

  <form method="post" action="/logout">
    <button>Sign out</button>
  </form>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button { font: inherit; }
  .notices { color: #555; }
</style>
```

Expand **Generated Rust** under the sample and find the first line of `render`:

```rust
let (src_me, src_notices,) = ::rustkit::__private::try_join!(crate::sources::me(ctx), crate::sources::notices(ctx))?;
```

Both resolvers start together, and rendering waits for both.

## Step 4: See the reads overlap

Restart with `LATENCY_MS=50 cargo run`, sign in, and request `/notes` several times:

```bash
for i in 1 2 3; do
  curl -s -b jar.txt -D - -o /dev/null http://127.0.0.1:3000/notes | grep -i server-timing
done
```

Each source sleeps for 50 ms, and the page needs both. Yet `sources` should still be a little above **50**, not 100. The two calls overlap because the generated code joins them. You get this from writing two `<DataSource>` elements; you didn't have to think about concurrency.

Compare that with doing both calls inside one resolver, one after the other:

```rust
let account = ctx.accounts.get(&user.id).await?;
let notices = ctx.accounts.notices(&user.id).await?; // starts only after `get` finishes
```

Those awaits run in sequence, so `sources` would be about the sum of the two delays. When data comes from independent calls, give each its own source, or join the calls yourself inside one resolver with `tokio::try_join!`.

Now try `LATENCY_MS=0`. `sources` drops close to zero and `render` is left on its own. That's the cost of producing this page's HTML. Run once with `cargo run --release` to see what it costs in an optimised build, which is the number that matters in production.

> [!TIP]
> To measure the whole response including network and connection setup, ask `curl` for its own timing: `curl -s -b jar.txt -o /dev/null -w '%{time_total}\n' http://127.0.0.1:3000/notes`. The difference between that and `sources + render` is time spent outside your page, mostly on the network and in the HTTP stack.

## Step 5: Report JavaScript bytes per route

The compiler records, for every route, how many bytes of JavaScript and CSS it loads. `App::routes()` exposes them. Run the `report` command you added in Step 2:

```bash
cargo run -- report
```

You should see one line per route, like this:

```text
route      mode      JS bytes CSS bytes
/          static           0       120
/login     request          0       156
/notes     request       7350       176
```

How to read it:

- **`/` and `/login` ship no JavaScript.** Neither has state, handlers or client sources. A form is plain HTML and works without a script.
- **`/notes` now loads JavaScript** because of the `showNotices` toggle. `js_bytes` is the page's own module plus the shared runtime, uncompressed. The runtime is most of it, and it's the same file on every interactive page, so a browser downloads it once and caches it for a year (assets are content-hashed and served as `immutable`).
- **CSS bytes** are the page's `<style>` block, also uncompressed.

To see what crosses the network after compression, fetch the files and compress them yourself:

```bash
JS=$(curl -s -b jar.txt http://127.0.0.1:3000/notes | grep -o '/_rk/assets/notes[^"]*\.js')
curl -s "http://127.0.0.1:3000$JS" | wc -c
curl -s "http://127.0.0.1:3000$JS" | gzip -9 | wc -c
```

Rust-kit doesn't compress responses itself. In production, let your reverse proxy do it; [Deploying](/tutorials/deploying/) shows a configuration that does.

> [!TIP]
> Turn the report into a budget. In your test suite, assert `js_bytes == 0` for every route that should stay script-free, as [Testing your app](/tutorials/testing-your-app/) does for `/`. A refactor that accidentally adds a handler to a static page then fails CI instead of shipping.

## Step 6: Check the payload

The handoff payload is part of the page weight too. Look at it:

```bash
curl -s -b jar.txt http://127.0.0.1:3000/notes | grep rk-payload
```

```html
<script type="application/json" id="rk-payload">{"s":{"notices":{"items":[{"text":"Welcome back, Alice."},{"text":"Maintenance is planned for Sunday."}]}}}</script>
```

Only `notices.items` is there. `count` is used only in the server-rendered button label, and the `me` source isn't read by anything the browser renders, so neither crosses into the page. Before you move a large list into a `$state`-controlled block, remember that every field the block reads will be embedded in the HTML.

## What just happened

- **`Server-Timing` is measured by the generated code.** `render` starts a clock, awaits the joined sources, records `sources`, writes the HTML and records `render`. The router turns those into the header for every per-request page.
- **Concurrency comes from the page structure.** Each `load="server"` source on a page is one future in a single `try_join!`, so independent reads overlap. If one fails, the request fails at once instead of waiting for the others.
- **Byte counts come from the compiler.** `js_bytes` and `css_bytes` are computed when the assets are generated and stored in the manifest, so the report needs no server and no network. You can run it in CI.

## Next steps

- [Deploying](/tutorials/deploying/) builds a release binary and runs it behind a TLS proxy.
- [Performance](/docs/performance/) covers the rest of Rust-kit's performance model.
- [Server loading](/docs/server-loading/) and [Client loading](/docs/client-loading/) help you decide where each source belongs.
- [The handoff payload](/docs/handoff-payload/) explains the projection rules in detail.

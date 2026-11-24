---
title: Performance
description: What Rust-kit does for performance by design, how to measure your own pages, and what has not been measured yet.
section: Guides
order: 2
---

Rust-kit makes a few structural choices that should keep pages fast: HTML that reads correctly without JavaScript, no JavaScript at all on pages that do not need it, server data loaded concurrently and once, and a native render function instead of a JavaScript engine on the server. This guide explains those choices, shows how to measure your own pages, and is plain about what has and has not been measured.

## Honest status

The first milestone measured bytes, not speed. The README records two numbers for the account demo: the account page's browser module is 662 B gzipped and the shared runtime is 2.7 KB gzipped. Both are reproduced below.

There are no benchmarks yet of render throughput, latency under load, or comparisons with other frameworks. Compiling templates to native Rust is an architectural choice that should make rendering cheap, but it is not yet a measured result. If you need numbers for a decision, measure your own pages with the tools on this page.

## What the compiler does for you

### No JavaScript unless the page needs it

A page gets a browser module only if it has a `$state` block, an event handler, or leftover script code. Otherwise the compiler emits no module and the page has no `<script>` tag. The demo's article, home and sign-in pages ship HTML and CSS only. The sign-in page posts a form, and forms work without JavaScript.

### Server sources load concurrently, once

All `load="server"` sources on a page start together in one `try_join!`:

```rust partial
let (src_account, src_plan,) = ::rustkit::__private::try_join!(
    crate::sources::account(ctx), crate::sources::plan(ctx))?;
```

The time spent waiting is roughly the slowest source, not the sum. Each source is called once per request however many times the template reads it. The demo's acceptance tests assert exactly one upstream read per account page view. If a source fails, the others are dropped and the page returns the error.

Results are not cached between requests, and two different sources that call the same upstream endpoint will call it twice. Share work in your resolvers or your service client if that matters. See [Server loading](/docs/server-loading/).

### Literal coalescing

The Rust render function writes static markup with `out.push_str`. The compiler merges runs of adjacent literal writes into one call, so a stretch of static HTML is a single copy into the output buffer, which starts with 4 KB of capacity. The browser module's render functions get the same treatment for `h += "…"`. You can see the merged writes in [Generated code](/docs/generated-code/).

### Only the fields the browser reads

When the browser has to re-render a server-loaded region, the [handoff payload](/docs/handoff-payload/) carries only the fields that region reads. The account page sends `{"displayName": "…"}`, not the whole account. Reading a whole object, such as passing `data` to a handler, sends the whole view, so read specific fields where you can.

### Prerendering and caching

Routes marked `export const prerender = true` render once when the router starts and are served from memory with `Cache-Control: public, max-age=300`. Assets have content-hashed names and are served with `public, max-age=31536000, immutable`, so a returning visitor downloads a module again only when it changes. See [Prerendering](/docs/prerendering/) and [Caching](/docs/caching/).

## Measuring server time

Every request-time page response carries a `Server-Timing` header:

```text
server-timing: sources;dur=27.167, render;dur=0.032
```

| Metric | Measures |
|---|---|
| `sources` | Wall time from the start of `render` until every server source has resolved. This is mostly upstream latency. |
| `render` | Time from then until the page is built: writing HTML, serialising the payload, and wrapping the document. This is the CPU cost of the template. |

Both are in milliseconds with three decimals. They do not include session lookup, header building, or network time. Prerendered pages, source endpoints and actions do not send the header.

Browser developer tools show `Server-Timing` in the timing panel of a request. From the command line:

```bash
curl -s -o /dev/null -D - -H "Cookie: rk_session=…" http://127.0.0.1:3000/account | grep -i server-timing
```

The header above is real: three consecutive requests to the demo's account page, debug build, on one development machine, returned `sources` of 30.2, 27.2 and 27.0 ms and `render` of 0.228, 0.032 and 0.033 ms. The demo adds an imitated 25 ms upstream round trip, which accounts for nearly all of the `sources` time. These are single observations, not a benchmark. Measure release builds for anything you report.

### Reading the two numbers

- **`sources` is high.** The upstream is slow, or a resolver awaits calls one after another. Split independent upstream calls into separate sources so they run concurrently, or `join` them inside the resolver. Consider loading slow, non-essential data with `load="client"` so it stops blocking the first response.
- **`render` is high.** Look for large `{#each}` lists or a big payload. A whole-object read in a browser-rendered region serialises the entire view.
- **Both are low but the page is slow.** The time is elsewhere: the network, a proxy, TLS, or the browser.

## JavaScript per route

The demo's `build` command prints bytes per route. This is the current output:

```bash
cargo run -p account-demo -- build dist
```

```text
route        mode       JS bytes  CSS bytes
/account     request        7998        689
/article     static            0        546
/            static            0        512
/login       request           0        512
```

`JS bytes` is the page module plus the shared runtime, uncompressed: for `/account`, a 1,340-byte module and the 6,658-byte runtime. The same numbers are available as `Route::js_bytes` and `Route::css_bytes`, so you can print them in your own app's build command or assert on them in tests, as in [Testing](/docs/testing/#test-prerendered-pages).

To get the gzipped sizes, compress the built files yourself:

```bash
for f in dist/_rk/assets/*.js; do echo "$f $(gzip -9c "$f" | wc -c)"; done
```

For the demo that gives 662 bytes for the account module and 2,719 bytes for the runtime, matching the README. The runtime is shared and cached across every interactive route, so the per-page cost after the first visit is the page module.

> [!NOTE]
> The router does not compress responses. Enable compression at your reverse proxy or CDN, or add a compression middleware to the axum router yourself. See [Deployment](/docs/deployment/).

## What to measure

When you evaluate a page, check these, in roughly this order:

1. **Does the first response contain the content?** Fetch the page with `curl` and look for the text. If the important text is inside a `load="client"` source, it is not in the HTML.
2. **Does the page need JavaScript?** A static page should report 0 JS bytes. Assert it in a test.
3. **How many upstream calls does a view make?** Count them with a stand-in service, as the demo does.
4. **`sources` and `render` from `Server-Timing`**, under release builds and realistic data.
5. **Payload size.** Look at the `rk-payload` script in the HTML. It should hold only the fields browser-rendered regions read.
6. **Client source requests.** Each `load="client"` source is one `GET /_rk/source/{name}` request, made the first time its block appears.

## Known costs

These are limits of the current release, not tuning opportunities:

- **Whole-block updates.** A state change re-renders each block that reads it, replacing its DOM. There are no per-attribute patches or keyed lists, so large lists inside a block are rebuilt in full and focus inside the block is lost.
- **No source refresh.** A client source is fetched once per page load; there is no way to refetch except the Retry button that appears after a failure.
- **No minification.** Modules and the runtime ship as generated.
- **In-memory sessions** are a lock-protected map in the process. Contention has not been measured.

The [roadmap](/docs/roadmap/) tracks what is planned.

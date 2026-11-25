---
title: Loading data on the server
description: Write a data source with a typed view and a resolver, keep a small in-memory service in AppState, and render a list with {#each}.
order: 3
level: Beginner
duration: 25 min
---

## What you'll build

A task list at `/tasks`. The tasks come from a small in-memory "task service" that lives in your application state. A *data source* reads from it and shapes the result into a view, and the page renders that view on the server with `{#if}` and `{#each}`.

The finished page arrives as complete HTML, ships no JavaScript, and never exposes fields the template doesn't use.

## Prerequisites

- The `hello-rk` project from [Your first page](/tutorials/your-first-page/). The [counter](/tutorials/counter/) page can stay; this tutorial doesn't touch it.

## How data gets onto a page

Rust-kit pages don't call services directly. Every piece of data has three parts:

1. **A service** that knows how to fetch it. Here it's an in-memory struct. In a real app it would be a database pool or an HTTP client. It lives in `AppState`.
2. **A source**: an `async fn` in `crate::sources` (the *resolver*) that returns a serializable *view*. The function's name is the source's name.
3. **A `<DataSource>`** element in the page that imports the source by name and says where to load it.

This split keeps decisions in Rust. The resolver chooses which fields leave the server, formats anything that needs formatting, and decides whether the caller may see the data at all.

## Step 1: Add an in-memory service

Create `src/tasks.rs`:

```rust title=src/tasks.rs
//! An in-memory stand-in for a task service.
//!
//! A real app would call a database or an HTTP API here. The methods are
//! `async` and return `Result` so that swapping one in changes this file only.

use rustkit::Error;

#[derive(Clone, Debug)]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub done: bool,
    /// Stored by the service, never shown on a page.
    pub internal_notes: String,
}

pub struct TaskService {
    tasks: Vec<Task>,
}

impl TaskService {
    pub fn demo() -> Self {
        let task = |id, title: &str, done| Task {
            id,
            title: title.to_owned(),
            done,
            internal_notes: format!("created by the seed data, id {id}"),
        };
        Self {
            tasks: vec![
                task(1, "Install Rust", true),
                task(2, "Write a page", true),
                task(3, "Load data on the server", false),
                task(4, "Check that <b>tags</b> are escaped", false),
            ],
        }
    }

    pub async fn list(&self) -> Result<Vec<Task>, Error> {
        Ok(self.tasks.clone())
    }
}
```

`list` is `async` and returns `rustkit::Error` on failure even though it can't fail. That's on purpose: it has the same shape as a real network call, so replacing it later won't ripple through the rest of the app. The `internal_notes` field stands in for data your backend has but your pages shouldn't show.

## Step 2: Write the source

Replace `pub mod sources {}` with a real module. Create `src/sources.rs`:

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
```

A few things to notice:

- **The signature is fixed.** A resolver takes `&RequestContext<AppState>` and returns `Result<View, rustkit::Error>`. The context dereferences to your `AppState`, so `ctx.tasks` reaches the service.
- **The view is not the record.** `TaskItem` has no `id` and no `internal_notes`. Whatever the view doesn't contain can't reach a page.
- **Derived values are computed here.** Templates can't do arithmetic or call functions, so the resolver works out `summary` and the 1-based `position`.
- **`#[serde(rename_all = "camelCase")]` is required** on every view struct. Templates read fields in camelCase (`data.displayName`), the generated Rust reads the matching snake_case field (`display_name`), and data sent to the browser uses the JSON names. The attribute keeps all three in agreement.

## Step 3: Put the service in AppState

Replace `src/lib.rs`:

```rust title=src/lib.rs
//! hello-rk: a Rust-kit app.

pub mod sources;
pub mod tasks;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

/// Shared application state. Resolvers reach it through `ctx`.
pub struct AppState {
    pub tasks: tasks::TaskService,
}

pub fn app() -> rustkit::App<AppState> {
    let state = AppState { tasks: tasks::TaskService::demo() };
    rustkit::App::new(generated::manifest(), state)
        // Plain http on localhost: don't mark the session cookie `Secure`.
        .secure_cookies(false)
}
```

`App::new` wraps the state in an `Arc`, so every request shares one `TaskService`. If your service holds a connection pool, this is where you'd create it.

## Step 4: Render the list

Create `src/routes/tasks.rk`:

```rk title=tasks.rk
<script>
  import { tasks } from "$sources";
</script>

<title>Tasks</title>

<main>
  <DataSource source={tasks} load="server" let:data>
    <h1>Tasks</h1>
    <p class="summary">{data.summary}</p>

    {#if data.items.length === 0}
      <p>Nothing to do. Enjoy your day.</p>
    {:else}
      <ol>
        {#each data.items as item}
          <li data-done={item.done}>
            {#if item.done}
              <s>{item.title}</s>
            {:else}
              {item.title}
            {/if}
            <small>#{item.position}</small>
          </li>
        {/each}
      </ol>
    {/if}
  </DataSource>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  .summary { color: #555; }
  li[data-done] { color: #777; }
  small { color: #999; margin-left: 0.5rem; }
</style>
```

Line by line:

- `import { tasks } from "$sources";` names the source. `$sources` isn't a real JavaScript module. It tells the compiler to look for `crate::sources::tasks`. The import must sit on one line.
- `<DataSource source={tasks} load="server" let:data>` loads the source while rendering on the server, and binds the view to `data` inside the element. `let:data` is the default name; `let:taskList` would bind `taskList` instead.
- `data.items.length` works on lists and strings. For strings it counts UTF-16 code units, the same as JavaScript.
- `{#each data.items as item}` repeats its body for each element. You can also write `{#each data.items as item, i}` to get a 0-based index, but since `{i + 1}` isn't allowed, the resolver supplies `position` instead.
- `data-done={item.done}` becomes a bare `data-done` when the task is done and disappears otherwise, the same rule you saw with `disabled` in the counter.

Expand **Generated Rust** under the sample. You'll find a line that starts the source read before any HTML is written:

```rust
let (src_tasks,) = ::rustkit::__private::try_join!(crate::sources::tasks(ctx))?;
```

Then a `for` loop over `&v_data.items` renders the list. There's no **Generated JavaScript**: nothing on this page reads state, so it ships HTML and CSS only.

## Step 5: Run it

```bash
cargo run
```

Open [http://127.0.0.1:3000/tasks](http://127.0.0.1:3000/tasks). You should see "2 of 4 done", two struck-through tasks, two open ones, and a position number after each.

Look at the fourth task. The service's title contains `<b>tags</b>`, but the page shows the angle brackets as text rather than making the word bold. Check the raw HTML:

```bash
curl -s http://127.0.0.1:3000/tasks | grep escaped
```

```html
              Check that &lt;b&gt;tags&lt;/b&gt; are escaped
```

Every interpolated value is escaped, in text and in attributes. There's no "raw HTML" escape hatch to forget about.

Now search the page for the hidden field:

```bash
curl -s http://127.0.0.1:3000/tasks | grep -c "seed data"
```

The count is `0`. `internal_notes` stayed on the server because the view never had it.

## Step 6: Break it on purpose

Two mistakes are worth making once, so you recognise them later.

**Calling a function in the template.** Suppose you want the titles in upper case:

```rk fail title=tasks.rk
<script>
  import { tasks } from "$sources";
</script>

<DataSource source={tasks} load="server" let:data>
  {#each data.items as item}
    <li>{item.title.toUpperCase()}</li>
  {/each}
</DataSource>
```

The compiler refuses, and its help text tells you the fix: compute the value in the resolver and expose it as a field. Rust can't run `toUpperCase()` the way a browser would, and Rust-kit won't guess.

**Forgetting the resolver.** Rename `tasks` in `sources.rs` to `task_list` and run `cargo build`. The page still compiles (it only knows the name), but rustc doesn't:

```text
error[E0425]: cannot find function `tasks` in module `crate::sources`
```

The name in `import { tasks }` must match a public `async fn` in `src/sources.rs`. Rename it back before you continue.

## What just happened

When a request for `/tasks` arrives, the generated `render` function:

1. **Starts every server source at once.** This page has one, but with two or more, they're joined with `try_join!` so they run concurrently, and each source is read exactly once per request no matter how many times the template uses it. If any resolver returns an error, rendering stops and that error becomes the response.
2. **Writes the HTML.** Field reads like `data.summary` compile to plain Rust field accesses (`src_tasks.summary`), converted through `rustkit::js::ToJs` so that numbers, booleans, `Option` and strings print exactly as JavaScript would print them.
3. **Records timings.** Run `curl -si http://127.0.0.1:3000/tasks | grep server-timing`. The `sources` figure is the time spent in your resolver; `render` is the time spent writing HTML. With an in-memory service both are tiny. [Measuring performance](/tutorials/measuring-performance/) makes `sources` interesting.

A server-loaded source never gets an HTTP endpoint. Try `curl -i http://127.0.0.1:3000/_rk/source/tasks`: it's a 404. The only way to reach this data is through the rendered page, which is exactly what you want for data that a browser has no reason to fetch on its own.

> [!NOTE]
> A route can take one `[param]`, and a resolver takes it as `&str` when every importer sits on that segment. This tutorial's source takes only the request context. Query strings and page arguments are on the [roadmap](/docs/roadmap/). See [Pages and routes](/docs/pages-and-routes/#one-dynamic-segment).

## Next steps

- [An activity feed](/tutorials/activity-feed/) loads a slow source in the browser instead, behind a button.
- [Data sources](/docs/data-sources/) is the full reference for sources, views and resolvers.
- [Server loading](/docs/server-loading/) covers how server sources are joined and timed.
- [Template syntax](/docs/template-syntax/) lists every block, including `{#each}` with an index.

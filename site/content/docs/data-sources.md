---
title: Data sources
description: Name a piece of data, give it a typed view, and write one async resolver that serves both server rendering and the browser.
section: Data
order: 1
---

A data source is how a page gets data. It has three parts:

- a **name**, such as `account`, which pages import from `"$sources"`;
- a **view type**, a Rust struct that holds exactly the fields pages may see;
- a **resolver**, an async Rust function that produces the view for the current request.

Pages never call services, read the session, or touch the database directly. They name a source in a `<DataSource>` element and read fields of its view. The resolver is the one place where your code decides what data a request may have.

## Writing a source

Sources live in the `crate::sources` module of your app. The compiler hard-codes that path (see [Project structure](/docs/project-structure/)), so a source named `account` must be the function `crate::sources::account`.

Here is the `account` source from the reference app, `examples/account-demo/src/sources.rs`:

```rust
use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub display_name: String,
}

pub async fn account(ctx: &Ctx) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(&user.id).await?;
    Ok(AccountView { display_name: record.display_name })
}
```

And a page that uses it:

```rk title=greeting.rk
<script>
  import { account } from "$sources";
</script>

<title>Your account</title>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
</DataSource>
```

`let:data` names the binding. If you leave it out, the binding is called `data` anyway.

## The resolver signature

Every resolver has the same shape:

```rust
pub async fn name(ctx: &RequestContext<AppState>) -> Result<View, Error>
```

| Part | Requirement |
| --- | --- |
| Name | Must match the name the page imports. Renaming on import (`import { account as a }`) is rejected by the compiler. |
| Argument | `&RequestContext<AppState>`, and `slug: &str` when every page that imports the source sits on the same `[slug]`. No query string and no argument from the page. See [Pages and routes](/docs/pages-and-routes/#one-dynamic-segment). |
| Return | `Result<View, rustkit::Error>`. The error decides the response; see [Errors](/docs/errors/). |
| View | A struct deriving `serde::Serialize`, with `#[serde(rename_all = "camelCase")]`. |

The generated code calls your resolver directly. For the page above the compiler writes something close to:

```rust
let (src_account,) = ::rustkit::__private::try_join!(crate::sources::account(ctx))?;
```

So if the signature is wrong, `rustc` reports the error in the generated file, pointing at the call.

## Reaching your app state

`RequestContext<S>` implements `Deref<Target = S>`. Whatever you put in your `AppState` is available as a field on `ctx`:

```rust
pub struct AppState {
    pub accounts_service: upstream::AccountsService,
}

// In a resolver:
let record = ctx.accounts_service.get(&user.id).await?;
```

This is where clients, connection pools and configuration belong. The state is created once in `App::new(manifest, state)` and shared behind an `Arc`, so it must be `Send + Sync`. Cloning a `RequestContext` is cheap; it is an `Arc` too.

## Checking who is asking

Most personal data should only be served to a signed-in user. Call `ctx.require_user()` first:

```rust
let user = ctx.require_user()?; // Result<User, Error>
```

`require_user` returns `Ok(User { id })` when the session has a user, and `Err(Error::Unauthorized)` when it does not. Using `?` passes that error out, and the framework turns it into the right response for where the source was used:

| Where the source ran | Response to `Error::Unauthorized` |
| --- | --- |
| Server render of a page | `303 See Other` to `/login?next=/the-page` |
| `GET /_rk/source/{name}` | `401` with `{"error":"Please sign in."}` |

If you only want to know whether someone is signed in, `ctx.user()` returns `Option<User>` without failing. Both calls mark the response as personalised, which changes its caching headers. [Sessions](/docs/sessions/) and [Caching](/docs/caching/) explain why.

> [!WARNING]
> Do not pass user identity into a source any other way. The session is the only input a resolver gets, which is what makes it safe to expose the same resolver over HTTP.

## Why views use camelCase

Templates read JavaScript-style names: `data.displayName`. The same template is compiled twice, and each target reads that name differently:

- The **Rust renderer** converts each field name to snake_case and reads the struct field: `data.displayName` becomes `v_data.display_name`.
- The **browser** reads the JSON it was given: `data.displayName`. For a client-loaded source, and for any nested struct or whole view in the [handoff payload](/docs/handoff-payload/), that JSON comes from serde.

Without `#[serde(rename_all = "camelCase")]`, serde would write `{"display_name": "…"}`. The server render would be correct, and then the browser would find `data.displayName` undefined and render an empty string. Nothing fails at build time; the page just goes blank in places.

So the rule is simple: write snake_case Rust fields, read camelCase names in templates, and put `#[serde(rename_all = "camelCase")]` on every view struct, including nested ones:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub items: Vec<ActivityItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub description: String,
}
```

## What a view may contain

A template can only do a few things with a field, and each needs the Rust type to support it:

| Template use | Rust types that work |
| --- | --- |
| `{data.name}` or `value={data.name}` | `String`, `&str`, `bool`, all integer and float types, `Option<T>` of these (`None` renders as nothing), `Box<T>` |
| `{#each data.items as item}` | `Vec<T>` (anything whose reference iterates) |
| `{data.items.length}`, `{data.name.length}` | `Vec<T>`, slices, `String` (string length counts UTF-16 units, as in JavaScript) |
| `{data.owner.name}` | a nested struct field |

Anything else, such as a date type, fails in `rustc` with a missing `ToJs` implementation. Format it in the resolver and expose a `String`. Templates cannot call functions; [Expressions](/docs/expressions/) explains why.

## Keep views small and explicit

The view is your boundary. Upstream records often have fields the browser must never see. In the demo, the upstream `AccountRecord` has an `email`; `AccountView` does not, so no template, payload or endpoint can leak it. The acceptance tests check that `alice@example.com` never appears in the page.

Build the view field by field from what you fetched. Avoid putting upstream structs straight into a view, because a field added upstream later would then be added to your page data without anyone noticing.

## One resolver, two paths

A page chooses how to load each source:

- `load="server"` calls the resolver while rendering the HTML. See [Server loading](/docs/server-loading/).
- `load="client"` renders a loading placeholder, and the browser fetches the view from `GET /_rk/source/{name}`, which calls the same resolver with the same session. See [Client loading](/docs/client-loading/).

You write the resolver once. The authorization check, the upstream call and the shape of the view are identical on both paths. This is why the acceptance test can show that Alice and Bob each see only their own data through both the HTML and the API.

Only sources that some page loads with `load="client"` get an HTTP endpoint. A source used only with `load="server"` is not reachable over HTTP at all; `GET /_rk/source/account` returns `404` in the demo.

## Limitations in v0.1

- The only extra argument is the route's one parameter, and only when every importer sits on that `[param]`. A page cannot pass its own argument, and a resolver cannot read the query string.
- There is no client-side refresh. Once the browser has a client-loaded view, it keeps it for the life of the page.
- There is no built-in caching of resolver results across requests. Each page request calls each server source once.

See the [roadmap](/docs/roadmap/) for what is planned.

---
title: Editing a profile
description: Toggle an editor with $state that reads server data from the handoff payload, then save it with a POST action that validates input, checks CSRF, and redirects.
order: 5
level: Intermediate
duration: 30 min
---

## What you'll build

A profile page at `/profile` that shows a display name and a short bio. An **Edit profile** button opens a form, already filled in with the current values, without another request to the server. Saving posts the form to a Rust *action*, which validates the input, updates the store, and redirects back to the page.

Along the way you'll see the *handoff payload*, the small JSON document the server embeds so the browser can render server data, and you'll see the CSRF protection Rust-kit adds to every form without being asked.

## Prerequisites

- The `hello-rk` project as you left it at the end of [An activity feed](/tutorials/activity-feed/).
- You should be comfortable with sources and resolvers from [Loading data on the server](/tutorials/server-data/).

## Step 1: A store you can write to

So far every service has been read-only. Create `src/profile.rs` with one profile behind a `Mutex`:

```rust title=src/profile.rs
//! An in-memory profile store. One profile, shared by every visitor.

use std::sync::Mutex;

use rustkit::Error;

#[derive(Clone, Debug)]
pub struct Profile {
    pub display_name: String,
    pub bio: String,
}

pub struct ProfileStore {
    profile: Mutex<Profile>,
}

impl ProfileStore {
    pub fn demo() -> Self {
        Self {
            profile: Mutex::new(Profile {
                display_name: "Ada Lovelace".into(),
                bio: "Writes programs for engines that do not exist yet.".into(),
            }),
        }
    }

    pub async fn get(&self) -> Result<Profile, Error> {
        Ok(self.profile.lock().unwrap().clone())
    }

    pub async fn update(&self, display_name: &str, bio: &str) -> Result<(), Error> {
        let mut p = self.profile.lock().unwrap();
        p.display_name = display_name.to_owned();
        p.bio = bio.to_owned();
        Ok(())
    }
}
```

`AppState` is shared by every request through an `Arc`, so anything you change in it needs interior mutability. A `std::sync::Mutex` is fine as long as you never hold the guard across an `.await`; here each method locks, copies or writes, and returns.

There's exactly one profile, and every visitor edits the same one. That's fine for learning forms. [Signing in](/tutorials/sign-in/) gives each user their own data.

## Step 2: Add a source for it

Replace `src/sources.rs`. Only the `profile` source and its view are new:

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub display_name: String,
    pub bio: String,
}

pub async fn profile(ctx: &Ctx) -> Result<ProfileView, Error> {
    let p = ctx.profile.get().await?;
    Ok(ProfileView { display_name: p.display_name, bio: p.bio })
}
```

## Step 3: Write the action

An *action* handles a form `POST`. Create `src/actions.rs`:

```rust title=src/actions.rs
//! Form actions. By the time one runs, Rust-kit has already checked the
//! request's origin and its CSRF token.

use rustkit::{Error, Form, Redirect};

type Ctx = rustkit::RequestContext<crate::AppState>;

pub async fn update_profile(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    let name = form.get("displayName").unwrap_or("").trim();
    let bio = form.get("bio").unwrap_or("").trim();

    let name_len = name.chars().count();
    if name_len == 0 || name_len > 60 || name.chars().any(char::is_control) {
        return Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()));
    }
    if bio.chars().count() > 280 {
        return Err(Error::BadRequest("Bio must be 280 characters or fewer.".into()));
    }

    ctx.profile.update(name, bio).await?;
    Ok(Redirect::to("/profile"))
}
```

An action receives the request context by value and the submitted fields as a `Form`, and returns either a `Redirect` or an `Error`:

- **`Redirect::to(path)`** answers `303 See Other`. The browser follows it with a `GET`, so reloading the page afterwards doesn't resubmit the form. This is the classic Post/Redirect/Get pattern.
- **`Error::BadRequest(message)`** answers `400` with a small error page that shows your message. Write it for the person filling in the form.

Always validate on the server, even if the form has `maxlength` and `required`. Those attributes help honest visitors; they don't stop anyone who sends the request another way.

## Step 4: Register the action

Replace `src/lib.rs`:

```rust title=src/lib.rs
//! hello-rk: a Rust-kit app.

pub mod actions;
pub mod feed;
pub mod profile;
pub mod sources;
pub mod tasks;

use std::time::Duration;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

/// Shared application state. Resolvers and actions reach it through `ctx`.
pub struct AppState {
    pub tasks: tasks::TaskService,
    pub feed: feed::FeedService,
    pub profile: profile::ProfileStore,
}

pub fn app() -> rustkit::App<AppState> {
    // Set FLAKY_FEED=1 to make every other feed read fail.
    let flaky = std::env::var_os("FLAKY_FEED").is_some();
    let state = AppState {
        tasks: tasks::TaskService::demo(),
        feed: feed::FeedService::new(Duration::from_millis(800), flaky),
        profile: profile::ProfileStore::demo(),
    };
    rustkit::App::new(generated::manifest(), state)
        // Plain http on localhost: don't mark the session cookie `Secure`.
        .secure_cookies(false)
        .action("/profile", actions::update_profile)
}
```

`.action("/profile", …)` registers a `POST` handler. The page at `/profile` answers `GET`, so one URL serves both.

## Step 5: Write the page

Create `src/routes/profile.rk`:

```rk title=profile.rk
<script>
  import { profile } from "$sources";
  let editing = $state(false);
</script>

<title>Profile</title>

<main>
  <DataSource source={profile} load="server" let:data>
    <h1>{data.displayName}</h1>
    <p class="bio">{data.bio}</p>

    <button onclick={() => editing = !editing}>
      {#if editing}Cancel{:else}Edit profile{/if}
    </button>

    {#if editing}
      <form method="post" action="/profile">
        <label>Display name
          <input name="displayName" value={data.displayName} maxlength="60" required>
        </label>
        <label>Bio
          <textarea name="bio" rows="3" maxlength="280">{data.bio}</textarea>
        </label>
        <button>Save</button>
      </form>
    {/if}
  </DataSource>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button, input, textarea { font: inherit; }
  form { display: grid; gap: 0.75rem; margin-top: 1rem; }
  label { display: grid; gap: 0.25rem; }
  .bio { color: #555; }
</style>
```

The form lives inside `{#if editing}`. Since `editing` starts as `false`, the server doesn't render the form at all; the browser renders it on the first click. That raises a question: the form reads `data.displayName` and `data.bio`, which only the server had. Where does the browser get them?

Expand **Generated JavaScript** under the sample and look at the `ssr` section. You'll find entries like `data: P.s["profile"]`. `P` is the handoff payload, and the next step shows what's in it.

Notice also that the form has no CSRF field in the template. You'll find one in the generated code anyway.

## Step 6: Try it in the browser

```bash
cargo run
```

Open [http://127.0.0.1:3000/profile](http://127.0.0.1:3000/profile). You should see "Ada Lovelace" and the bio. Click **Edit profile**: the form appears, filled in, and the Network tab shows no new request. Change the name, click **Save**, and the page reloads with the new name in the heading.

Now submit an empty name. Your browser stops you, because of `required`. Remove that check with the developer tools (or use `curl` in the next step) and you'll get the server's answer: a `400` page saying "Display name must be 1 to 60 characters." with a link back.

> [!NOTE]
> Validation errors show as a plain error page for now. There's no built-in way to re-render the form with the message beside the field and the visitor's input kept. Until there is, keep server rules simple and mirror them with HTML attributes such as `maxlength` and `required`. See the [roadmap](/docs/roadmap/).

## Step 7: Look at the payload and the token

Fetch the page with `curl`, saving cookies to a file:

```bash
curl -si -c cookies.txt http://127.0.0.1:3000/profile
```

Three parts of the response matter here:

```text
cache-control: private, no-store
vary: Cookie
set-cookie: rk_session=1e49…; Path=/; HttpOnly; SameSite=Lax
```

```html
<script type="application/json" id="rk-payload">{"csrf":"2b07…","s":{"profile":{"bio":"Writes programs for engines that do not exist yet.","displayName":"Ada Lovelace"}}}</script>
```

- **The payload carries exactly what the browser needs.** `s.profile` holds `bio` and `displayName` because the browser-rendered editor reads those two fields. If the view had a field the editor doesn't read, it wouldn't be here. JSON in the payload is escaped so it can't close the `<script>` element early, whatever your users type.
- **`csrf` is a per-session token.** Rust-kit adds a hidden `_csrf` input to every `<form method="post">`. When the server renders the form, the token goes straight into the HTML. When the browser renders it, as here, the token comes from the payload.
- **The page now has a session.** Issuing a token needs a session to tie it to, so Rust-kit creates an anonymous one and sets a cookie. Because the response now depends on that session, it's marked `private, no-store` with `Vary: Cookie`, so shared caches won't store one visitor's token and hand it to another.

Now post the form the way a browser would. Copy the `csrf` value into the command:

```bash
TOKEN=paste-the-csrf-value-here
curl -si -b cookies.txt -H 'Origin: http://127.0.0.1:3000' \
  --data-urlencode 'displayName=Ada L.' --data-urlencode 'bio=Hi' \
  --data-urlencode "_csrf=$TOKEN" http://127.0.0.1:3000/profile
```

You get `303 See Other` with `location: /profile`. Then try the ways it can go wrong:

| Request | Response |
| --- | --- |
| No `_csrf` field | `400`, "This form has expired. Reload the page and try again." |
| Valid token, `Origin: https://evil.example` | `403` |
| Valid token, `displayName` of only spaces | `400`, "Display name must be 1 to 60 characters." |

For example:

```bash
curl -si -b cookies.txt --data-urlencode 'displayName=X' http://127.0.0.1:3000/profile
```

Your action code only ever runs for the third row. The first two are refused before it's called.

## What just happened

**The handoff.** When the compiler saw a browser-rendered block (`{#if editing}`) reading server data (`data.displayName`, `data.bio`), it did two things. It added code to `render` that serializes just those fields from the `profile` view into the payload. And it gave the block an `ssr` entry that rebuilds `data` from the payload when the runtime adopts the page. The browser never fetches the profile, and the `profile` source still has no HTTP endpoint.

**The action.** `App::action` mounted a `POST` route. For every request it:

1. Refuses a cross-site request, and refuses an `Origin` header that doesn't match the `Host` the request was sent to (`403`).
2. Loads the session from the cookie and compares the `_csrf` field with the session's token in constant time. Missing or wrong: `400`.
3. Calls your function, then turns `Ok(Redirect)` into a `303` and `Err(e)` into an error page with the matching status.

The `SameSite=Lax` cookie, the origin check and the token are three separate defences against cross-site request forgery. You got all three without writing any of them.

## Next steps

- [Signing in](/tutorials/sign-in/) starts a new project with real users, so each person edits only their own data.
- [Form actions](/docs/form-actions/) is the reference for `App::action`, `Form` and `Redirect`.
- [The handoff payload](/docs/handoff-payload/) explains exactly which fields cross into the page and why.
- [Security](/docs/security/) covers CSRF, origin checks and cache headers together.

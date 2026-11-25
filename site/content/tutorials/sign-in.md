---
title: Signing in
description: Build a small members app with login and logout actions, per-user data behind require_user, and the redirect to /login?next=… for signed-out visitors.
order: 6
level: Intermediate
duration: 30 min
---

## What you'll build

A new app called `members`. It has three pages:

- `/`: a public home page.
- `/login`: a form to sign in as one of two demo users, Alice or Bob.
- `/notes`: the signed-in user's own notes, with a **Sign out** button.

A signed-out visitor who opens `/notes` is sent to `/login?next=/notes`. Alice and Bob each see only their own notes, even when they use the app at the same time. You'll use the same project in [Testing your app](/tutorials/testing-your-app/), [Measuring performance](/tutorials/measuring-performance/) and [Deploying](/tutorials/deploying/).

> [!WARNING]
> The sign-in in this tutorial has no password. It lets you choose a demo account so you can focus on sessions. A real app must verify a credential here, for example a password checked against a slow hash such as Argon2, or the result of an identity-provider flow.

## Prerequisites

- [Your first page](/tutorials/your-first-page/), for project setup.
- [Editing a profile](/tutorials/profile-form/), for actions and CSRF.

## Step 1: Create the project

```bash
cargo new members
cd members
```

```toml title=Cargo.toml
[package]
name = "members"
version = "0.1.0"
edition = "2024"

[dependencies]
rustkit = { git = "https://github.com/nicholas-johnson/rust-kit" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
rustkit-compiler = { git = "https://github.com/nicholas-johnson/rust-kit" }
```

```rust title=build.rs
fn main() {
    rustkit_compiler::build_script();
}
```

## Step 2: An accounts service

Create `src/accounts.rs`. It holds two accounts, each with a few notes, and imitates a network hop with a configurable delay:

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
}
```

## Step 3: A source that needs a user

Create `src/sources.rs`:

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
```

`ctx.require_user()` returns the signed-in `User` (its `id` is whatever string you passed to `login`), or `Err(Error::Unauthorized)`. The `?` hands that error straight back to Rust-kit, which knows what to do with it.

This is the most important line in the tutorial. The resolver doesn't take a user id from the URL or the form. It takes it from the session, so there's no request Bob can make that returns Alice's notes. Authorization lives next to the data it protects, and it applies the same way whether the page loads the source on the server or the browser loads it from `/_rk/source/me`.

## Step 4: Login and logout actions

Create `src/actions.rs`:

```rust title=src/actions.rs
//! Sign-in and sign-out. Rust-kit checks origin and CSRF before these run.

use rustkit::{Error, Form, Redirect};

type Ctx = rustkit::RequestContext<crate::AppState>;

/// Demo sign-in: picks one of the seeded accounts, with no password.
/// A real app verifies a credential or finishes an identity-provider flow here.
pub async fn login(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    let id = form.get("user").unwrap_or("");
    if !ctx.accounts.exists(id) {
        return Err(Error::BadRequest("Choose one of the demo accounts.".into()));
    }
    ctx.login(id);
    Ok(Redirect::to("/notes"))
}

pub async fn logout(ctx: Ctx, _form: Form) -> Result<Redirect, Error> {
    ctx.logout();
    Ok(Redirect::to("/"))
}
```

`ctx.login(id)` destroys the current session, if any, and creates a new one for that user, with a new id and a new CSRF token. Replacing the id at sign-in prevents *session fixation*, where an attacker plants a session id in a victim's browser and waits for them to sign in with it.

`ctx.logout()` destroys the session on the server and tells the browser to delete the cookie.

Signing out is a `POST` with a CSRF token, not a link. A plain `GET /logout` could be triggered by any site that embeds an image pointing at it.

## Step 5: The library and the binary

```rust title=src/lib.rs
//! members: sign-in with per-user data.

pub mod accounts;
pub mod actions;
pub mod sources;

use std::time::Duration;

/// The routes `build.rs` compiled from `src/routes/**/*.rk`.
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/rustkit_generated.rs"));
}

pub struct AppState {
    pub accounts: accounts::AccountsService,
}

/// Builds the app. `latency` is added to every accounts-service call.
/// `secure_cookies` must be true whenever browsers reach the app over https.
pub fn app(latency: Duration, secure_cookies: bool) -> rustkit::App<AppState> {
    let state = AppState { accounts: accounts::AccountsService::demo(latency) };
    rustkit::App::new(generated::manifest(), state)
        .secure_cookies(secure_cookies)
        .action("/login", actions::login)
        .action("/logout", actions::logout)
}
```

`app` takes its settings as parameters so tests and other commands can build it differently. You'll be glad of that in the next few tutorials.

```rust title=src/main.rs
use std::time::Duration;

#[tokio::main]
async fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:3000".into());
    // Local development over plain http, so no `Secure` cookies.
    let app = members::app(Duration::from_millis(20), false);
    let router = app.into_router().await.expect("failed to build the router");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("could not bind");
    println!("listening on http://{addr}");
    axum::serve(listener, router).await.unwrap();
}
```

## Step 6: The pages

The home page is public and the same for everyone, so it's prerendered (you'll learn more about that in [A static blog](/tutorials/static-blog/)):

```rk title=index.rk
<script context="module">
  export const prerender = true;
</script>

<title>Members</title>

<main>
  <h1>Members</h1>
  <p>Each member sees only their own notes.</p>
  <p><a href="/notes">Your notes</a> · <a href="/login">Sign in</a></p>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
</style>
```

Save it as `src/routes/index.rk`. The sign-in page is `src/routes/login.rk`:

```rk title=login.rk
<title>Sign in</title>

<main>
  <h1>Sign in</h1>
  <p>Demo accounts only. There is no password.</p>
  <form method="post" action="/login">
    <label>Account
      <select name="user">
        <option value="alice">Alice</option>
        <option value="bob">Bob</option>
      </select>
    </label>
    <button>Sign in</button>
  </form>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button, select { font: inherit; }
</style>
```

And the notes page is `src/routes/notes.rk`:

```rk title=notes.rk
<script>
  import { me } from "$sources";
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

  <form method="post" action="/logout">
    <button>Sign out</button>
  </form>
</main>

<style>
  body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
  button { font: inherit; }
</style>
```

Neither page mentions sessions. The login form gets its CSRF field automatically, and the notes page is protected because its source calls `require_user`.

## Step 7: Sign in

```bash
cargo run
```

Open [http://127.0.0.1:3000/notes](http://127.0.0.1:3000/notes). You're not signed in, so you land on the sign-in page, and the address bar reads `/login?next=/notes`. Choose **Alice** and click **Sign in**. You arrive at "Alice's notes" with two notes.

Now open a private window (it has its own cookies), go to `/login`, and sign in as **Bob**. Bob sees one note, "Tune the guitar". Reload both windows a few times. Each keeps seeing only its own data.

Click **Sign out** in Alice's window. You're back on the home page. Open `/notes` again and you're sent to sign in.

## Step 8: Watch the cookies

`curl` makes the session flow visible. Start with a signed-out request:

```bash
curl -si http://127.0.0.1:3000/notes
```

```text
HTTP/1.1 303 See Other
location: /login?next=/notes
cache-control: no-store
```

The resolver returned `Unauthorized`, and because this was a page request, Rust-kit turned it into a redirect to the login path with the page's path in `next`. The browser endpoint for a client source returns `401` instead, since a fetch can't usefully follow a redirect to a form.

Now sign in, keeping cookies in a file:

```bash
curl -s -c jar.txt http://127.0.0.1:3000/login > login.html
TOKEN=$(grep -o 'name="_csrf" value="[0-9a-f]*"' login.html | cut -d'"' -f4)
curl -si -b jar.txt -c jar.txt -d "user=alice&_csrf=$TOKEN" http://127.0.0.1:3000/login
```

The `GET /login` response set an `rk_session` cookie, because the form's CSRF token needs a session. The `POST` response sets a *different* `rk_session` value: that's the rotation you read about in Step 4. Now `curl -s -b jar.txt http://127.0.0.1:3000/notes` returns Alice's page.

Two more details:

- The token from `login.html` no longer works after sign-in. A new session has a new token. Post `/logout` with the old token and you get `400`; take the token from the notes page instead.
- `logout` answers with `set-cookie: rk_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`, which tells the browser to drop the cookie.

## About `next`

The redirect puts the page you wanted in `?next=`, but neither pages nor actions can read query parameters yet, so this tutorial's `login` action always sends you to `/notes`. When query access lands you'll be able to redirect to `next`. When you do, accept only paths that start with a single `/`, so the parameter can't send people to another site. Track it on the [roadmap](/docs/roadmap/).

Also note that if an *action* returns `Unauthorized` (say, `require_user` inside a form handler), Rust-kit redirects to the login path without a `next`. The login path is `/login` by default; change it with `App::login_path("/sign-in")`.

## What just happened

- **Sessions are server-side.** The cookie holds only an opaque, random 256-bit id. The user id and CSRF token stay in a store on the server. The cookie is `HttpOnly` (scripts can't read it) and `SameSite=Lax`.
- **Pages became personal automatically.** Any response that reads the session (through `user`, `require_user` or a CSRF token) is sent with `Cache-Control: private, no-store` and `Vary: Cookie`. The prerendered home page never touches the session, so it stays `public, max-age=300`. Compare them with `curl -I`.
- **Authorization lives in the resolver.** The page didn't need an "is signed in" check, and there's no second place to forget one.

> [!IMPORTANT]
> This app uses the default in-memory store, so restarting the server signs everyone out. To keep the sign-in, pass `SessionStore::persistent(path)` to `App::session_store`. The cookie, the user id and the CSRF token then survive a restart. Two processes still must not share that file. [Sessions](/docs/sessions/) and [Deploying](/tutorials/deploying/) cover it.

## Next steps

- [Testing your app](/tutorials/testing-your-app/) turns everything you just checked by hand into automated tests.
- [Sessions](/docs/sessions/) documents `login`, `logout`, `user`, `require_user` and the cookie.
- [Security](/docs/security/) summarises every default that protected this app.

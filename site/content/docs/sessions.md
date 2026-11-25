---
title: Sessions
description: The opaque rk_session cookie, the session store, signing users in and out, and how reading the session changes caching.
section: Server
order: 2
---

Rust-kit keeps session state on the server. The browser holds only a random identifier in a cookie called `rk_session`. Everything else, the signed-in user and the CSRF token, lives in a session store inside your process.

You interact with sessions through the `RequestContext` your resolvers, pages and actions receive. There is no session object to pass around and no way for a page template to read the session directly.

## The cookie

When the framework creates a session, it sets:

```http
set-cookie: rk_session=5f0c…e91a; Path=/; HttpOnly; SameSite=Lax; Secure
```

| Part | Why |
| --- | --- |
| Value | 256 bits from the operating system's secure random source, hex encoded (64 characters). It carries no data and cannot be guessed. |
| `Path=/` | One session for the whole site. |
| `HttpOnly` | Page scripts cannot read it, so a script injection cannot steal it. |
| `SameSite=Lax` | Browsers do not send it on cross-site POSTs, sub-resource requests or `fetch` calls. Top-level links from other sites still work. |
| `Secure` | Sent only over HTTPS. Controlled by `App::secure_cookies`; see below. |

There is no `Max-Age` or `Expires`, so it is a browser-session cookie. How long the session stays valid is decided on the server.

## The session store

`SessionStore` is a map from session id to the user id, the CSRF token and the time the session was last used.

- **Idle timeout.** A session expires after 12 hours without a request that loads it. Each request that finds the session resets the clock.
- **Expiry is lazy.** An expired session is removed when someone presents its id, not on a timer.
- **One process.** Two server processes do not share sessions. There is no lock and no shared store.

`App::new` uses an in-memory store with the 12-hour timeout, so a restart of that process signs everyone out. To keep sessions, install a file store:

```rust
let app = rustkit::App::new(generated::manifest(), state)
    .session_store(rustkit::SessionStore::persistent("/var/lib/members/sessions.json"));
```

`SessionStore::persistent` loads the file at startup and rewrites it when a session is created, used, or destroyed. The write goes to a temporary file in the same directory, is flushed, and then replaces the real file. On Unix the file mode is `0600`. A missing file starts empty.

The JSON is a map from session id to `user_id` (a string, or `null` for an anonymous session), `csrf`, and `last_seen_ms` (Unix time in milliseconds). A restart does not move the idle clock. Presenting an id that has already been idle for 12 hours fails, and that id is then removed from the file. A file written before `last_seen_ms` existed is treated as used at startup.

The account demo's `serve` command stores sessions in `sessions.json` in the working directory. `rustkit dev` uses `target/rustkit-dev/<package>/sessions.json` instead, and ignores `session_store`, so a rebuild keeps the dev session.

`SessionStore::new(idle_timeout)` builds an in-memory store with a different timeout. `persistent` always uses 12 hours. The cookie name stays `rk_session`.

> [!IMPORTANT]
> Point one process at the file. A second process writing the same path can overwrite the first process's sessions. Run a single instance. A store that several instances share is on the [roadmap](/docs/roadmap/).

## Who is signed in

Two methods on the context read the session:

```rust
// Option<User>: None for anonymous visitors.
let greeting = match ctx.user() {
    Some(user) => format!("Welcome back, {}", user.id),
    None => "Welcome".to_owned(),
};

// Result<User, Error>: Err(Error::Unauthorized) for anonymous visitors.
let user = ctx.require_user()?;
```

`User` has one field, `id: String`, which is whatever you passed to `login`. Load anything else (name, roles) from your own services, keyed by that id.

Use `require_user` in resolvers and actions that need a signed-in user. Its error becomes the right response for the context: a `303` to sign-in for a page, a `401` for the source endpoint. Use `user` when anonymous visitors are fine but should see something different.

At build time, for [prerendered](/docs/prerendering/) pages, `require_user` returns an `Error::Internal` explaining that a prerendered route read the session, so the mistake fails loudly rather than producing a page for no one.

## Signing in

Call `ctx.login(user_id)` in an action after you have verified who the visitor is:

```rust
pub async fn login(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    // Your own check: a password hash, an identity provider, a magic link.
    let id = verify_credentials(&ctx, &form).await?;
    ctx.login(id);
    Ok(Redirect::to("/account"))
}
```

`login` does three things:

1. destroys the visitor's current session, if they had one;
2. creates a new session with a new random id, the given user id and a new CSRF token;
3. sets the new `rk_session` cookie on the response.

Step 1 and 2 together are **session rotation**. An attacker who planted a session id in a victim's browser before sign-in (session fixation) is left holding an id that no longer exists. The acceptance tests check that the cookie after sign-in differs from the anonymous one the sign-in page issued.

## Signing out

`ctx.logout()` destroys the session on the server and clears the cookie:

```http
set-cookie: rk_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0
```

Even if the browser kept the old cookie, the id no longer matches anything in the store. Sign-out should be a POST form so that another site cannot sign your visitors out with a link or an image:

```rk title=nav.rk
<title>Home</title>

<nav>
  <a href="/">Home</a>
  <form method="post" action="/logout"><button>Sign out</button></form>
</nav>
```

## Anonymous sessions and CSRF tokens

`ctx.csrf_token()` returns the session's CSRF token. If the visitor has no session, it **creates an anonymous one** (no user) and sets the cookie, so the token it returns can be checked when the form comes back.

You rarely call it yourself. The compiler calls it whenever a page renders a `<form method="post">`, or a browser-rendered block might render one. So the first visit to a page with a form, such as a sign-in page, starts a session:

```http
GET /login HTTP/1.1

HTTP/1.1 200 OK
cache-control: private, no-store
vary: Cookie
set-cookie: rk_session=…; Path=/; HttpOnly; SameSite=Lax; Secure
```

Each anonymous session takes a small amount of memory, and a record in the session file when you use one, until it idles out. See [Security](/docs/security/#what-is-not-covered) for what that means for abuse.

## Secure cookies in development

`Secure` cookies are not stored by browsers over plain HTTP, except on `localhost` in most browsers. The runtime adds `Secure` by default. Turn it off for local HTTP development with `App::secure_cookies(false)`:

```rust
let local = addr.starts_with("127.0.0.1");
let app = rustkit::App::new(generated::manifest(), state)
    .secure_cookies(!local);
```

The demo does exactly this. Leave it on anywhere that is not your own machine.

> [!NOTE]
> `secure_cookies` affects the cookie set at sign-in and for new anonymous sessions. The cookie that `logout` sends to clear the session never includes `Secure`.

## The personalised flag

Every context tracks whether the response depends on the session. These calls set the flag:

| Call | Sets personalised |
| --- | --- |
| `ctx.user()` | yes, even when it returns `None` |
| `ctx.require_user()` | yes (it calls `user()`) |
| `ctx.csrf_token()` | yes |
| `ctx.login(…)`, `ctx.logout()` | yes |
| `Deref` to `AppState` | no |

When a page render finishes, the router checks the flag:

- **personalised** → `Cache-Control: private, no-store` and `Vary: Cookie`;
- **not personalised** → `Cache-Control: no-cache`.

This is automatic. You cannot forget to mark a page private, because the same call that reads the user marks it. One consequence: any page with a POST form is personalised, because it renders a CSRF token. [Caching](/docs/caching/) lists every header the runtime sets.

## Not available yet

- Storing your own values in the session. `SessionData` holds only the user id and CSRF token.
- Configuring the cookie name, domain, lifetime or idle timeout through `App`. `persistent` uses the 12-hour timeout.
- A store shared by more than one process.
- Signing out all of a user's sessions.

See the [roadmap](/docs/roadmap/).

---
title: Form actions
description: Handle POSTed HTML forms with an async Rust function, with origin and CSRF checks done for you and a 303 redirect on success.
section: Server
order: 1
---

Rust-kit changes data the way the web always has: an HTML `<form method="post">` sends fields to a URL, the server does the work, and the browser is redirected to a page that shows the result. On the server, that URL is handled by a **form action**, an async Rust function you register on the `App`.

There is no client-side mutation API. Every write goes through an action, works without JavaScript, and is protected against cross-site request forgery before your code runs.

## A complete example

The page posts to `/account/profile`:

```rk title=profile.rk
<script>
  import { account } from "$sources";
</script>

<title>Edit profile</title>

<DataSource source={account} load="server" let:data>
  <form method="post" action="/account/profile">
    <label>Display name <input name="name" value={data.displayName} /></label>
    <button>Save</button>
  </form>
</DataSource>
```

The action validates the input, calls the service, and redirects:

```rust
use rustkit::{Error, Form, Redirect};

type Ctx = rustkit::RequestContext<crate::AppState>;

pub async fn update_profile(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    let user = ctx.require_user()?;
    let name = form.get("name").unwrap_or("").trim();
    let len = name.chars().count();
    if len == 0 || len > 60 || name.chars().any(char::is_control) {
        return Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()));
    }
    ctx.accounts_service.update_display_name(&user.id, name).await?;
    Ok(Redirect::to("/account"))
}
```

And the app registers it:

```rust
rustkit::App::new(generated::manifest(), state)
    .action("/account/profile", actions::update_profile)
    .action("/login", actions::login)
    .action("/logout", actions::logout)
```

## Registering an action

`App::action(path, f)` adds a `POST` route at `path`. The function takes the request context **by value** and the submitted form, and returns `Result<Redirect, Error>`:

```rust
pub fn action<F, Fut>(self, path: &str, f: F) -> Self
where
    F: Fn(RequestContext<S>, Form) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Redirect, Error>> + Send + 'static;
```

The context is owned rather than borrowed so the future can be `'static`. It is a cheap `Arc` clone, and it dereferences to your `AppState` just as it does in resolvers.

An action path can be the same as a page path. In the demo, `GET /login` renders `login.rk` and `POST /login` runs `actions::login`.

## Reading fields with `Form`

`Form` wraps the decoded `application/x-www-form-urlencoded` body as a `HashMap<String, String>`. Use `form.get(name)`, which returns `Option<&str>`:

```rust
let name = form.get("name").unwrap_or("").trim();
```

Each field name holds one value. The hidden `_csrf` field is still present in the map; ignore it. File uploads (`multipart/form-data`) are not supported. A body that cannot be decoded as a URL-encoded form is rejected by the underlying router before your action runs, with a plain-text error rather than a Rust-kit error page.

## Redirecting with `Redirect::to`

On success, an action returns `Redirect::to(path)`. The framework sends:

```http
HTTP/1.1 303 See Other
location: /account
cache-control: no-store
set-cookie: rk_session=…; Path=/; HttpOnly; SameSite=Lax; Secure
```

`303 See Other` tells the browser to follow the redirect with a `GET`. That is the Post/Redirect/Get pattern: the visitor lands on an ordinary page, and pressing reload re-fetches that page instead of re-submitting the form. The `Set-Cookie` line is only present when the action changed the session, for example by calling `ctx.login(...)`.

The next page render calls its resolvers again, so it shows the new data. The acceptance test `saving_the_profile_updates_upstream_and_the_next_page` posts a new name and checks that the next `GET /account` contains `<h1>Hello, Alice L.</h1>`.

## The CSRF field is added for you

Every `<form method="post">` in a page template gets a hidden field as its first child:

```html
<form method="post" action="/account/profile"><input type="hidden" name="_csrf" value="9b1e…">
```

You never write this field. The compiler inserts it for any `form` whose `method` attribute is the literal `post` (any case):

- If the server renders the form, the Rust renderer writes the session's token into the HTML.
- If the browser renders the form, for example inside `{#if editing}`, the token comes from the [handoff payload](/docs/handoff-payload/#the-csrf-token).

Rendering the token calls `ctx.csrf_token()`, which creates an anonymous session if the visitor does not have one. That is why the demo's sign-in page sets a cookie before anyone has signed in. See [Sessions](/docs/sessions/).

> [!WARNING]
> Do not put a POST form on a [prerendered](/docs/prerendering/) page. The token would be generated once, at build time, for a session no visitor has, and every submission would be rejected as expired.

## What runs before your action

For every POST to a registered action path, in this order:

1. **Origin check.** If `Sec-Fetch-Site` is `cross-site`, or an `Origin` header is present and is not `http://` or `https://` followed by the request's `Host`, the response is `403`.
2. **CSRF check.** The `_csrf` field must match the token stored in the visitor's session, compared in constant time. If the field is missing, the session is missing, or the values differ, the response is `400` with "This form has expired. Reload the page and try again."
3. **Your action** runs with the context and the form.

Details and reasoning are in [Security](/docs/security/#csrf-protection).

## Validation errors

To reject input, return `Error::BadRequest` with a message written for the visitor:

```rust
return Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()));
```

The visitor sees a `400` page that shows your message (HTML-escaped) and a "Go back" link. In v0.1 there is no way to re-render the original page with the error next to the field and the visitor's input preserved. Keep messages specific so going back and fixing the field is easy. Inline validation errors are on the [roadmap](/docs/roadmap/).

Other errors map as usual. An `Error::Unauthorized` from an action, typically from `ctx.require_user()?`, redirects with `303` to the login path. Unlike page renders, the action redirect does not add `?next=`. See [Errors](/docs/errors/).

## Progressive enhancement

Actions are plain HTML forms, so they work before the page's JavaScript has loaded, when it fails to load, and with JavaScript turned off. The browser runtime does not intercept form submissions; even a form the browser rendered submits as a normal navigation.

JavaScript adds things around the form, not instead of it. In the account page, a `$state` variable shows and hides the editor. The form inside is still an ordinary POST with its CSRF token.

A useful habit: whenever you add an interaction, ask what happens without JavaScript. If the form is inside a block that only appears after a click, consider putting a link to a server-rendered page with the same form next to the button.

## Sign-in and sign-out

Sign-in and sign-out are actions too. The demo's versions:

```rust
pub async fn login(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    match form.get("user") {
        Some(id @ ("alice" | "bob")) => {
            ctx.login(id);
            Ok(Redirect::to("/account"))
        }
        _ => Err(Error::BadRequest("Choose one of the demo accounts.".into())),
    }
}

pub async fn logout(ctx: Ctx, _form: Form) -> Result<Redirect, Error> {
    ctx.logout();
    Ok(Redirect::to("/"))
}
```

The demo accepts any seeded account without a password. A real app verifies a credential or completes an identity-provider flow before calling `ctx.login`. [Sessions](/docs/sessions/) covers what `login` and `logout` do to the cookie.

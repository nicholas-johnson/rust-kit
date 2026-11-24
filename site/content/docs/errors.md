---
title: Errors
description: The rustkit::Error type, the status code and page each variant produces, how sign-in redirects work, and what is logged.
section: Server
order: 6
---

Resolvers, page renders and actions all return `Result<_, rustkit::Error>`. You do not build error responses yourself. You return the variant that describes what went wrong, and the framework turns it into the right response for where it happened: an HTML page, a JSON body, or a redirect to sign-in.

## The `Error` type

```rust
pub enum Error {
    /// No signed-in user. Pages redirect to the login route; the API returns 401.
    Unauthorized,
    Forbidden,
    NotFound,
    /// The request was understood but its input was invalid.
    BadRequest(String),
    /// An upstream service failed or returned something unusable.
    Upstream(String),
    Internal(String),
}
```

| Variant | Status | Public message | Logged |
| --- | --- | --- | --- |
| `Unauthorized` | `401` (pages and actions redirect instead; see below) | Please sign in. | no |
| `Forbidden` | `403` | You don't have access to this. | no |
| `NotFound` | `404` | Not found. | no |
| `BadRequest(msg)` | `400` | `msg`, as written | no |
| `Upstream(detail)` | `502` | A service this page depends on is unavailable. Please try again. | yes |
| `Internal(detail)` | `500` | Something went wrong. | yes |

`Error::status()` returns the code and `Error::public_message()` returns the text. These are the only two things a visitor ever sees.

## Choosing a variant

- **`Unauthorized`**: nobody is signed in. You will rarely construct it; `ctx.require_user()?` returns it.
- **`Forbidden`**: someone is signed in, but this is not theirs. Also returned by the framework for cross-site requests.
- **`NotFound`**: the thing does not exist. In the demo, the upstream service returns it for an unknown account id.
- **`BadRequest(msg)`**: the visitor sent something invalid. Write `msg` for the visitor: it is shown on the page.
- **`Upstream(detail)`**: a service you depend on failed, timed out, or returned something you could not use. Put the diagnostic detail in the string; it is logged, not shown.
- **`Internal(detail)`**: a bug or an unexpected state on your side. Same logging rules as `Upstream`.

Two helpers make conversion easier:

```rust
// Any Display value becomes Error::Internal with its text.
let cfg = std::fs::read_to_string(path).map_err(Error::internal)?;

// serde_json errors convert automatically with `?`.
let v: Thing = serde_json::from_str(&body)?;
```

For upstream clients, map their errors explicitly so failures show up as `502` rather than `500`:

```rust
let res = client.get(url).send().await
    .map_err(|e| Error::Upstream(format!("accounts: {e}")))?;
```

(`client` here stands for whatever HTTP client your app uses; Rust-kit does not ship one.)

## Errors while rendering a page

A page render calls every `load="server"` resolver before writing any HTML. If one fails, nothing has been sent yet, so the framework can send a clean error page instead:

```http
HTTP/1.1 404 Not Found
content-type: text/html; charset=utf-8
cache-control: no-store
```

```html
<!doctype html><html lang="en"><head><meta charset="utf-8"><title>404</title></head><body><h1>404</h1><p>Not found.</p><p><a href="javascript:history.back()">Go back</a></p></body></html>
```

The page is deliberately minimal and has no styling. The message is HTML-escaped. Custom error pages are not available yet; see the [roadmap](/docs/roadmap/).

Any URL that matches no route gets the same `404` page.

## Unauthorized becomes a sign-in redirect

When a page render fails with `Error::Unauthorized`, a `401` page would be a dead end. Instead the visitor is sent to sign in, with the page they wanted in `next`:

```http
GET /account HTTP/1.1

HTTP/1.1 303 See Other
location: /login?next=/account
cache-control: no-store
```

The acceptance test `signed_out_visitors_are_sent_to_sign_in` checks exactly this. `next` is the route's path; any query string on the original request is not included.

The sign-in path defaults to `/login`. Change it with `App::login_path`:

```rust
rustkit::App::new(generated::manifest(), state)
    .login_path("/sign-in")
```

Rust-kit does not read `next` for you. If you want visitors to return to the page they asked for, your sign-in page has to carry `next` through to the action, and the action has to redirect to it. Sources and pages take no URL parameters in v0.1, so there is no built-in way to read `next` in a template yet.

> [!WARNING]
> If you do redirect to `next`, check that it is a local path (starts with `/` and not `//`). Otherwise your sign-in page becomes an open redirect.

### In actions

When a [form action](/docs/form-actions/) returns `Error::Unauthorized`, for example because the session expired while the form was open, the response is also `303`, but to the login path **without** `next`:

```http
HTTP/1.1 303 See Other
location: /login
```

## Errors from the source endpoint

`GET /_rk/source/{name}` is called by the browser runtime, which expects JSON. Its errors are JSON too, with the same status codes and public messages, and no redirect:

```http
HTTP/1.1 401 Unauthorized
content-type: application/json
cache-control: no-store

{"error":"Please sign in."}
```

| Situation | Status | Body |
| --- | --- | --- |
| Resolver returns an error | per the table above | `{"error": public_message}` |
| `Sec-Fetch-Site: cross-site` | `403` | `{"error":"You don't have access to this."}` |
| Name is not a client-loaded source | `404` | `{"error":"Not found."}` |

In the browser, any non-2xx response puts the region into its error state, with a Retry button. The runtime does not read the error message. See [Client loading](/docs/client-loading/#what-the-visitor-sees).

## Errors the framework raises itself

| Situation | Response |
| --- | --- |
| Action POST with a foreign `Origin` or `Sec-Fetch-Site: cross-site` | `403` page |
| Action POST with a missing or wrong `_csrf` token | `400` page: "This form has expired. Reload the page and try again." |
| A prerendered page's resolver calls `require_user` | `Error::Internal` from `into_router` or `build`; the app does not start |
| No matching route | `404` page |

Some request problems are rejected by axum, the router underneath, before Rust-kit sees them: for example a POST body that is not URL-encoded form data, or a `GET` to a path that only has an action. Those responses come from axum and are not Rust-kit error pages.

## What gets logged

For `Upstream` and `Internal` errors, the framework writes one line to standard error with the full detail:

```text
rustkit: upstream error: accounts: connection refused
rustkit: internal error: missing field `displayName` at line 1 column 2
```

This happens when the error becomes a response: an error page or a JSON error. Errors returned from `App::into_router` or `App::build`, such as a failing prerender, are handed back to your code instead, and it is up to you to print them.

Nothing else is logged: not other error variants, not successful requests. There is no integration with `log` or `tracing` yet. If you need structured logs or request logs, add a tower layer to the router returned by `App::into_router`, and log inside your resolvers and actions where you have context.

## Public messages are safe to show

`public_message()` never includes the detail string of `Upstream` or `Internal`. That is the point of the split: you can write `Error::Upstream(format!("GET {url} returned {status}: {body}"))` with everything you need for debugging, and the visitor sees only the generic sentence.

`BadRequest` is the exception. Its string *is* the public message, because it exists to tell the visitor what to fix. Keep it free of internal details:

```rust
// Good: tells the visitor what to do.
Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()))

// Bad: leaks implementation details to the page.
Err(Error::BadRequest(format!("validation failed in update_profile: {e:?}")))
```

## Not available yet

- Custom error pages or templates.
- Rendering the original page again with an inline error (for example, form validation messages next to fields).
- Structured logging.

See the [roadmap](/docs/roadmap/).

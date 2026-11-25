---
title: Security
description: The protections Rust-kit applies by default, how each one works, and what you still have to handle yourself.
section: Server
order: 3
---

Rust-kit's security defaults are designed so that the obvious way to write a page is also the safe one. Most of them need no code from you: you cannot forget a CSRF field the compiler writes, or an escape the renderer always applies. This page explains each protection, the reasoning behind it, and the gaps v0.1 leaves for you to fill.

## At a glance

| Threat | Default protection |
| --- | --- |
| Cross-site request forgery | Per-session CSRF token in every POST form, checked in constant time; `Origin` and `Sec-Fetch-Site` checks |
| Cross-site data reads | `Sec-Fetch-Site: cross-site` refused on source endpoints; `SameSite=Lax` cookie |
| Script injection (XSS) | Every bound value HTML-escaped in both renderers; payload JSON escaped for `<script>` |
| Session theft and fixation | Opaque random `HttpOnly` cookie; id rotated at sign-in |
| Over-sharing data | Only view fields leave the server; only fields browser code reads go into the payload |
| Shared caches serving private pages | `private, no-store` and `Vary: Cookie` on anything that read the session |
| Leaking internals in errors | Generic public messages for upstream and internal errors |

## CSRF protection

A cross-site request forgery tricks a signed-in visitor's browser into submitting a form to your site from another one. Rust-kit applies three independent checks to every POST to a registered [form action](/docs/form-actions/), before the action runs.

### The token

Each session has a CSRF token: 256 random bits, created with the session. The compiler adds `<input type="hidden" name="_csrf" value="…">` to every `<form method="post">` in your templates, whether the server or the browser renders it. The action handler compares the submitted `_csrf` with the session's token.

The comparison is constant-time: `rustkit::constant_time_eq` checks the lengths, then XORs every byte pair and only looks at the accumulated result. It takes the same time no matter where the first difference is, so an attacker cannot learn the token one character at a time by measuring response times.

A missing token, a missing session or a mismatch all give the same response:

```http
HTTP/1.1 400 Bad Request
content-type: text/html; charset=utf-8
cache-control: no-store

…<h1>400</h1><p>This form has expired. Reload the page and try again.</p>…
```

### The origin check

Before checking the token, the action handler looks at two headers:

1. If `Sec-Fetch-Site` is `cross-site`, the request is refused with `403`.
2. If an `Origin` header is present, it must be exactly `http://{Host}` or `https://{Host}`, where `{Host}` is the request's `Host` header. Otherwise the request is refused with `403`.

Requests with no `Origin` header pass this step and rely on the token. Modern browsers send `Origin` on every POST, so a forged cross-site form fails here even before the token is looked at. The acceptance test `profile_action_rejects_forged_and_invalid_posts` sends a valid token with `Origin: https://evil.example` and gets `403`.

> [!WARNING]
> The check compares `Origin` with the `Host` header your server receives. If a reverse proxy rewrites `Host` (for example to an internal name), legitimate form posts will fail with `403`. Configure the proxy to pass the public `Host` through. See [Deployment](/docs/deployment/).

### SameSite cookies

The session cookie is `SameSite=Lax`, so browsers do not attach it to cross-site POSTs at all. That is a third layer: even without the other two checks, a forged POST would arrive without a session.

## Source endpoints

`GET /_rk/source/{name}` returns personal data as JSON, so it gets its own protections:

- It refuses any request whose `Sec-Fetch-Site` is `cross-site` with `403`, even when it carries a valid cookie.
- It runs the same resolver as server rendering, so the same `require_user` check applies. An anonymous request gets `401`.
- It exists only for sources some page loads with `load="client"`. Server-only sources have no endpoint.
- Responses are `private, no-store` with `Vary: Cookie`.

The test `users_only_see_their_own_data` checks that Alice and Bob each receive only their own activity through both the page and the endpoint, that anonymous requests get `401`, and that a cross-site request gets `403`.

## Escaping

Every value a template binds is escaped, in both renderers, with the same five replacements:

| Character | Escaped as |
| --- | --- |
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |
| `'` | `&#39;` |

This applies to text (`{data.name}`) and attribute values (`value={data.name}`). There is no "raw HTML" binding in v0.1, so there is no way to opt out by accident. Static text you write in the template is written as-is, because you wrote it.

Event handlers are JavaScript you write, compiled into the page module. They are never built from data, so data cannot become code through them.

> [!NOTE]
> Escaping makes a value safe as text, not safe as a URL. `href={data.website}` will happily produce `href="javascript:…"` if that is what the data contains. Validate URLs in your resolver before putting them in a view.

## The payload

Server data that browser code needs travels in `<script type="application/json" id="rk-payload">`. It is serialized with `rustkit::js::html_safe_json`, which escapes `<`, `>`, `&`, U+2028 and U+2029 as JSON `\u` sequences, so a value can never close the script element. The test `user_text_is_escaped_in_html_and_payload` saves a display name of `</script><img src=x onerror=alert(1)>` and checks that it appears only escaped. Details are in [Handoff payload](/docs/handoff-payload/#escaping).

## Only declared fields leave the server

Data reaches the browser in exactly two ways, and both are limited by the view types you write:

1. **The view struct.** Resolvers return a view, not an upstream record. Fields that are not on the view cannot be rendered, serialized or fetched. In the demo, the upstream `email` field is not on `AccountView`, and the tests check it never appears.
2. **The payload projection.** For server-loaded sources, only fields read by browser-rendered blocks and handlers are sent.

The view is the real boundary. If a source is also loaded with `load="client"` anywhere in the app, its whole view is available from the endpoint.

## Sessions

The session cookie is an opaque 256-bit random id, `HttpOnly`, `SameSite=Lax`, and `Secure` unless you turn it off with `App::secure_cookies(false)`. All session state is on the server. Signing in destroys the old session and issues a new id, which defeats session fixation. Signing out destroys the session on the server. See [Sessions](/docs/sessions/).

## Cache isolation

Any response that read the session is sent with `Cache-Control: private, no-store` and `Vary: Cookie`, so CDNs and shared proxies will not store it or serve it to someone else. The flag is set by the same calls that read the session (`user`, `require_user`, `csrf_token`, `login`, `logout`), so it cannot drift out of step with the code. Source endpoint responses are always private. See [Caching](/docs/caching/).

## Error messages

Error pages and JSON errors show only `Error::public_message()`. For `Upstream` and `Internal` errors that is a fixed, generic sentence; the detail goes to standard error on the server. The message in `Error::BadRequest` is shown as written (escaped), so do not put internal details in it. See [Errors](/docs/errors/).

## What is not covered

These are your responsibility in v0.1, or not available yet:

- **Authentication.** Rust-kit stores who is signed in, but checking passwords or talking to an identity provider is up to your action. The demo signs in without a password.
- **Authorization beyond "signed in".** `require_user` only says someone is signed in. Whether that user may see a record is your resolver's job. A resolver may receive the route's one parameter, which is caller-controlled, so do not treat it as proof of access. Upstream calls must still be scoped to `user.id`.
- **Rate limiting and brute-force protection.** There is none. Add it in front of the app or around your actions.
- **Session store limits.** Each visit to a page with a form creates an anonymous in-memory session, and expired sessions are only removed when presented again. A flood of requests can grow memory. Put the app behind something that limits request rates.
- **Security headers.** The runtime does not set `Content-Security-Policy`, `Strict-Transport-Security`, `X-Frame-Options` or similar. Add them at your proxy or with an axum layer on the router from `App::into_router`.
- **TLS.** Terminate HTTPS in a proxy or load balancer.
- **URL and CSS injection.** Escaping does not validate URLs or style values. Validate them in resolvers.
- **Request body limits** beyond the defaults of the underlying axum router.

See the [roadmap](/docs/roadmap/) for planned work, and [FAQ](/docs/faq/) for common questions.

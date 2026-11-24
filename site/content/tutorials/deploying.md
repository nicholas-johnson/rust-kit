---
title: Deploying
description: Build a release binary, prerender its static pages, and run it behind a TLS reverse proxy with secure cookies, knowing the limits of in-memory sessions.
order: 10
level: Advanced
duration: 40 min
---

## What you'll build

A production setup for the `members` app:

- a release binary that contains every page and asset it serves;
- a `build` command that writes the prerendered pages and assets to a folder, for a CDN or the proxy to serve;
- the binary listening on `127.0.0.1:3000`, with `Secure` session cookies, behind a reverse proxy that handles TLS on port 443;
- a systemd unit that starts it on boot and restarts it if it crashes.

You'll also learn the one constraint that shapes every Rust-kit deployment today: sessions live in the server's memory.

## Prerequisites

- The `members` project as you left it at the end of [Measuring performance](/tutorials/measuring-performance/).
- For the real deployment: a Linux server with systemd, a domain name pointing at it, and Caddy or nginx as a reverse proxy. You can do Steps 1 to 4 on your own machine.

The server configuration in this tutorial is an example. Adapt paths, names and hardening to your environment.

## Step 1: Make cookies secure by default

Until now, `main.rs` passed `false` to `secure_cookies` so sign-in would work over plain `http://127.0.0.1`. In production, browsers reach the app over HTTPS, and the session cookie must carry `Secure` so it's never sent over an unencrypted connection.

Flip the default: cookies are secure unless you explicitly say otherwise. Replace `src/main.rs`:

```rust title=src/main.rs
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("report") => report(),
        Some("build") => build(args.get(1).map_or("dist", String::as_str)).await,
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            serve(addr).await;
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; use `serve [addr]`, `build [dir]` or `report`");
            std::process::exit(2);
        }
    }
}

/// Simulated upstream latency in milliseconds, from `LATENCY_MS` (default 20).
fn latency() -> Duration {
    let ms = std::env::var("LATENCY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    Duration::from_millis(ms)
}

/// Session cookies carry `Secure` unless `SECURE_COOKIES=0`. Only turn it off
/// for local development over plain http.
fn secure_cookies() -> bool {
    std::env::var("SECURE_COOKIES").map_or(true, |v| v != "0")
}

async fn serve(addr: &str) {
    let secure = secure_cookies();
    let app = members::app(latency(), secure);
    let router = app.into_router().await.expect("failed to build the router");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("could not bind");
    println!("listening on http://{addr} (secure cookies: {secure})");
    axum::serve(listener, router).await.unwrap();
}

/// Writes prerendered pages and every asset to `dir`.
async fn build(dir: &str) {
    let app = members::app(Duration::ZERO, true);
    let report = app.build(&PathBuf::from(dir)).await.expect("build failed");
    println!("wrote {} files to {dir}", report.files.len());
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

From now on, local development over plain HTTP looks like this:

```bash
SECURE_COOKIES=0 cargo run
```

Why make the safe setting the default? Because the failure modes aren't symmetric. Forgetting `SECURE_COOKIES=0` locally breaks sign-in on your laptop, and you notice in seconds. Forgetting to turn `Secure` *on* in production works perfectly and leaks session cookies over any plain-HTTP request, and nobody notices.

> [!NOTE]
> `rustkit::App::new` already defaults to secure cookies. The `false` you passed in earlier tutorials was the exception, and this step puts the decision in one visible place.

## Step 2: Build for release

```bash
cargo build --release
```

The binary is `target/release/members`. It's self-contained: the compiled pages are Rust code, and every CSS and JavaScript asset is embedded with `include_str!` when the crate compiles. To deploy, you copy one file. There's no template folder, `node_modules` or asset directory to keep in sync.

Check what it serves before you ship it:

```bash
./target/release/members report
```

## Step 3: Prerender to a static directory

The `build` command writes everything that doesn't need a request:

```bash
./target/release/members build dist
```

```text
wrote 6 files to dist
```

`dist/` holds `index.html` (the prerendered home page) and every file under `_rk/assets/`, including the notes page's module and the shared runtime. `/login` and `/notes` aren't there, because they're rendered per request.

You don't *need* this folder, since the binary serves all of it itself. It's useful when you want a CDN or your proxy to serve the static parts without touching the Rust process. Asset names are content-hashed, so you can upload a new `dist/_rk/assets/` beside the old one and let both exist while deploys roll over.

## Step 4: Try production mode locally

Run the release binary the way production will, with secure cookies and no simulated latency:

```bash
LATENCY_MS=0 ./target/release/members serve 127.0.0.1:3000
```

```bash
curl -si http://127.0.0.1:3000/login | grep -i set-cookie
```

```text
set-cookie: rk_session=…; Path=/; HttpOnly; SameSite=Lax; Secure
```

The cookie now carries `Secure`. Over plain HTTP, browsers handle that inconsistently: some refuse it outright, and some make an exception for `localhost`. Don't rely on either; that's what `SECURE_COOKIES=0` is for. In production, TLS ends at the proxy, so the browser sees HTTPS and keeps the cookie.

`LATENCY_MS=0` matters here only because the accounts service is fake. A real service has real latency, and you'd delete the knob.

## Step 5: Put a TLS proxy in front

The binary speaks plain HTTP on loopback. A reverse proxy accepts HTTPS from the internet and forwards requests to it. Binding to `127.0.0.1` means nothing can reach the app without going through the proxy.

With Caddy, which obtains and renews certificates automatically, the whole configuration is:

```text title=/etc/caddy/Caddyfile
members.example.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:3000
}
```

`encode zstd gzip` compresses responses, which Rust-kit doesn't do itself. Caddy also redirects `http://` to `https://` by default.

If you use nginx, the important line is `proxy_set_header Host $host;`:

```text title=/etc/nginx/sites-enabled/members
server {
    listen 443 ssl;
    server_name members.example.com;
    # ssl_certificate and ssl_certificate_key go here.

    gzip on;
    gzip_types text/css text/javascript application/json;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
    }
}
```

> [!IMPORTANT]
> The proxy must pass the browser's original `Host` header through. Rust-kit accepts a form post only when its `Origin` matches `http://` or `https://` plus the request's `Host`. By default nginx sends the upstream's address (`127.0.0.1:3000`) as `Host`, so every sign-in and every other form post fails with `403`. Caddy preserves `Host` by default.

You can simulate both cases against the local binary. With the host preserved, the origin check passes:

```bash
curl -s -c jar.txt -H 'Host: members.example.com' http://127.0.0.1:3000/login > login.html
TOKEN=$(grep -o 'name="_csrf" value="[0-9a-f]*"' login.html | cut -d'"' -f4)
curl -s -o /dev/null -w '%{http_code}\n' -b jar.txt \
  -H 'Host: members.example.com' -H 'Origin: https://members.example.com' \
  -d "user=alice&_csrf=$TOKEN" http://127.0.0.1:3000/login
```

That prints `303`. Drop the `-H 'Host: …'` from the second request, as a misconfigured proxy would, and it prints `403`.

### Serving the static parts from the proxy (optional)

If you copied `dist/` to the server, the proxy can serve the hashed assets directly and pass everything else to the app:

```text title=/etc/caddy/Caddyfile
members.example.com {
    encode zstd gzip

    handle /_rk/assets/* {
        root * /opt/members/dist
        header Cache-Control "public, max-age=31536000, immutable"
        file_server
    }

    handle {
        reverse_proxy 127.0.0.1:3000
    }
}
```

Only do this with assets built from the *same* binary you're running, or pages will link to hashes the folder doesn't have. Serving everything from the binary is simpler and perfectly fast.

## Step 6: Run it as a service

Copy the binary to the server, for example to `/opt/members/members`. Then create a systemd unit. This is an example; adjust it to your conventions:

```text title=/etc/systemd/system/members.service
[Unit]
Description=members (Rust-kit app)
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/opt/members/members serve 127.0.0.1:3000
Environment=LATENCY_MS=0
Restart=on-failure
RestartSec=2

# Run as a throwaway user with a read-only view of the system.
DynamicUser=yes
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now members
sudo systemctl status members
```

`SECURE_COOKIES` isn't set, so it takes the secure default. The app writes nothing to disk, which is why `ProtectSystem=strict` is fine.

When a resolver or action fails with an upstream or internal error, Rust-kit sends the visitor a generic message and prints the detail to standard error, prefixed with `rustkit:`. systemd collects it:

```bash
journalctl -u members -f
```

## Step 7: Check it from outside

From your own machine:

```bash
curl -sI https://members.example.com/
curl -si https://members.example.com/login | grep -iE 'set-cookie|cache-control'
```

You should see:

- `/` answers `200` with `cache-control: public, max-age=300`, and no cookie. It's prerendered, so the proxy or a CDN may cache it.
- `/login` sets `rk_session` with `Secure`, `HttpOnly` and `SameSite=Lax`, and is `private, no-store`.

Then sign in with a browser, open `/notes`, and sign out. If sign-in fails with `403`, check that the proxy preserves `Host`. If it seems to work but you land back on the sign-in page, check that the site is served over HTTPS and the cookie has `Secure`.

## Step 8: Understand the single-instance limit

Rust-kit's session store is an in-memory map inside the server process. That has three consequences you need to plan for:

1. **A restart signs everyone out.** Deploying a new version, a crash, or a reboot clears every session. For many internal tools that's acceptable. For a public app, deploy at quiet times and keep restarts rare.
2. **Run exactly one instance.** Two processes behind a load balancer don't share sessions, so a visitor who signs in on one and lands on the other is signed out, and their CSRF token is rejected. "Sticky sessions" on the load balancer reduce this, but don't remove it: a restart still loses that instance's sessions.
3. **Scale up, not out.** One Rust process on a multi-core machine handles a lot of traffic, because rendering is native code and every request runs on Tokio's thread pool. When you need more, use a bigger machine.

Sessions also expire after 12 hours without use. Persistent, shared session storage is on the [roadmap](/docs/roadmap/). Until it lands, these limits apply to any app that signs people in. A fully prerendered site, like the one in [A static blog](/tutorials/static-blog/), has none of them: copy `dist/` to any static host and you're done.

## What just happened

- **One artifact.** The release binary contains the router, every compiled page, every asset and the runtime. `build` exports the static subset for anyone who wants it.
- **TLS at the edge.** The proxy terminates HTTPS and compresses responses. The app listens only on loopback, sets `Secure` cookies, and relies on the proxy to pass `Host` through so its origin checks still work.
- **Safe defaults where it counts.** Cookies are secure unless you say otherwise. Personal pages are never publicly cacheable. Errors stay out of responses and go to the log.

### Deployment checklist

- Release build, copied as a single binary.
- `SECURE_COOKIES` unset (or anything but `0`) in production.
- App bound to `127.0.0.1`, reachable only through the proxy.
- Proxy serves HTTPS, redirects HTTP, compresses, and preserves `Host`.
- Exactly one instance, with a restart policy.
- Logs collected from standard error.

## Next steps

- [Deployment](/docs/deployment/) is the reference version of this tutorial, with more proxy and hosting options.
- [Security](/docs/security/) lists every default you relied on here.
- [Sessions](/docs/sessions/) explains the session store and its limits.
- [Troubleshooting](/docs/troubleshooting/) covers the common `403` and sign-in problems in more depth.

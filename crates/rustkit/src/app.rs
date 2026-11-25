//! Turns a compiled manifest plus application actions into an HTTP service.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::Path as UrlPath;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::{get, post};

use crate::context::RequestContext;
use crate::error::Error;
use crate::js::escape;
use crate::session::SessionStore;

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A rendered page plus the timings the renderer measured.
pub struct Page {
    pub html: String,
    /// Wall time spent waiting on resolvers (mostly upstream latency).
    pub sources_time: Duration,
    /// Time spent producing HTML once the data was available.
    pub render_time: Duration,
}

pub type RenderFn<S> = for<'a> fn(&'a RequestContext<S>, Option<&'a str>) -> BoxFut<'a, Result<Page, Error>>;
pub type SourceApiFn<S> = for<'a> fn(&'a str, &'a RequestContext<S>, &'a [(&'a str, &'a str)]) -> Option<BoxFut<'a, Result<serde_json::Value, Error>>>;

/// One compiled route.
pub struct Route<S> {
    pub path: &'static str,
    /// The single dynamic segment's name, when `path` contains `{name}`.
    pub param: Option<&'static str>,
    pub prerender: bool,
    pub render: RenderFn<S>,
    /// Browser JavaScript this route loads (runtime plus page module), in bytes.
    pub js_bytes: usize,
    pub css_bytes: usize,
}

pub struct Asset {
    pub path: &'static str,
    pub content_type: &'static str,
    pub body: &'static str,
}

/// Everything the compiler generated for an application.
pub struct Manifest<S> {
    pub routes: Vec<Route<S>>,
    pub assets: Vec<Asset>,
    /// Sources some page loads in the browser. Only these get an HTTP endpoint.
    pub client_sources: &'static [&'static str],
    pub source_api: SourceApiFn<S>,
}

/// Submitted form fields.
#[derive(Debug, Default, Clone)]
pub struct Form(pub HashMap<String, String>);

impl Form {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// What an action does once it succeeds: redirect with 303 See Other.
pub struct Redirect(pub String);

impl Redirect {
    pub fn to(path: impl Into<String>) -> Self {
        Redirect(path.into())
    }
}

type ActionFn<S> = Arc<dyn Fn(RequestContext<S>, Form) -> BoxFut<'static, Result<Redirect, Error>> + Send + Sync>;

pub struct App<S> {
    manifest: Manifest<S>,
    state: Arc<S>,
    actions: Vec<(String, ActionFn<S>)>,
    sessions: SessionStore,
    secure_cookies: bool,
    login_path: String,
}

struct Shared<S> {
    state: Arc<S>,
    sessions: SessionStore,
    secure_cookies: bool,
    login_path: String,
    source_api: SourceApiFn<S>,
}

impl<S> Shared<S> {
    fn context(&self, headers: &HeaderMap) -> RequestContext<S> {
        let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
        RequestContext::new(self.state.clone(), self.sessions.clone(), cookie, self.secure_cookies)
    }
}

/// Summary of a static build.
pub struct BuildReport {
    pub files: Vec<String>,
}

impl<S: Send + Sync + 'static> App<S> {
    pub fn new(manifest: Manifest<S>, state: S) -> Self {
        Self {
            manifest,
            state: Arc::new(state),
            actions: Vec::new(),
            sessions: match crate::dev::session_file() {
                Some(path) => SessionStore::persistent(path),
                None => SessionStore::default(),
            },
            secure_cookies: true,
            login_path: "/login".into(),
        }
    }

    /// Whether session cookies carry `Secure`. Leave on outside local development.
    pub fn secure_cookies(mut self, on: bool) -> Self {
        self.secure_cookies = on;
        self
    }

    pub fn login_path(mut self, path: impl Into<String>) -> Self {
        self.login_path = path.into();
        self
    }

    /// Registers a form action. The framework checks the request's origin and
    /// CSRF token before the action runs.
    pub fn action<F, Fut>(mut self, path: &str, f: F) -> Self
    where
        F: Fn(RequestContext<S>, Form) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Redirect, Error>> + Send + 'static,
    {
        self.actions.push((path.to_owned(), Arc::new(move |ctx, form| Box::pin(f(ctx, form)))));
        self
    }

    pub fn routes(&self) -> &[Route<S>] {
        &self.manifest.routes
    }

    /// A handle on the application state, e.g. for tests or background tasks.
    pub fn state(&self) -> Arc<S> {
        self.state.clone()
    }

    /// Renders every prerendered route with a request-free context.
    pub async fn prerender(&self) -> Result<Vec<(&'static str, String)>, Error> {
        let mut out = Vec::new();
        for r in self.manifest.routes.iter().filter(|r| r.prerender) {
            if r.param.is_some() {
                return Err(Error::Internal(format!("dynamic route {} cannot be prerendered", r.path)));
            }
            let ctx = RequestContext::for_build(self.state.clone());
            let page = (r.render)(&ctx, None).await?;
            out.push((r.path, page.html));
        }
        Ok(out)
    }

    /// Writes prerendered routes and every asset to `dir`.
    pub async fn build(&self, dir: &Path) -> std::io::Result<BuildReport> {
        let mut files = Vec::new();
        let pages = self.prerender().await.map_err(std::io::Error::other)?;
        for (path, html) in pages {
            let rel = if path == "/" { "index.html".to_owned() } else { format!("{}/index.html", &path[1..]) };
            let file = dir.join(&rel);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, html)?;
            files.push(rel);
        }
        for a in &self.manifest.assets {
            let rel = a.path.trim_start_matches('/');
            let file = dir.join(rel);
            std::fs::create_dir_all(file.parent().unwrap())?;
            std::fs::write(&file, a.body)?;
            files.push(rel.to_owned());
        }
        Ok(BuildReport { files })
    }

    /// Builds the router. Prerendered routes are rendered once, here.
    pub async fn into_router(self) -> Result<Router, Error> {
        let prerendered: HashMap<&str, String> = self.prerender().await?.into_iter().collect();
        let shared = Arc::new(Shared {
            state: self.state,
            sessions: self.sessions,
            secure_cookies: self.secure_cookies,
            login_path: self.login_path,
            source_api: self.manifest.source_api,
        });
        let mut router = Router::new();

        for route in self.manifest.routes {
            if let Some(html) = prerendered.get(route.path) {
                let html = html.clone();
                router = router.route(
                    route.path,
                    get(move || async move {
                        Response::builder()
                            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                            // In development a reload must show the new build.
                            .header(header::CACHE_CONTROL, if crate::dev::enabled() { "no-cache" } else { "public, max-age=300" })
                            .body(Body::from(html))
                            .unwrap()
                    }),
                );
                continue;
            }
            let shared = shared.clone();
            let render = route.render;
            let path = route.path;
            let param = route.param;
            if param.is_some() {
                router = router.route(
                    path,
                    get(move |UrlPath(value): UrlPath<String>, headers: HeaderMap| async move {
                        let ctx = shared.context(&headers);
                        let next = request_path(path, param, Some(&value));
                        match render(&ctx, Some(&value)).await {
                            Ok(page) => html_response(page, &ctx),
                            Err(Error::Unauthorized) => redirect(&format!("{}?next={next}", shared.login_path), ctx.take_set_cookie()),
                            Err(e) => error_page(&e),
                        }
                    }),
                );
                continue;
            }
            router = router.route(
                path,
                get(move |headers: HeaderMap| async move {
                    let ctx = shared.context(&headers);
                    match render(&ctx, None).await {
                        Ok(page) => html_response(page, &ctx),
                        Err(Error::Unauthorized) => redirect(&format!("{}?next={path}", shared.login_path), ctx.take_set_cookie()),
                        Err(e) => error_page(&e),
                    }
                }),
            );
        }

        if !self.manifest.client_sources.is_empty() {
            let shared = shared.clone();
            router = router.route(
                "/_rk/source/{name}",
                get(
                    move |UrlPath(name): UrlPath<String>, headers: HeaderMap, axum::extract::RawQuery(query): axum::extract::RawQuery| async move {
                        source_endpoint(&shared, &name, &headers, query.as_deref()).await
                    },
                ),
            );
        }

        for (path, action) in self.actions {
            let shared = shared.clone();
            router = router.route(
                &path,
                post(move |headers: HeaderMap, axum::extract::Form(fields): axum::extract::Form<HashMap<String, String>>| {
                    let action = action.clone();
                    async move {
                        if !same_origin(&headers) {
                            return error_page(&Error::Forbidden);
                        }
                        let ctx = shared.context(&headers);
                        if !ctx.check_csrf(fields.get("_csrf").map(String::as_str)) {
                            return error_page(&Error::BadRequest("This form has expired. Reload the page and try again.".into()));
                        }
                        match action(ctx.clone(), Form(fields)).await {
                            Ok(Redirect(to)) => redirect(&to, ctx.take_set_cookie()),
                            Err(Error::Unauthorized) => redirect(&shared.login_path, ctx.take_set_cookie()),
                            Err(e) => error_page(&e),
                        }
                    }
                }),
            );
        }

        for asset in self.manifest.assets {
            router = router.route(
                asset.path,
                get(move || async move {
                    Response::builder()
                        .header(header::CONTENT_TYPE, asset.content_type)
                        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                        .body(Body::from(asset.body))
                        .unwrap()
                }),
            );
        }

        if crate::dev::enabled() {
            router = crate::dev::install(router);
        }
        Ok(router.fallback(|| async { error_page(&Error::NotFound) }))
    }
}

async fn source_endpoint<S>(shared: &Shared<S>, name: &str, headers: &HeaderMap, query: Option<&str>) -> Response<Body> {
    // Browsers label cross-site requests; refuse them outright.
    if headers.get("sec-fetch-site").is_some_and(|v| v == "cross-site") {
        return json_error(&Error::Forbidden);
    }
    let ctx = shared.context(headers);
    let owned = query_pairs(query.unwrap_or(""));
    let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let Some(fut) = (shared.source_api)(name, &ctx, &pairs) else {
        return json_error(&Error::NotFound);
    };
    match fut.await {
        Ok(v) => {
            let res = Response::builder().header(header::CONTENT_TYPE, "application/json");
            cache_headers(res, true).body(Body::from(v.to_string())).unwrap()
        }
        Err(e) => json_error(&e),
    }
}

fn html_response<S>(page: Page, ctx: &RequestContext<S>) -> Response<Body> {
    let timing = format!("sources;dur={:.3}, render;dur={:.3}", page.sources_time.as_secs_f64() * 1000.0, page.render_time.as_secs_f64() * 1000.0);
    let mut res = Response::builder().header(header::CONTENT_TYPE, "text/html; charset=utf-8").header("server-timing", timing);
    res = cache_headers(res, ctx.is_personalised());
    if let Some(c) = ctx.take_set_cookie() {
        res = res.header(header::SET_COOKIE, c);
    }
    res.body(Body::from(page.html)).unwrap()
}

/// Replaces `{name}` in an axum pattern with the captured segment, percent-encoded.
fn request_path(pattern: &str, param: Option<&str>, value: Option<&str>) -> String {
    match (param, value) {
        (Some(name), Some(value)) => pattern.replacen(&format!("{{{name}}}"), &encode_segment(value), 1),
        _ => pattern.to_owned(),
    }
}

fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn query_pairs(q: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.push((percent_decode(k), percent_decode(v)));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn cache_headers(res: axum::http::response::Builder, personalised: bool) -> axum::http::response::Builder {
    if personalised {
        res.header(header::CACHE_CONTROL, "private, no-store").header(header::VARY, "Cookie")
    } else {
        res.header(header::CACHE_CONTROL, "no-cache")
    }
}

/// Accepts requests whose `Origin` (when sent) matches the `Host` they were sent to.
fn same_origin(headers: &HeaderMap) -> bool {
    if headers.get("sec-fetch-site").is_some_and(|v| v == "cross-site") {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    origin == format!("http://{host}") || origin == format!("https://{host}")
}

fn redirect(to: &str, cookie: Option<String>) -> Response<Body> {
    let mut res = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, HeaderValue::from_str(to).unwrap_or(HeaderValue::from_static("/")))
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(c) = cookie {
        res = res.header(header::SET_COOKIE, c);
    }
    res.body(Body::empty()).unwrap()
}

fn error_page(e: &Error) -> Response<Body> {
    if matches!(e, Error::Upstream(_) | Error::Internal(_)) {
        eprintln!("rustkit: {e}");
    }
    let status = StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{code}</title>{dev}</head>\
         <body><h1>{code}</h1><p>{msg}</p><p><a href=\"javascript:history.back()\">Go back</a></p></body></html>",
        code = status.as_u16(),
        msg = escape(e.public_message()),
        dev = crate::dev::script_tag(),
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(html))
        .unwrap()
}

fn json_error(e: &Error) -> Response<Body> {
    if matches!(e, Error::Upstream(_) | Error::Internal(_)) {
        eprintln!("rustkit: {e}");
    }
    let body = serde_json::json!({ "error": e.public_message() }).to_string();
    Response::builder()
        .status(StatusCode::from_u16(e.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap()
}

/// The HTML document around a page body. Called by generated code.
///
/// `css` lists stylesheet URLs in cascade order: app-wide sheets (such as
/// Tailwind) first, then the page's own `<style>` block.
pub fn document(title: &str, css: &[&str], body: &str, payload: Option<&str>, js: Option<&str>) -> String {
    let mut out = String::with_capacity(body.len() + 512);
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>");
    crate::js::escape_into(&mut out, title);
    out.push_str("</title>\n");
    out.push_str(&crate::dev::script_tag());
    for css in css {
        out.push_str(&format!("<link rel=\"stylesheet\" href=\"{css}\">\n"));
    }
    if let Some(js) = js {
        out.push_str(&format!("<link rel=\"modulepreload\" href=\"{js}\">\n"));
    }
    out.push_str("</head>\n<body>\n");
    out.push_str(body);
    if let Some(p) = payload {
        out.push_str("\n<script type=\"application/json\" id=\"rk-payload\">");
        out.push_str(p);
        out.push_str("</script>");
    }
    if let Some(js) = js {
        out.push_str(&format!("\n<script type=\"module\" src=\"{js}\"></script>"));
    }
    out.push_str("\n</body>\n</html>\n");
    out
}

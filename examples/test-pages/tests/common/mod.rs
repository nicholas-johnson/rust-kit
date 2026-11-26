//! Helpers shared by the test files: requests against the real router, and
//! small HTML extraction utilities.
#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

pub struct Res {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Res {
    pub fn header(&self, name: &str) -> &str {
        self.headers.get(name).map_or("", |v| v.to_str().unwrap())
    }

    /// `name=value` from `Set-Cookie`, ready for a `Cookie` header.
    pub fn cookie(&self) -> Option<String> {
        let c = self.headers.get(header::SET_COOKIE)?.to_str().unwrap();
        Some(c.split(';').next().unwrap().to_owned())
    }

    pub fn csrf(&self) -> String {
        let marker = "name=\"_csrf\" value=\"";
        let start = self.body.find(marker).expect("page has a CSRF field") + marker.len();
        self.body[start..start + 64].to_owned()
    }

    /// The JSON handoff payload, if the page has one.
    pub fn payload(&self) -> Option<serde_json::Value> {
        let p = self.body.split("id=\"rk-payload\">").nth(1)?.split("</script>").next()?;
        Some(serde_json::from_str(p).unwrap())
    }

    /// The page module's URL, if the page has one.
    pub fn module_url(&self) -> Option<String> {
        let rest = self.body.split("<script type=\"module\" src=\"").nth(1)?;
        Some(rest.split('"').next()?.to_owned())
    }

    /// Inner HTML of the element with `id`.
    pub fn inner(&self, id: &str) -> String {
        inner_html(&self.body, id).unwrap_or_else(|| panic!("no element with id {id} in:\n{}", self.body))
    }

    /// The opening tag of the element with `id`.
    pub fn tag(&self, id: &str) -> String {
        let at = self.body.find(&format!("id=\"{id}\"")).unwrap_or_else(|| panic!("no element with id {id}"));
        let start = self.body[..at].rfind('<').unwrap();
        let end = at + self.body[at..].find('>').unwrap();
        self.body[start..=end].to_owned()
    }
}

/// Removes ` data-rk="…"` so a test about a tag can ignore scoping.
pub fn strip_scope(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let needle = " data-rk=\"";
    while let Some(at) = rest.find(needle) {
        out.push_str(&rest[..at]);
        let value = at + needle.len();
        let end = value + rest[value..].find('"').expect("unclosed data-rk") + 1;
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Removes `<!--rk:…-->` block markers, leaving what a reader sees.
pub fn strip_markers(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("<!--") {
        let end = at + rest[at..].find("-->").unwrap() + 3;
        out.push_str(&rest[..at]);
        let c = &rest[at + 4..end - 3];
        if !(c.starts_with("rk:") || c.starts_with("/rk:")) {
            out.push_str(&rest[at..end]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub fn inner_html(html: &str, id: &str) -> Option<String> {
    let at = html.find(&format!("id=\"{id}\""))?;
    let open = html[..at].rfind('<')?;
    let name: String = html[open + 1..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let start = at + html[at..].find('>')? + 1;
    // Find the matching close tag, allowing nested elements of the same name.
    let (open_tag, close_tag) = (format!("<{name}"), format!("</{name}>"));
    let mut depth = 1;
    let mut i = start;
    while depth > 0 {
        let next_close = i + html[i..].find(&close_tag)?;
        match html[i..next_close].find(&open_tag) {
            Some(o) if is_tag_boundary(&html[i + o + open_tag.len()..]) => {
                depth += 1;
                i = i + o + open_tag.len();
            }
            Some(o) => i = i + o + open_tag.len(),
            None => {
                depth -= 1;
                if depth == 0 {
                    return Some(html[start..next_close].to_owned());
                }
                i = next_close + close_tag.len();
            }
        }
    }
    None
}

fn is_tag_boundary(rest: &str) -> bool {
    rest.starts_with([' ', '>', '/', '\n'])
}

pub async fn send(router: &Router, req: Request<Body>) -> Res {
    let res = router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    Res { status, headers, body }
}

/// Replaces each `{param}` in a route pattern with `hello`, which is a legal URI.
pub fn located(pattern: &str) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        let end = rest[start + 1..].find('}').expect("param brace");
        out.push_str(&rest[..start]);
        out.push_str("hello");
        rest = &rest[start + 1 + end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn get(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(path).header(header::HOST, "localhost");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::empty()).unwrap()
}

pub fn post(path: &str, cookie: Option<&str>, form: &str) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::HOST, "localhost")
        .header(header::ORIGIN, "http://localhost")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    b.body(Body::from(form.to_owned())).unwrap()
}

pub async fn router() -> Router {
    test_pages::app(false).into_router().await.unwrap()
}

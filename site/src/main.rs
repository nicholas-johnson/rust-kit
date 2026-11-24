//! Builds rust-kit.com.
//!
//! `cargo run -p site -- build [out]` renders `site/content/**/*.md` and the
//! landing page into static files. Every ```rk code block is compiled with the
//! real Rust-kit compiler; a sample that fails to compile fails the build
//! (unless marked `fail`, in which case it must fail and its diagnostic is
//! shown). Internal links and anchors are checked too.

mod highlight;
mod landing;
mod markdown;
mod templates;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const SITE_URL: &str = "https://rust-kit.com";
pub const REPO_URL: &str = "https://github.com/nicholas-johnson/rust-kit";

/// Doc sections, in sidebar order. A doc's `section:` must be one of these.
pub const DOC_SECTIONS: &[&str] = &["Start here", "Core concepts", "Data", "Server", "Guides", "Reference", "Project"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Docs,
    Tutorials,
}

impl Kind {
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Docs => "docs",
            Kind::Tutorials => "tutorials",
        }
    }
}

pub struct Page {
    pub kind: Kind,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub section: String,
    pub order: i32,
    pub level: Option<String>,
    pub duration: Option<String>,
    pub html: String,
    pub toc: Vec<(u8, String, String)>,
    pub search: Vec<SearchEntry>,
    pub source: PathBuf,
}

impl Page {
    pub fn url(&self) -> String {
        format!("/{}/{}/", self.kind.dir(), self.slug)
    }
}

pub struct SearchEntry {
    pub heading: String,
    pub anchor: String,
    pub text: String,
}

/// 64-bit FNV-1a, for cache-busting asset names.
pub fn hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:08x}", h >> 32)
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn front_matter(src: &str, file: &Path) -> (BTreeMap<String, String>, String) {
    let Some(rest) = src.strip_prefix("---\n") else {
        panic!("{}: missing front matter", file.display());
    };
    let end = rest.find("\n---\n").unwrap_or_else(|| panic!("{}: unterminated front matter", file.display()));
    let mut map = BTreeMap::new();
    for line in rest[..end].lines() {
        if let Some((k, v)) = line.split_once(':') {
            map.insert(k.trim().to_owned(), v.trim().trim_matches('"').to_owned());
        }
    }
    (map, rest[end + 5..].to_owned())
}

fn load(kind: Kind, root: &Path, errors: &mut Vec<String>) -> Vec<Page> {
    let dir = root.join("content").join(kind.dir());
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    let mut pages = Vec::new();
    for file in files {
        let src = fs::read_to_string(&file).unwrap();
        let (fm, body) = front_matter(&src, &file);
        let get = |k: &str| fm.get(k).cloned();
        let title = get("title").unwrap_or_else(|| panic!("{}: missing title", file.display()));
        let section = get("section").unwrap_or_default();
        if kind == Kind::Docs && !DOC_SECTIONS.contains(&section.as_str()) {
            errors.push(format!("{}: section `{section}` is not one of {DOC_SECTIONS:?}", file.display()));
        }
        let rendered = markdown::render(&body, &file, errors);
        pages.push(Page {
            kind,
            slug: file.file_stem().unwrap().to_string_lossy().into_owned(),
            title,
            description: get("description").unwrap_or_default(),
            section,
            order: get("order").and_then(|o| o.parse().ok()).unwrap_or(100),
            level: get("level"),
            duration: get("duration"),
            html: rendered.html,
            toc: rendered.toc,
            search: rendered.search,
            source: file,
        });
    }
    pages.sort_by(|a, b| {
        let sec = |p: &Page| DOC_SECTIONS.iter().position(|s| *s == p.section).unwrap_or(usize::MAX);
        (sec(a), a.order, &a.title).cmp(&(sec(b), b.order, &b.title))
    });
    pages
}

fn write(out: &Path, rel: &str, body: &str) {
    let path = out.join(rel.trim_start_matches('/'));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// Checks every root-relative `href` and its `#anchor` against the built pages.
fn check_links(pages: &BTreeMap<String, String>, assets: &BTreeSet<String>) -> Vec<String> {
    let mut ids: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (url, html) in pages {
        let set = ids.entry(url.as_str()).or_default();
        for part in html.split(" id=\"").skip(1) {
            set.insert(part.split('"').next().unwrap().to_owned());
        }
    }
    let mut errors = Vec::new();
    for (url, html) in pages {
        for part in html.split("href=\"").skip(1) {
            let href = part.split('"').next().unwrap();
            let (path, anchor) = match href.split_once('#') {
                Some((p, a)) => (if p.is_empty() { url.as_str() } else { p }, Some(a)),
                None => (href, None),
            };
            if !path.starts_with('/') || path.starts_with("//") {
                continue;
            }
            if assets.contains(path) {
                continue;
            }
            match ids.get(path) {
                None => errors.push(format!("{url}: broken link to {href}")),
                Some(set) => {
                    if let Some(a) = anchor.filter(|a| !a.is_empty() && !set.contains(*a)) {
                        errors.push(format!("{url}: link to missing anchor {path}#{a}"));
                    }
                }
            }
        }
    }
    errors
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let allow_broken = args.iter().any(|a| a == "--allow-broken-links");
    args.retain(|a| a != "--allow-broken-links");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = match args.as_slice() {
        [cmd] if cmd == "build" => root.join("dist"),
        [cmd, dir] if cmd == "build" => PathBuf::from(dir),
        _ => {
            eprintln!("usage: site build [out-dir] [--allow-broken-links]");
            std::process::exit(2);
        }
    };

    let mut errors = Vec::new();
    let docs = load(Kind::Docs, &root, &mut errors);
    let tutorials = load(Kind::Tutorials, &root, &mut errors);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}\n");
        }
        eprintln!("site build failed with {} error(s)", errors.len());
        std::process::exit(1);
    }

    if out.exists() {
        fs::remove_dir_all(&out).unwrap();
    }
    fs::create_dir_all(&out).unwrap();

    // Assets with content hashes.
    let css = fs::read_to_string(root.join("static/style.css")).unwrap();
    let js = fs::read_to_string(root.join("static/site.js")).unwrap();
    let css_url = format!("/assets/style.{}.css", hash(&css));
    let js_url = format!("/assets/site.{}.js", hash(&js));
    write(&out, &css_url, &css);
    write(&out, &js_url, &js);
    let mut assets: BTreeSet<String> = [css_url.clone(), js_url.clone()].into();
    for name in ["logo.svg", "logo-mark.svg", "favicon.svg", "social.svg"] {
        let body = fs::read_to_string(root.join("static").join(name)).unwrap();
        write(&out, &format!("/{name}"), &body);
        assets.insert(format!("/{name}"));
    }
    let ctx = templates::Ctx { css: &css_url, js: &js_url, docs: &docs, tutorials: &tutorials };

    let mut built: BTreeMap<String, String> = BTreeMap::new();
    let mut emit = |url: &str, html: String| {
        let rel = if url.ends_with('/') { format!("{url}index.html") } else { url.to_owned() };
        write(&out, &rel, &html);
        built.insert(url.to_owned(), html);
    };

    emit("/", landing::render(&ctx, &root));
    emit("/docs/", templates::docs_index(&ctx));
    emit("/tutorials/", templates::tutorials_index(&ctx));
    for (list, kind) in [(&docs, Kind::Docs), (&tutorials, Kind::Tutorials)] {
        for (i, p) in list.iter().enumerate() {
            let prev = i.checked_sub(1).map(|j| &list[j]);
            let next = list.get(i + 1);
            emit(&p.url(), templates::article(&ctx, p, prev, next, kind));
        }
    }
    let not_found = templates::not_found(&ctx);
    write(&out, "/404.html", &not_found);

    write(&out, "/search.json", &templates::search_index(&ctx));
    write(&out, "/CNAME", "rust-kit.com\n");
    write(&out, "/.nojekyll", "");
    write(&out, "/robots.txt", &format!("User-agent: *\nAllow: /\nSitemap: {SITE_URL}/sitemap.xml\n"));
    let mut sitemap = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for url in built.keys() {
        sitemap.push_str(&format!("  <url><loc>{SITE_URL}{url}</loc></url>\n"));
    }
    sitemap.push_str("</urlset>\n");
    write(&out, "/sitemap.xml", &sitemap);
    assets.extend(["/search.json", "/sitemap.xml", "/robots.txt"].map(String::from));

    let link_errors = check_links(&built, &assets);
    if !link_errors.is_empty() {
        for e in &link_errors {
            eprintln!("{e}");
        }
        if allow_broken {
            eprintln!("warning: {} broken link(s) allowed", link_errors.len());
        } else {
            eprintln!("site build failed: {} broken link(s)", link_errors.len());
            std::process::exit(1);
        }
    }
    let samples = markdown::SAMPLES.load(std::sync::atomic::Ordering::Relaxed);
    println!(
        "built {} pages ({} docs, {} tutorials), compiled {samples} rk samples, into {}",
        built.len() + 1,
        docs.len(),
        tutorials.len(),
        out.display()
    );
}

//! Page chrome shared by every route.

use crate::{DOC_SECTIONS, Kind, Page, REPO_URL, SITE_URL, esc};

pub struct Ctx<'a> {
    pub css: &'a str,
    pub js: &'a str,
    pub docs: &'a [Page],
    pub tutorials: &'a [Page],
}

pub const MARK_SVG: &str = r##"<svg class="mark" viewBox="0 0 64 64" aria-hidden="true"><defs><linearGradient id="mk-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff8a3d"/><stop offset="1" stop-color="#c2410c"/></linearGradient></defs><path d="M32 3.5 56.7 17.75v28.5L32 60.5 7.3 46.25v-28.5Z" fill="url(#mk-g)" stroke="url(#mk-g)" stroke-width="5" stroke-linejoin="round"/><path d="M32 7.5 53.3 19.8v24.4L32 56.5 10.7 44.2V19.8Z" fill="none" stroke="#fff" stroke-opacity=".16" stroke-width="1.2" stroke-linejoin="round"/><rect x="18.5" y="16" width="7.5" height="32" rx="2.2" fill="#fff"/><path d="M46 18.5 31.5 32 46 45.5" fill="none" stroke="#0f766e" stroke-width="8" stroke-linecap="round" stroke-linejoin="round" opacity=".55" transform="translate(.6 1.2)"/><path d="M46 18.5 31.5 32 46 45.5" fill="none" stroke="#2dd4bf" stroke-width="7.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

const GITHUB_SVG: &str = r#"<svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>"#;

pub fn head(ctx: &Ctx, title: &str, description: &str, path: &str) -> String {
    let full = if title.is_empty() { "Rust-kit".to_owned() } else { format!("{title} · Rust-kit") };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t}</title>
<meta name="description" content="{d}">
<link rel="canonical" href="{SITE_URL}{path}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Rust-kit">
<meta property="og:title" content="{t}">
<meta property="og:description" content="{d}">
<meta property="og:url" content="{SITE_URL}{path}">
<meta name="twitter:card" content="summary">
<meta name="theme-color" content="#14110f">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="{css}">
<script>document.documentElement.dataset.theme=localStorage.getItem("rk-theme")||(matchMedia("(prefers-color-scheme: light)").matches?"light":"dark")</script>
<script src="{js}" defer></script>
</head>
"##,
        t = esc(&full),
        d = esc(description),
        css = ctx.css,
        js = ctx.js,
    )
}

pub fn header(active: &str) -> String {
    let link = |href: &str, label: &str, key: &str| {
        let cur = if key == active { " aria-current=\"page\"" } else { "" };
        format!("<a href=\"{href}\"{cur}>{label}</a>")
    };
    format!(
        r##"<a class="skip" href="#main">Skip to content</a>
<header class="topbar">
  <div class="topbar-inner">
    <label class="nav-toggle" for="nav-open" aria-label="Open navigation"><span></span><span></span><span></span></label>
    <a class="brand" href="/" aria-label="Rust-kit home">{MARK_SVG}<span class="wordmark">rust<span>-kit</span></span></a>
    <nav class="primary" aria-label="Primary">{}{}{}</nav>
    <div class="actions">
      <button class="search-trigger" type="button" data-search-open aria-label="Search"><svg viewBox="0 0 20 20" width="16" height="16" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.5" fill="none" stroke="currentColor" stroke-width="2"/><path d="m13 13 4.5 4.5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg><span>Search</span><kbd>⌘K</kbd></button>
      <button class="theme-toggle" type="button" data-theme-toggle aria-label="Toggle colour theme"><svg class="sun" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><circle cx="12" cy="12" r="4.5" fill="currentColor"/><g stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></g></svg><svg class="moon" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5Z"/></svg></button>
      <a class="gh" href="{REPO_URL}" aria-label="Rust-kit on GitHub">{GITHUB_SVG}</a>
    </div>
  </div>
</header>
"##,
        link("/docs/", "Docs", "docs"),
        link("/tutorials/", "Tutorials", "tutorials"),
        link("/docs/design/", "Design", "design"),
    )
}

pub fn footer() -> String {
    format!(
        r##"<footer class="site-footer">
  <div class="footer-inner">
    <div class="footer-brand"><a class="brand" href="/">{MARK_SVG}<span class="wordmark">rust<span>-kit</span></span></a>
      <p>Rust renders. The browser responds.</p></div>
    <div class="footer-cols">
      <div><h2>Learn</h2><a href="/docs/getting-started/">Getting started</a><a href="/tutorials/">Tutorials</a><a href="/docs/">Documentation</a></div>
      <div><h2>Reference</h2><a href="/docs/template-syntax/">Template syntax</a><a href="/docs/diagnostics/">Diagnostics</a><a href="/docs/runtime-api/">Runtime API</a></div>
      <div><h2>Project</h2><a href="{REPO_URL}">GitHub</a><a href="/docs/roadmap/">Roadmap</a><a href="/docs/design/">Design proposal</a></div>
    </div>
  </div>
  <p class="fineprint">Rust-kit is an independent open-source project. It is not affiliated with or endorsed by the Rust Foundation. Licensed MIT OR Apache-2.0.</p>
</footer>
<dialog class="search" id="search" aria-label="Search the docs">
  <form method="dialog" class="search-box">
    <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.5" fill="none" stroke="currentColor" stroke-width="2"/><path d="m13 13 4.5 4.5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>
    <input type="search" placeholder="Search docs and tutorials" autocomplete="off" spellcheck="false" aria-label="Search" data-search-input>
    <button type="submit" class="esc" aria-label="Close">Esc</button>
  </form>
  <ol class="search-results" data-search-results></ol>
  <p class="search-empty" data-search-empty hidden>No results. Try a shorter word.</p>
</dialog>
"##
    )
}

fn sidebar(ctx: &Ctx, kind: Kind, current: &str) -> String {
    let mut s = String::from("<nav class=\"sidebar\" aria-label=\"Section\"><div class=\"sidebar-inner\">");
    s.push_str("<div class=\"side-switch\"><a href=\"/docs/\" class=\"");
    s.push_str(if kind == Kind::Docs { "on" } else { "" });
    s.push_str("\">Docs</a><a href=\"/tutorials/\" class=\"");
    s.push_str(if kind == Kind::Tutorials { "on" } else { "" });
    s.push_str("\">Tutorials</a></div>");
    let item = |p: &Page, prefix: String| {
        let cur = if p.url() == current { " aria-current=\"page\"" } else { "" };
        format!("<li><a href=\"{}\"{cur}>{prefix}{}</a></li>", p.url(), esc(&p.title))
    };
    match kind {
        Kind::Docs => {
            for sec in DOC_SECTIONS {
                let items: Vec<&Page> = ctx.docs.iter().filter(|p| p.section == *sec).collect();
                if items.is_empty() {
                    continue;
                }
                s.push_str(&format!("<h2>{sec}</h2><ul>"));
                for p in items {
                    s.push_str(&item(p, String::new()));
                }
                s.push_str("</ul>");
            }
        }
        Kind::Tutorials => {
            s.push_str("<h2>Tutorials</h2><ol class=\"numbered\">");
            for (i, p) in ctx.tutorials.iter().enumerate() {
                s.push_str(&item(p, format!("<span class=\"num\">{:02}</span>", i + 1)));
            }
            s.push_str("</ol>");
        }
    }
    s.push_str("</div></nav>");
    s
}

fn toc(p: &Page) -> String {
    if p.toc.len() < 2 {
        return "<aside class=\"toc\"></aside>".into();
    }
    let mut s = String::from("<aside class=\"toc\"><div class=\"toc-inner\"><h2>On this page</h2><ul>");
    for (lvl, id, text) in &p.toc {
        s.push_str(&format!("<li class=\"l{lvl}\"><a href=\"#{id}\">{}</a></li>", esc(text)));
    }
    s.push_str("</ul></div></aside>");
    s
}

pub fn article(ctx: &Ctx, p: &Page, prev: Option<&Page>, next: Option<&Page>, kind: Kind) -> String {
    let mut s = head(ctx, &p.title, &p.description, &p.url());
    let active = if p.slug == "design" { "design" } else { kind.dir() };
    s.push_str(&format!("<body class=\"page-{}\">\n<input type=\"checkbox\" id=\"nav-open\" class=\"nav-open\" hidden>\n", kind.dir()));
    s.push_str(&header(active));
    s.push_str("<div class=\"shell\">");
    s.push_str(&sidebar(ctx, kind, &p.url()));
    s.push_str("<main id=\"main\"><article class=\"prose\">");
    let crumb = match kind {
        Kind::Docs => format!("<a href=\"/docs/\">Docs</a><span>/</span>{}", esc(&p.section)),
        Kind::Tutorials => "<a href=\"/tutorials/\">Tutorials</a>".to_owned(),
    };
    s.push_str(&format!("<p class=\"crumbs\">{crumb}</p><h1>{}</h1>", esc(&p.title)));
    if !p.description.is_empty() {
        s.push_str(&format!("<p class=\"lead\">{}</p>", esc(&p.description)));
    }
    if kind == Kind::Tutorials {
        s.push_str("<p class=\"meta\">");
        if let Some(l) = &p.level {
            s.push_str(&format!("<span class=\"chip level\">{}</span>", esc(l)));
        }
        if let Some(d) = &p.duration {
            s.push_str(&format!("<span class=\"chip\">{}</span>", esc(d)));
        }
        s.push_str("</p>");
    }
    s.push_str(&p.html);
    s.push_str("</article><nav class=\"pager\" aria-label=\"Pagination\">");
    if let Some(pv) = prev {
        s.push_str(&format!("<a class=\"prev\" href=\"{}\"><small>Previous</small>{}</a>", pv.url(), esc(&pv.title)));
    } else {
        s.push_str("<span></span>");
    }
    if let Some(nx) = next {
        s.push_str(&format!("<a class=\"next\" href=\"{}\"><small>Next</small>{}</a>", nx.url(), esc(&nx.title)));
    }
    s.push_str("</nav>");
    let rel = p.source.strip_prefix(env!("CARGO_MANIFEST_DIR")).map_or_else(|_| p.source.display().to_string(), |r| r.display().to_string());
    s.push_str(&format!(
        "<p class=\"edit\"><a href=\"{REPO_URL}/blob/main/site{rel}\">Edit this page on GitHub</a></p></main>"
    ));
    s.push_str(&toc(p));
    s.push_str("</div>");
    s.push_str(&footer());
    s.push_str("</body>\n</html>\n");
    s
}

fn card(p: &Page, idx: Option<usize>) -> String {
    let num = idx.map_or(String::new(), |i| format!("<span class=\"num\">{:02}</span>", i + 1));
    let meta = match (&p.level, &p.duration) {
        (Some(l), Some(d)) => format!("<span class=\"card-meta\">{} · {}</span>", esc(l), esc(d)),
        _ => String::new(),
    };
    format!(
        "<a class=\"card\" href=\"{}\">{num}<h3>{}</h3><p>{}</p>{meta}</a>",
        p.url(),
        esc(&p.title),
        esc(&p.description)
    )
}

pub fn docs_index(ctx: &Ctx) -> String {
    let mut s = head(ctx, "Documentation", "Everything about Rust-kit: concepts, data sources, actions, sessions, the compiler and the runtime.", "/docs/");
    s.push_str("<body class=\"page-index\">\n<input type=\"checkbox\" id=\"nav-open\" class=\"nav-open\" hidden>\n");
    s.push_str(&header("docs"));
    s.push_str("<main id=\"main\" class=\"index\"><div class=\"index-hero\"><p class=\"eyebrow\">Documentation</p><h1>Learn how Rust-kit thinks</h1><p class=\"lead\">One page file. A native renderer on the server, just enough JavaScript in the browser. Start with the basics or jump to the reference.</p></div>");
    for sec in DOC_SECTIONS {
        let items: Vec<&Page> = ctx.docs.iter().filter(|p| p.section == *sec).collect();
        if items.is_empty() {
            continue;
        }
        s.push_str(&format!("<section class=\"index-section\"><h2 id=\"{}\">{sec}</h2><div class=\"cards\">", crate::markdown::slugify(sec)));
        for p in items {
            s.push_str(&card(p, None));
        }
        s.push_str("</div></section>");
    }
    s.push_str("</main>");
    s.push_str(&footer());
    s.push_str("</body>\n</html>\n");
    s
}

pub fn tutorials_index(ctx: &Ctx) -> String {
    let mut s = head(ctx, "Tutorials", "Step-by-step tutorials: build pages, load data, handle forms, sign users in, test and deploy a Rust-kit app.", "/tutorials/");
    s.push_str("<body class=\"page-index\">\n<input type=\"checkbox\" id=\"nav-open\" class=\"nav-open\" hidden>\n");
    s.push_str(&header("tutorials"));
    s.push_str("<main id=\"main\" class=\"index\"><div class=\"index-hero\"><p class=\"eyebrow\">Tutorials</p><h1>Build it, one step at a time</h1><p class=\"lead\">Each tutorial is a complete, working exercise. Every page sample on these pages is compiled by the real Rust-kit compiler when the site is built.</p></div><section class=\"index-section\"><div class=\"cards tutorials\">");
    for (i, p) in ctx.tutorials.iter().enumerate() {
        s.push_str(&card(p, Some(i)));
    }
    s.push_str("</div></section></main>");
    s.push_str(&footer());
    s.push_str("</body>\n</html>\n");
    s
}

pub fn not_found(ctx: &Ctx) -> String {
    let mut s = head(ctx, "Not found", "This page does not exist.", "/404.html");
    s.push_str("<body class=\"page-index\">\n<input type=\"checkbox\" id=\"nav-open\" class=\"nav-open\" hidden>\n");
    s.push_str(&header(""));
    s.push_str("<main id=\"main\" class=\"index notfound\"><p class=\"eyebrow\">404</p><h1>Nothing rendered here</h1><p class=\"lead\">The server looked, found no route, and sent you this instead. Try the docs or search.</p><p><a class=\"btn primary\" href=\"/docs/\">Read the docs</a> <button class=\"btn\" type=\"button\" data-search-open>Search</button></p></main>");
    s.push_str(&footer());
    s.push_str("</body>\n</html>\n");
    s
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn search_index(ctx: &Ctx) -> String {
    let mut entries = Vec::new();
    for (p, kind) in ctx.docs.iter().map(|p| (p, "Docs")).chain(ctx.tutorials.iter().map(|p| (p, "Tutorial"))) {
        for e in &p.search {
            let url = if e.anchor.is_empty() { p.url() } else { format!("{}#{}", p.url(), e.anchor) };
            let text: String = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
            let heading = if e.heading.is_empty() { p.description.clone() } else { e.heading.clone() };
            entries.push(format!(
                "{{\"t\":{},\"k\":{},\"h\":{},\"u\":{},\"x\":{}}}",
                json_str(&p.title),
                json_str(kind),
                json_str(&heading),
                json_str(&url),
                json_str(&text.chars().take(260).collect::<String>())
            ));
        }
    }
    format!("[{}]", entries.join(",\n"))
}

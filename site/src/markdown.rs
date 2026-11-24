//! Markdown to HTML, with heading anchors, callouts, highlighted code, and
//! compiled `rk` samples.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use rustkit_compiler::{Options as CompileOptions, compile_page};

use crate::highlight::highlight;
use crate::{SearchEntry, esc};

pub static SAMPLES: AtomicUsize = AtomicUsize::new(0);

pub struct Rendered {
    pub html: String,
    pub toc: Vec<(u8, String, String)>,
    pub search: Vec<SearchEntry>,
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// Re-indents generated code by brace depth so it reads well on the page.
pub fn indent(code: &str) -> String {
    let mut depth: i32 = 0;
    let mut out = String::new();
    for line in code.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let mut opens = 0;
        let mut closes_lead = 0;
        let mut seen_other = false;
        let mut quote: Option<char> = None;
        let mut prev = ' ';
        for c in t.chars() {
            match quote {
                Some(q) => {
                    if c == q && prev != '\\' {
                        quote = None;
                    }
                }
                None => match c {
                    '"' | '\'' | '`' => quote = Some(c),
                    '{' | '(' | '[' => {
                        opens += 1;
                        seen_other = true;
                    }
                    '}' | ')' | ']' => {
                        if seen_other {
                            opens -= 1;
                        } else {
                            closes_lead += 1;
                        }
                    }
                    _ => seen_other = true,
                },
            }
            prev = if prev == '\\' && c == '\\' { ' ' } else { c };
        }
        depth = (depth - closes_lead).max(0);
        out.push_str(&"    ".repeat(depth as usize));
        out.push_str(t);
        out.push('\n');
        depth = (depth + opens).max(0);
    }
    out
}

fn figure(lang: &str, title: Option<&str>, src: &str, extra_class: &str) -> String {
    let label = title.map_or_else(|| lang.to_owned(), str::to_owned);
    format!(
        "<figure class=\"code {extra_class}\"><figcaption><span class=\"lang\">{}</span><button class=\"copy\" type=\"button\" aria-label=\"Copy code\">Copy</button></figcaption><pre><code class=\"lang-{lang}\">{}</code></pre></figure>",
        esc(&label),
        highlight(lang, src.trim_end_matches('\n'))
    )
}

fn code_block(info: &str, src: &str, file: &Path, errors: &mut Vec<String>) -> String {
    let mut words = info.split_whitespace();
    let lang = words.next().unwrap_or("text");
    let mut title = None;
    let mut fail = false;
    let mut partial = false;
    for w in words {
        match w {
            "fail" => fail = true,
            "partial" => partial = true,
            w if w.starts_with("title=") => title = Some(w[6..].trim_matches('"')),
            other => errors.push(format!("{}: unknown code block flag `{other}`", file.display())),
        }
    }
    if lang != "rk" || partial {
        let label_class = if partial { "partial" } else { "" };
        return figure(lang, title, src, label_class);
    }

    SAMPLES.fetch_add(1, Ordering::Relaxed);
    let opts = CompileOptions { state_type: "crate::AppState", sources_module: "crate::sources" };
    let name = title.unwrap_or("page.rk");
    let mut html = figure("rk", Some(name), src, "rk-sample");
    match (compile_page(src, &opts), fail) {
        (Ok(page), false) => {
            html.push_str("<div class=\"compiled\">");
            html.push_str(&format!(
                "<details><summary><span class=\"ok\">✓ compiles</span> Generated Rust <small>{} lines</small></summary>{}</details>",
                indent(&page.rust).lines().count(),
                figure("rust", Some("generated · render()"), &indent(&page.rust), "generated")
            ));
            match &page.js {
                Some(js) => {
                    let js = js.replace("__RUNTIME__", "runtime.js");
                    html.push_str(&format!(
                        "<details><summary>Generated JavaScript <small>{} bytes + shared runtime</small></summary>{}</details>",
                        js.len(),
                        figure("js", Some("generated · page module"), &indent(&js), "generated")
                    ));
                }
                None => html.push_str("<p class=\"nojs\"><span class=\"dot\"></span>No JavaScript. This page ships HTML and CSS only.</p>"),
            }
            html.push_str("</div>");
        }
        (Err(d), true) => {
            html.push_str(&format!(
                "<div class=\"compiled\"><p class=\"diag-label\">✗ Build fails, as intended:</p><pre class=\"diag\">{}</pre></div>",
                esc(&d.render(&format!("src/routes/{name}"), src))
            ));
        }
        (Ok(_), true) => errors.push(format!("{}: an `rk fail` sample compiled successfully:\n{src}", file.display())),
        (Err(d), false) => errors.push(format!(
            "{}: an rk sample does not compile:\n{}",
            file.display(),
            d.render(&format!("{} ({name})", file.display()), src)
        )),
    }
    html
}

pub fn render(src: &str, file: &Path, errors: &mut Vec<String>) -> Rendered {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_GFM
        | Options::ENABLE_SMART_PUNCTUATION;
    let parser = Parser::new_ext(src, opts);

    let mut out: Vec<Event> = Vec::new();
    let mut toc = Vec::new();
    let mut search = vec![SearchEntry { heading: String::new(), anchor: String::new(), text: String::new() }];
    let mut ids = BTreeSet::new();

    let mut heading: Option<(HeadingLevel, Option<String>, Vec<Event>, String)> = None;
    let mut code: Option<(String, String)> = None;

    for ev in parser {
        if let Some((_, _, inner, text)) = heading.as_mut() {
            match ev {
                Event::End(TagEnd::Heading(_)) => {
                    let (level, given, inner, text) = heading.take().unwrap();
                    let base = given.unwrap_or_else(|| slugify(&text));
                    let mut id = base.clone();
                    let mut n = 2;
                    while !ids.insert(id.clone()) {
                        id = format!("{base}-{n}");
                        n += 1;
                    }
                    let lvl = level as u8;
                    let mut inner_html = String::new();
                    html::push_html(&mut inner_html, inner.into_iter());
                    if lvl == 2 || lvl == 3 {
                        toc.push((lvl, id.clone(), text.clone()));
                    }
                    search.push(SearchEntry { heading: text, anchor: id.clone(), text: String::new() });
                    out.push(Event::Html(
                        format!("<h{lvl} id=\"{id}\"><a class=\"anchor\" href=\"#{id}\" aria-hidden=\"true\">#</a>{inner_html}</h{lvl}>\n").into(),
                    ));
                }
                Event::Text(ref t) | Event::Code(ref t) => {
                    text.push_str(t);
                    inner.push(ev);
                }
                other => inner.push(other),
            }
            continue;
        }
        if let Some((_, buf)) = code.as_mut() {
            match ev {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (info, buf) = code.take().unwrap();
                    out.push(Event::Html(code_block(&info, &buf, file, errors).into()));
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Heading { level, id, .. }) => {
                if level == HeadingLevel::H1 {
                    errors.push(format!("{}: use `##` headings; the page title comes from front matter", file.display()));
                }
                heading = Some((level, id.map(|s| s.to_string()), Vec::new(), String::new()));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(i) => i.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((info, String::new()));
            }
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                let (class, label) = match kind {
                    BlockQuoteKind::Note => ("note", "Note"),
                    BlockQuoteKind::Tip => ("tip", "Tip"),
                    BlockQuoteKind::Important => ("important", "Important"),
                    BlockQuoteKind::Warning => ("warning", "Warning"),
                    BlockQuoteKind::Caution => ("caution", "Caution"),
                };
                out.push(Event::Html(format!("<aside class=\"callout {class}\"><p class=\"callout-title\">{label}</p>\n").into()));
            }
            Event::End(TagEnd::BlockQuote(Some(_))) => out.push(Event::Html("</aside>\n".into())),
            Event::Start(Tag::Table(a)) => {
                out.push(Event::Html("<div class=\"table-wrap\">".into()));
                out.push(Event::Start(Tag::Table(a)));
            }
            Event::End(TagEnd::Table) => {
                out.push(Event::End(TagEnd::Table));
                out.push(Event::Html("</div>".into()));
            }
            Event::Text(ref t) | Event::Code(ref t) => {
                let entry = search.last_mut().unwrap();
                if entry.text.len() < 600 {
                    entry.text.push_str(t);
                    entry.text.push(' ');
                }
                out.push(ev);
            }
            other => out.push(other),
        }
    }

    let mut html_out = String::new();
    html::push_html(&mut html_out, out.into_iter());
    search.retain(|e| !e.text.trim().is_empty() || !e.heading.is_empty());
    Rendered { html: html_out, toc, search }
}

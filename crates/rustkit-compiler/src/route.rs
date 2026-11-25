//! Filesystem routes.
//!
//! `layout.rk` is not a URL. It wraps every page in its directory, and nested
//! folders nest layouts, outermost first. A path may contain one `[param]`
//! segment: `blog/[slug].rk` is `/blog/{slug}`. The parameter is a string on
//! the page. A resolver takes it as `&str` when every file that imports the
//! source sits on that same segment. Rest parameters, optional segments and a
//! second dynamic segment are rejected.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Diag;
use crate::expr::Expr;
use crate::parse::{self, AttrValue, Node, PageFile, Part};
use crate::script;

pub struct Span {
    pub base: usize,
    pub src: String,
    pub display: String,
}

pub struct PreparedPage {
    pub display: String,
    pub rel: PathBuf,
    /// Axum path. A dynamic segment is `{name}`.
    pub pattern: String,
    pub param: Option<String>,
    pub page: PageFile,
}

pub struct Prepared {
    pub pages: Vec<PreparedPage>,
    /// `source name` → the route parameter its resolver takes.
    pub param_sources: BTreeMap<String, String>,
    pub spans: Vec<Span>,
}

struct Parsed {
    rel: PathBuf,
    display: String,
    src: String,
    page: PageFile,
    /// Axum path for a page. Empty when the path failed to classify.
    pattern: String,
    param: Option<String>,
    layout: bool,
    classified: bool,
}

pub fn prepare(routes_dir: &Path) -> Result<Prepared, String> {
    let mut rels = Vec::new();
    collect(routes_dir, routes_dir, &mut rels).map_err(|e| format!("reading {}: {e}", routes_dir.display()))?;
    rels.sort();

    let mut errors = String::new();
    let mut parsed = Vec::new();
    for rel in rels {
        let path = routes_dir.join(&rel);
        let src = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return Err(format!("reading {}: {e}", path.display())),
        };
        let display = rel.display().to_string();
        let page = match parse::parse(&src) {
            Ok(p) => p,
            Err(d) => {
                errors.push_str(&d.render(&format!("src/routes/{display}"), &src));
                continue;
            }
        };
        let (pattern, param, classified) = match route_pattern(&rel) {
            Ok((pattern, param)) => (pattern, param, true),
            Err(d) => {
                errors.push_str(&d.render(&format!("src/routes/{display}"), &src));
                (String::new(), None, false)
            }
        };
        parsed.push(Parsed {
            rel,
            display,
            src,
            page,
            pattern,
            param,
            layout: false,
            classified,
        });
    }
    for f in &mut parsed {
        f.layout = f.rel.file_name().is_some_and(|n| n == "layout.rk");
    }

    let mut param_sources = BTreeMap::new();
    if let Err(msg) = source_params(&parsed, &mut param_sources) {
        errors.push_str(&msg);
    }
    for f in &parsed {
        if f.layout {
            if let Err(d) = check_layout(f) {
                errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
            }
        } else if let Some(pos) = first_slot(&f.page.nodes) {
            let d = Diag::new(pos, "`<slot>` is only valid in `layout.rk`");
            errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
        } else if let Err(d) = check_page_prerender(f) {
            errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
        }
    }

    let mut patterns: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in parsed.iter().enumerate() {
        if f.layout || !f.classified {
            continue;
        }
        patterns.entry(f.pattern.clone()).or_default().push(i);
    }
    for (pattern, idxs) in &patterns {
        if idxs.len() > 1 {
            let names: Vec<&str> = idxs.iter().map(|i| parsed[*i].display.as_str()).collect();
            let f = &parsed[idxs[1]];
            let d = Diag::new(0, format!("`{}` and `{}` both serve `{pattern}`", names[0], names[1])).help("an `index.rk` and a file of the same name are the same route; keep one");
            if names.len() > 2 {
                errors.push_str(&format!("`{}` also serves `{pattern}`\n", names[2..].join("`, `")));
            }
            errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Shift template positions into one buffer so a later diagnostic can name
    // the file it came from. Each file is shifted once and then cloned per page.
    let mut buf = String::new();
    let mut spans = Vec::new();
    for f in &mut parsed {
        let base = buf.len();
        buf.push_str(&f.src);
        spans.push(Span {
            base,
            src: f.src.clone(),
            display: format!("src/routes/{}", f.display),
        });
        buf.push('\n');
        shift_nodes(&mut f.page.nodes, base);
    }

    let mut pages = Vec::new();
    for f in &parsed {
        if f.layout || !f.classified {
            continue;
        }
        let pattern = f.pattern.clone();
        let chain = layout_chain(&parsed, &f.rel);
        let layouts: Vec<&Parsed> = chain.iter().map(|n| &parsed[*n]).collect();
        if let Err(msg) = check_state(&layouts, f) {
            errors.push_str(&msg);
            continue;
        }
        let page = compose(&layouts, f, &mut buf, &mut spans);
        pages.push(PreparedPage {
            display: f.display.clone(),
            rel: f.rel.clone(),
            pattern,
            param: f.param.clone(),
            page,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Prepared { pages, param_sources, spans })
}

pub fn render_diag(spans: &[Span], d: &Diag) -> String {
    if let Some(s) = spans.iter().find(|s| d.pos >= s.base && d.pos < s.base + s.src.len()) {
        let mut d = d.clone();
        d.pos -= s.base;
        return d.render(&s.display, &s.src);
    }
    d.render("src/routes/(composed)", "")
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no such directory"));
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            collect(root, &p, out)?;
        } else if p.extension().is_some_and(|x| x == "rk") {
            out.push(p.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

fn route_pattern(rel: &Path) -> Result<(String, Option<String>), Diag> {
    let segs = stem_segs(rel);
    let mut param = None;
    let mut parts = Vec::new();
    for (i, seg) in segs.iter().enumerate() {
        if i + 1 == segs.len() && seg == "index" {
            continue;
        }
        if let Some(name) = dynamic_name(seg)? {
            if param.is_some() {
                return Err(
                    Diag::new(0, format!("a route can have only one `[param]` segment; this path also has `[{name}]`")).help("rest parameters and multiple dynamic segments are not in this milestone")
                );
            }
            param = Some(name.clone());
            parts.push(format!("{{{name}}}"));
        } else {
            parts.push(seg.clone());
        }
    }
    let pattern = if parts.is_empty() { "/".to_owned() } else { format!("/{}", parts.join("/")) };
    Ok((pattern, param))
}

fn stem_segs(rel: &Path) -> Vec<String> {
    rel.with_extension("").components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
}

/// `Some(name)` for a `[param]` segment, `None` for a static segment.
fn dynamic_name(seg: &str) -> Result<Option<String>, Diag> {
    if seg.starts_with("[[") && seg.ends_with("]]") {
        return Err(Diag::new(0, format!("optional segments (`{seg}`) are not supported")).help("this milestone has layouts and one `[param]` segment"));
    }
    if seg.starts_with("[...") && seg.ends_with(']') {
        return Err(Diag::new(0, format!("rest parameters (`{seg}`) are not supported")).help("this milestone has layouts and one `[param]` segment"));
    }
    if let Some(name) = seg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if !is_param_name(name) {
            return Err(Diag::new(0, format!("`[{name}]` is not a valid parameter name")).help("use a Rust identifier: letters, digits and `_`. `self`, `super`, `crate` and `Self` are not allowed"));
        }
        return Ok(Some(name.to_owned()));
    }
    if seg.contains('[') || seg.contains(']') {
        return Err(Diag::new(0, format!("`{seg}` is not a valid route segment")));
    }
    Ok(None)
}

fn is_param_name(name: &str) -> bool {
    if matches!(name, "self" | "super" | "crate" | "Self") {
        return false;
    }
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_layout(f: &Parsed) -> Result<(), Diag> {
    let slots = slot_positions(&f.page.nodes);
    match slots.as_slice() {
        [] => {
            return Err(Diag::new(0, "`layout.rk` needs a `<slot>` where the page is rendered").help("write `<slot />` once; the pages in this folder are rendered there"));
        }
        [_] => {}
        [_, second, ..] => {
            return Err(Diag::new(*second, "a layout can have only one `<slot>`"));
        }
    }
    if let Some((src, pos)) = &f.page.module_script {
        if let Some(i) = src.find("export const prerender = true") {
            return Err(Diag::new(pos + i, "`prerender` belongs on the page, not on `layout.rk`"));
        }
    }
    Ok(())
}

fn check_page_prerender(f: &Parsed) -> Result<(), Diag> {
    let Some(name) = &f.param else { return Ok(()) };
    let Some((src, pos)) = &f.page.module_script else {
        return Ok(());
    };
    let Some(i) = src.find("export const prerender = true") else {
        return Ok(());
    };
    Err(Diag::new(pos + i, format!("`[{name}]` has no single page to prerender")).help("remove `prerender`, or use a fixed path. Content collections are not in this milestone"))
}

fn source_params(files: &[Parsed], out: &mut BTreeMap<String, String>) -> Result<(), String> {
    let mut sites: BTreeMap<String, Vec<(usize, Option<String>)>> = BTreeMap::new();
    let mut errors = String::new();
    for (i, f) in files.iter().enumerate() {
        let names = match &f.page.script {
            Some((src, pos)) => match script::parse(src, *pos) {
                Ok(s) => s.sources,
                Err(d) => {
                    errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
                    continue;
                }
            },
            None => Vec::new(),
        };
        let mut seen = BTreeSet::new();
        for name in names {
            if seen.insert(name.clone()) {
                sites.entry(name).or_default().push((i, f.param.clone()));
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for (source, sites) in sites {
        let mut kinds: BTreeMap<Option<String>, Vec<usize>> = BTreeMap::new();
        for (i, param) in sites {
            kinds.entry(param).or_default().push(i);
        }
        if kinds.len() == 1 {
            if let Some(param) = kinds.keys().next().cloned().flatten() {
                out.insert(source, param);
            }
            continue;
        }
        let describe = |idxs: &[usize]| -> String { idxs.iter().map(|i| format!("`{}`", files[*i].display)).collect::<Vec<_>>().join(", ") };
        let mut parts = Vec::new();
        let mut at = 0usize;
        for (kind, idxs) in &kinds {
            at = idxs[0];
            match kind {
                Some(name) => parts.push(format!("`{name}` from {}", describe(idxs))),
                None => parts.push(format!("no parameter from {}", describe(idxs))),
            }
        }
        let f = &files[at];
        let pos = import_pos(f, &source);
        let d = Diag::new(pos, format!("source `{source}` is imported with {}", parts.join(", and with ")))
            .help("a resolver takes the parameter only when every file that imports it sits on that `[param]`. Move the import under the dynamic segment, or split the source");
        errors.push_str(&d.render(&format!("src/routes/{}", f.display), &f.src));
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

fn import_pos(f: &Parsed, name: &str) -> usize {
    let Some((src, base)) = &f.page.script else {
        return 0;
    };
    src.find(name).map_or(*base, |i| base + i)
}

fn check_state(layouts: &[&Parsed], page: &Parsed) -> Result<(), String> {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for f in layouts.iter().copied().chain(std::iter::once(page)) {
        let Some((src, base)) = &f.page.script else {
            continue;
        };
        let script = script::parse(src, *base).map_err(|d| d.render(&format!("src/routes/{}", f.display), &f.src))?;
        for (name, _) in script.state {
            if let Some(prev) = seen.get(&name) {
                let pos = src.find(&format!("let {name}")).map_or(*base, |i| base + i);
                let d = Diag::new(pos, format!("`$state` `{name}` is already declared in `{prev}`"));
                return Err(d.render(&format!("src/routes/{}", f.display), &f.src));
            }
            if page.param.as_ref().is_some_and(|p| p == &name) {
                let pos = src.find(&format!("let {name}")).map_or(*base, |i| base + i);
                let d = Diag::new(pos, format!("`{name}` is the route parameter, so it cannot also be `$state`"));
                return Err(d.render(&format!("src/routes/{}", f.display), &f.src));
            }
            seen.insert(name, f.display.clone());
        }
    }
    Ok(())
}

/// Layouts that wrap `page`, outermost first.
fn layout_chain(files: &[Parsed], page: &Path) -> Vec<usize> {
    let mut dirs = Vec::new();
    let mut dir = page.parent().unwrap_or(Path::new("")).to_path_buf();
    loop {
        dirs.push(dir.clone());
        if dir.as_os_str().is_empty() {
            break;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    dirs.reverse();
    let mut out = Vec::new();
    for d in dirs {
        let candidate = if d.as_os_str().is_empty() { PathBuf::from("layout.rk") } else { d.join("layout.rk") };
        if let Some(i) = files.iter().position(|f| f.layout && f.rel == candidate) {
            out.push(i);
        }
    }
    out
}

fn compose(layouts: &[&Parsed], page: &Parsed, buf: &mut String, spans: &mut Vec<Span>) -> PageFile {
    let mut nodes = page.page.nodes.clone();
    for layout in layouts.iter().rev() {
        let mut tree = layout.page.nodes.clone();
        let mut child = Some(std::mem::take(&mut nodes));
        fill_slot(&mut tree, &mut child);
        nodes = tree;
    }
    let mut next = 0usize;
    renumber(&mut nodes, &mut next);

    let mut css = String::new();
    for f in layouts.iter().copied().chain(std::iter::once(page)) {
        if let Some(style) = &f.page.style {
            if !css.is_empty() && !css.ends_with('\n') {
                css.push('\n');
            }
            css.push_str(style);
            if !style.ends_with('\n') {
                css.push('\n');
            }
        }
    }

    let mut merged = String::new();
    for f in layouts.iter().copied().chain(std::iter::once(page)) {
        if let Some((src, _)) = &f.page.script {
            merged.push_str(src);
            if !src.ends_with('\n') {
                merged.push('\n');
            }
        }
    }
    let script = if merged.is_empty() {
        None
    } else {
        let base = buf.len();
        buf.push_str(&merged);
        spans.push(Span {
            base,
            src: merged.clone(),
            display: format!("src/routes/{} (script)", page.display),
        });
        buf.push('\n');
        Some((merged, base))
    };

    let title = page.page.title.clone().or_else(|| layouts.iter().rev().find_map(|l| l.page.title.clone()));
    PageFile {
        script,
        module_script: page.page.module_script.clone(),
        style: if css.trim().is_empty() { None } else { Some(css) },
        title,
        nodes,
    }
}

fn first_slot(nodes: &[Node]) -> Option<usize> {
    let mut slots = Vec::new();
    slot_positions_into(nodes, &mut slots);
    slots.into_iter().next()
}

fn slot_positions(nodes: &[Node]) -> Vec<usize> {
    let mut slots = Vec::new();
    slot_positions_into(nodes, &mut slots);
    slots
}

fn slot_positions_into(nodes: &[Node], out: &mut Vec<usize>) {
    for n in nodes {
        match n {
            Node::Slot { pos } => out.push(*pos),
            Node::Element(e) => slot_positions_into(&e.children, out),
            Node::If { branches, els, .. } => {
                for (_, body) in branches {
                    slot_positions_into(body, out);
                }
                if let Some(body) = els {
                    slot_positions_into(body, out);
                }
            }
            Node::Each { body, .. } | Node::Source { body, .. } => slot_positions_into(body, out),
            Node::Text(_) | Node::Expr { .. } => {}
        }
    }
}

fn fill_slot(nodes: &mut Vec<Node>, child: &mut Option<Vec<Node>>) -> bool {
    let mut i = 0;
    while i < nodes.len() {
        if matches!(nodes[i], Node::Slot { .. }) {
            let kids = child.take().unwrap();
            let mut rest = nodes.split_off(i);
            rest.remove(0);
            nodes.extend(kids);
            nodes.extend(rest);
            return true;
        }
        let hit = match &mut nodes[i] {
            Node::Element(e) => fill_slot(&mut e.children, child),
            Node::If { branches, els, .. } => {
                let mut hit = false;
                for (_, body) in branches {
                    if fill_slot(body, child) {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    if let Some(body) = els {
                        hit = fill_slot(body, child);
                    }
                }
                hit
            }
            Node::Each { body, .. } | Node::Source { body, .. } => fill_slot(body, child),
            _ => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

fn renumber(nodes: &mut [Node], next: &mut usize) {
    for n in nodes {
        match n {
            Node::Text(_) | Node::Slot { .. } => {}
            Node::Expr { id, .. } => {
                *next += 1;
                *id = *next;
            }
            Node::Element(e) => {
                *next += 1;
                e.id = *next;
                renumber(&mut e.children, next);
            }
            Node::If { id, branches, els, .. } => {
                *next += 1;
                *id = *next;
                for (_, body) in branches {
                    renumber(body, next);
                }
                if let Some(body) = els {
                    renumber(body, next);
                }
            }
            Node::Each { id, body, .. } | Node::Source { id, body, .. } => {
                *next += 1;
                *id = *next;
                renumber(body, next);
            }
        }
    }
}

fn shift_nodes(nodes: &mut [Node], delta: usize) {
    if delta == 0 {
        return;
    }
    for n in nodes {
        match n {
            Node::Text(_) => {}
            Node::Slot { pos } => *pos += delta,
            Node::Expr { expr, .. } => shift_expr(expr, delta),
            Node::Element(e) => {
                for a in &mut e.attrs {
                    match &mut a.value {
                        AttrValue::Handler { pos, .. } => *pos += delta,
                        AttrValue::Parts(parts) => {
                            for p in parts {
                                if let Part::Expr(e) = p {
                                    shift_expr(e, delta);
                                }
                            }
                        }
                        AttrValue::Bare | AttrValue::Static(_) => {}
                    }
                }
                shift_nodes(&mut e.children, delta);
            }
            Node::If { branches, els, .. } => {
                for (cond, body) in branches {
                    shift_expr(cond, delta);
                    shift_nodes(body, delta);
                }
                if let Some(body) = els {
                    shift_nodes(body, delta);
                }
            }
            Node::Each { expr, body, pos, .. } => {
                shift_expr(expr, delta);
                *pos += delta;
                shift_nodes(body, delta);
            }
            Node::Source { body, pos, .. } => {
                *pos += delta;
                shift_nodes(body, delta);
            }
        }
    }
}

fn shift_expr(e: &mut Expr, delta: usize) {
    match e {
        Expr::Path { pos, .. } => *pos += delta,
        Expr::Not(inner) => shift_expr(inner, delta),
        Expr::Bin(_, a, b) => {
            shift_expr(a, delta);
            shift_expr(b, delta);
        }
        Expr::Str(_) | Expr::Num(_) | Expr::Bool(_) | Expr::Null => {}
    }
}

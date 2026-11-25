//! Code generation for one page.
//!
//! The template is walked twice from the same tree. The Rust walk produces the
//! initial HTML. Any node that depends on browser state (or loads data in the
//! browser) becomes a *block*: Rust renders its initial contents between
//! `<!--rk:bN-->` markers, and the JavaScript walk produces a render function
//! the browser runtime calls when that block's state changes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::Diag;
use crate::expr::{Expr, Op};
use crate::parse::{AttrValue, Element, Load, Node, PageFile, Part};
use crate::script::{self, Script};

#[derive(Clone, Debug, PartialEq)]
enum Binding {
    ServerSource(String),
    ClientSource(String),
    Loop,
    /// The route's `[param]`, a string in scope for the whole composed page.
    Param,
}

type Scope = Vec<(String, Binding)>;

#[derive(Default)]
enum Projection {
    #[default]
    None,
    Fields(BTreeSet<String>),
    All,
}

pub struct Options<'a> {
    pub state_type: &'a str,
    pub sources_module: &'a str,
}

/// The generated code for one page.
pub struct PageOutput {
    /// Body of the Rust `render` function's module.
    pub rust: String,
    /// The page's browser module, if it needs one.
    pub js: Option<String>,
    pub css: Option<String>,
    pub prerender: bool,
    pub server_sources: Vec<String>,
    pub client_sources: Vec<String>,
}

struct Gen<'a> {
    opts: &'a Options<'a>,
    state: Vec<(String, Expr)>,
    imported: Vec<String>,
    blocks: BTreeMap<String, String>,
    handlers: BTreeMap<String, String>,
    handler_keys: BTreeMap<(usize, usize), String>,
    ssr_scopes: BTreeMap<String, String>,
    events: BTreeSet<String>,
    server_sources: Vec<String>,
    client_sources: BTreeSet<String>,
    projections: BTreeMap<String, Projection>,
    csrf_in_js: bool,
    tmp: usize,
    /// The route's single `[param]`, if it has one.
    param: Option<&'a str>,
    /// Sources called as `source(ctx, params.<param>.as_str())`.
    param_sources: &'a BTreeSet<String>,
    /// A browser block or handler reads the route parameter.
    param_in_js: bool,
}

pub fn generate(page: &PageFile, opts: &Options) -> Result<PageOutput, Diag> {
    let none = BTreeSet::new();
    generate_routed(page, opts, None, &none)
}

/// `param` is the route's `[name]` segment. `param_sources` are resolvers
/// called with that parameter as `&str`.
pub(crate) fn generate_routed(page: &PageFile, opts: &Options, param: Option<&str>, param_sources: &BTreeSet<String>) -> Result<PageOutput, Diag> {
    let script = match &page.script {
        Some((src, pos)) => script::parse(src, *pos)?,
        None => Script {
            sources: vec![],
            state: vec![],
            rest: String::new(),
        },
    };
    if let Some(name) = param {
        if let Some((src, base)) = &page.script {
            if script.state.iter().any(|(n, _)| n == name) {
                let at = src.find(&format!("let {name}")).map_or(*base, |i| base + i);
                return Err(Diag::new(at, format!("`{name}` is the route parameter, so it cannot also be `$state`")));
            }
        }
    }
    let prerender = page.module_script.as_ref().is_some_and(|(s, _)| s.contains("export const prerender = true"));

    let mut g = Gen {
        opts,
        state: script.state.clone(),
        imported: script.sources.clone(),
        blocks: BTreeMap::new(),
        handlers: BTreeMap::new(),
        handler_keys: BTreeMap::new(),
        ssr_scopes: BTreeMap::new(),
        events: BTreeSet::new(),
        server_sources: Vec::new(),
        client_sources: BTreeSet::new(),
        projections: BTreeMap::new(),
        csrf_in_js: false,
        tmp: 0,
        param,
        param_sources,
        param_in_js: false,
    };

    let mut scope = Vec::new();
    if let Some(name) = param {
        scope.push((name.to_owned(), Binding::Param));
    }
    let mut body = String::new();
    g.rs_nodes(&page.nodes, &scope, &mut body)?;

    let state_names: Vec<String> = g.state.iter().map(|(n, _)| n.clone()).collect();
    let user_js = script::rewrite_state(&script.rest, &state_names);
    let needs_js = !g.blocks.is_empty() || !g.handlers.is_empty() || !user_js.trim().is_empty();

    let title = page.title.clone().unwrap_or_default();
    let rust = g.rust_module(&body, needs_js, &title);
    let js = needs_js.then(|| g.js_module(&user_js));
    Ok(PageOutput {
        rust,
        js,
        css: page.style.clone().filter(|s| !s.trim().is_empty()),
        prerender,
        server_sources: g.server_sources.clone(),
        client_sources: g.client_sources.iter().cloned().collect(),
    })
}

fn lookup<'s>(scope: &'s Scope, name: &str) -> Option<&'s Binding> {
    scope.iter().rev().find(|(n, _)| n == name).map(|(_, b)| b)
}

/// A parameter or field name that is a Rust keyword, written so it parses.
fn raw_ident(name: &str) -> String {
    const KW: [&str; 12] = ["type", "match", "move", "ref", "self", "struct", "use", "fn", "impl", "loop", "mod", "where"];
    if KW.contains(&name) { format!("r#{name}") } else { name.to_owned() }
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    raw_ident(&out)
}

/// A JavaScript string literal.
fn js_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Joins runs of adjacent literal writes (`out.push_str("…");` or
/// `h += "…";`) into one, so the renderer makes fewer, larger writes.
fn coalesce(code: &str, prefix: &str, suffix: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut run: Option<String> = None;
    for line in code.lines() {
        if let Some(inner) = line.strip_prefix(prefix).and_then(|l| l.strip_suffix(suffix)) {
            run.get_or_insert_with(String::new).push_str(inner);
            continue;
        }
        if let Some(r) = run.take() {
            let _ = writeln!(out, "{prefix}{r}{suffix}");
        }
        out.push_str(line);
        out.push('\n');
    }
    if let Some(r) = run {
        let _ = writeln!(out, "{prefix}{r}{suffix}");
    }
    out
}

fn rs_str(s: &str) -> String {
    format!("{s:?}")
}

fn is_post_form(e: &Element) -> bool {
    e.tag == "form" && e.attrs.iter().any(|a| a.name == "method" && matches!(&a.value, AttrValue::Static(v) if v.eq_ignore_ascii_case("post")))
}

fn static_attr(name: &str, raw: &str) -> String {
    format!(" {name}=\"{}\"", raw.replace('"', "&quot;"))
}

impl Gen<'_> {
    fn is_state(&self, name: &str, scope: &Scope) -> bool {
        lookup(scope, name).is_none() && self.state.iter().any(|(n, _)| n == name)
    }

    fn uses_state(&self, e: &Expr, scope: &Scope) -> bool {
        e.roots().iter().any(|r| self.is_state(r, scope))
    }

    fn state_deps(&self, exprs: &[&Expr], scope: &Scope) -> Vec<String> {
        let mut deps: Vec<String> = exprs.iter().flat_map(|e| e.roots()).filter(|r| self.is_state(r, scope)).collect();
        deps.sort();
        deps.dedup();
        deps
    }

    /// Expressions whose state dependencies make `node` a block of its own.
    fn own_exprs<'n>(node: &'n Node) -> Vec<&'n Expr> {
        match node {
            Node::Expr { expr, .. } => vec![expr],
            Node::If { branches, .. } => branches.iter().map(|(c, _)| c).collect(),
            Node::Each { expr, .. } => vec![expr],
            Node::Element(e) => e
                .attrs
                .iter()
                .flat_map(|a| match &a.value {
                    AttrValue::Parts(p) => p.iter().filter_map(|p| if let Part::Expr(e) = p { Some(e) } else { None }).collect(),
                    _ => vec![],
                })
                .collect(),
            _ => vec![],
        }
    }

    fn is_dynamic(&self, node: &Node, scope: &Scope) -> bool {
        match node {
            Node::Source { load: Load::Client, .. } => true,
            Node::Slot { .. } | Node::Source { .. } | Node::Text(_) => false,
            n => Self::own_exprs(n).iter().any(|e| self.uses_state(e, scope)),
        }
    }

    fn node_id(node: &Node) -> usize {
        match node {
            Node::Expr { id, .. } | Node::If { id, .. } | Node::Each { id, .. } | Node::Source { id, .. } => *id,
            Node::Element(e) => e.id,
            Node::Text(_) | Node::Slot { .. } => unreachable!(),
        }
    }

    fn check_source(&self, name: &str, pos: usize) -> Result<(), Diag> {
        if self.imported.iter().any(|s| s == name) {
            Ok(())
        } else {
            Err(Diag::new(pos, format!("`{name}` is not imported from \"$sources\"")).help(format!("add `import {{ {name} }} from \"$sources\";` to the page script")))
        }
    }

    fn unknown(&self, name: &str, pos: usize) -> Diag {
        Diag::new(pos, format!("`{name}` is not defined here")).help("template expressions can read `$state` variables, a `<DataSource let:…>` binding, an `{#each}` item, or the route's `[param]`")
    }

    fn source_call(&self, name: &str) -> String {
        let sm = self.opts.sources_module;
        if self.param_sources.contains(name) {
            let ident = raw_ident(self.param.expect("parameter source on a fixed route"));
            format!("{sm}::{name}(ctx, params.{ident}.as_str())")
        } else {
            format!("{sm}::{name}(ctx)")
        }
    }

    // ---------------------------------------------------------------- Rust

    fn rs_val(&self, e: &Expr, scope: &Scope) -> Result<String, Diag> {
        const J: &str = "::rustkit::js";
        Ok(match e {
            Expr::Str(s) => format!("{J}::Val::str({})", rs_str(s)),
            Expr::Num(n) => format!("{J}::Val::Num({n:?})"),
            Expr::Bool(b) => format!("{J}::Val::Bool({b})"),
            Expr::Null => format!("{J}::Val::Null"),
            Expr::Not(x) => format!("{J}::Val::Bool(!({}).truthy())", self.rs_val(x, scope)?),
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.rs_val(a, scope)?, self.rs_val(b, scope)?);
                match op {
                    Op::And => format!("{J}::Val::Bool(({a}).truthy() && ({b}).truthy())"),
                    Op::Or => format!("{J}::Val::Bool(({a}).truthy() || ({b}).truthy())"),
                    Op::Eq => format!("{J}::Val::Bool({J}::strict_eq(&({a}), &({b})))"),
                    Op::Ne => format!("{J}::Val::Bool(!{J}::strict_eq(&({a}), &({b})))"),
                    Op::Lt => format!("{J}::Val::Bool({J}::lt(&({a}), &({b})))"),
                    Op::Le => format!("{J}::Val::Bool({J}::le(&({a}), &({b})))"),
                    Op::Gt => format!("{J}::Val::Bool({J}::gt(&({a}), &({b})))"),
                    Op::Ge => format!("{J}::Val::Bool({J}::ge(&({a}), &({b})))"),
                }
            }
            Expr::Path { root, segs, pos } => {
                if self.is_state(root, scope) {
                    let lit = &self.state.iter().find(|(n, _)| n == root).unwrap().1;
                    if !segs.is_empty() {
                        return Err(Diag::new(*pos, "state values are primitives and have no fields"));
                    }
                    return self.rs_val(lit, scope);
                }
                let (segs, length) = match segs.split_last() {
                    Some((last, init)) if last == "length" => (init, true),
                    _ => (segs.as_slice(), false),
                };
                let path = match lookup(scope, root) {
                    Some(Binding::Param) => {
                        if !segs.is_empty() {
                            return Err(Diag::new(*pos, "a route parameter is a string and has no fields"));
                        }
                        format!("params.{}", raw_ident(root))
                    }
                    Some(_) => {
                        let mut path = format!("v_{root}");
                        for s in segs {
                            path.push('.');
                            path.push_str(&snake(s));
                        }
                        path
                    }
                    None => return Err(self.unknown(root, *pos)),
                };
                if length { format!("{J}::length(&{path})") } else { format!("{J}::ToJs::to_js(&{path})") }
            }
        })
    }

    fn rs_nodes(&mut self, nodes: &[Node], scope: &Scope, out: &mut String) -> Result<(), Diag> {
        for n in nodes {
            self.rs_node(n, scope, out)?;
        }
        Ok(())
    }

    fn rs_node(&mut self, node: &Node, scope: &Scope, out: &mut String) -> Result<(), Diag> {
        if let Node::Text(t) = node {
            let _ = writeln!(out, "out.push_str({});", rs_str(t));
            return Ok(());
        }
        if let Node::Slot { pos } = node {
            return Err(Diag::new(*pos, "`<slot>` is only valid in `layout.rk`"));
        }
        if !self.is_dynamic(node, scope) {
            return self.rs_inner(node, scope, out);
        }
        let key = self.js_block(node, scope)?;
        self.ssr_scope(&key, node, scope)?;
        let _ = writeln!(out, "out.push_str(\"<!--rk:{key}-->\");");
        self.rs_inner(node, scope, out)?;
        let _ = writeln!(out, "out.push_str(\"<!--/rk:{key}-->\");");
        Ok(())
    }

    /// Renders a node's initial HTML, ignoring whether the node itself is a block.
    fn rs_inner(&mut self, node: &Node, scope: &Scope, out: &mut String) -> Result<(), Diag> {
        match node {
            Node::Text(_) | Node::Slot { .. } => unreachable!(),
            Node::Expr { expr, .. } => {
                let _ = writeln!(out, "::rustkit::js::text(&mut out, &{});", self.rs_val(expr, scope)?);
            }
            Node::Element(e) => self.rs_element(e, scope, out)?,
            Node::If { branches, els, .. } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    let _ = writeln!(out, "{kw} ({}).truthy() {{", self.rs_val(cond, scope)?);
                    self.rs_nodes(body, scope, out)?;
                }
                if let Some(body) = els {
                    out.push_str("} else {\n");
                    self.rs_nodes(body, scope, out)?;
                }
                out.push_str("}\n");
            }
            Node::Each { expr, item, index, body, pos, .. } => {
                let Expr::Path { root, segs, pos: ppos } = expr else {
                    return Err(Diag::new(*pos, "`{#each}` needs a list field, such as `data.items`"));
                };
                if lookup(scope, root).is_none() {
                    return Err(if self.is_state(root, scope) {
                        Diag::new(*ppos, "`{#each}` over `$state` is not supported yet").help("state is limited to primitive literals in the first release; iterate over a source field instead")
                    } else {
                        self.unknown(root, *ppos)
                    });
                }
                let mut path = format!("v_{root}");
                for s in segs {
                    path.push('.');
                    path.push_str(&snake(s));
                }
                let idx = index.as_deref().map_or("_".to_owned(), |i| format!("v_{i}"));
                let _ = writeln!(out, "for ({idx}, v_{item}) in (&{path}).into_iter().enumerate() {{");
                let mut inner = scope.clone();
                inner.push((item.clone(), Binding::Loop));
                if let Some(i) = index {
                    inner.push((i.clone(), Binding::Loop));
                }
                self.rs_nodes(body, &inner, out)?;
                out.push_str("}\n");
            }
            Node::Source {
                source,
                load: Load::Server,
                bind,
                body,
                pos,
                ..
            } => {
                self.check_source(source, *pos)?;
                if !self.server_sources.contains(source) {
                    self.server_sources.push(source.clone());
                }
                let _ = writeln!(out, "{{ let v_{bind} = &src_{source};");
                let mut inner = scope.clone();
                inner.push((bind.clone(), Binding::ServerSource(source.clone())));
                self.rs_nodes(body, &inner, out)?;
                out.push_str("}\n");
            }
            Node::Source { load: Load::Client, .. } => {
                // The browser fills this region after it fetches the source.
                out.push_str("out.push_str(\"<p class=\\\"rk-loading\\\" aria-busy=\\\"true\\\">Loading…</p>\");\n");
            }
        }
        Ok(())
    }

    fn rs_element(&mut self, e: &Element, scope: &Scope, out: &mut String) -> Result<(), Diag> {
        let _ = writeln!(out, "out.push_str(\"<{}\");", e.tag);
        let mut handlers = Vec::new();
        for (ai, a) in e.attrs.iter().enumerate() {
            match &a.value {
                AttrValue::Bare => {
                    let _ = writeln!(out, "out.push_str({});", rs_str(&format!(" {}", a.name)));
                }
                AttrValue::Static(v) => {
                    let _ = writeln!(out, "out.push_str({});", rs_str(&static_attr(&a.name, v)));
                }
                AttrValue::Parts(parts) => {
                    if let [Part::Expr(x)] = parts.as_slice() {
                        let _ = writeln!(out, "::rustkit::js::attr(&mut out, {}, &{});", rs_str(&a.name), self.rs_val(x, scope)?);
                    } else {
                        let _ = writeln!(out, "out.push_str({});", rs_str(&format!(" {}=\"", a.name)));
                        for p in parts {
                            match p {
                                Part::Text(t) => {
                                    let _ = writeln!(out, "out.push_str({});", rs_str(&t.replace('"', "&quot;")));
                                }
                                Part::Expr(x) => {
                                    let _ = writeln!(out, "::rustkit::js::text(&mut out, &{});", self.rs_val(x, scope)?);
                                }
                            }
                        }
                        out.push_str("out.push_str(\"\\\"\");\n");
                    }
                }
                AttrValue::Handler { js, pos } => {
                    let key = self.handler(e.id, ai, &a.name, js, *pos, scope)?;
                    self.ssr_scope_handler(&key, js, *pos, scope)?;
                    handlers.push(format!("{}:{key}", &a.name[2..]));
                }
            }
        }
        if !handlers.is_empty() {
            let _ = writeln!(out, "out.push_str({});", rs_str(&format!(" data-rk-on=\"{}\"", handlers.join(" "))));
        }
        out.push_str("out.push_str(\">\");\n");
        if is_post_form(e) {
            out.push_str("out.push_str(\"<input type=\\\"hidden\\\" name=\\\"_csrf\\\" value=\\\"\");\n");
            out.push_str("::rustkit::js::escape_into(&mut out, &ctx.csrf_token());\n");
            out.push_str("out.push_str(\"\\\">\");\n");
        }
        if !e.void {
            self.rs_nodes(&e.children, scope, out)?;
            let _ = writeln!(out, "out.push_str(\"</{}>\");", e.tag);
        }
        Ok(())
    }

    // ------------------------------------------------- server → browser handoff

    /// Collects the names a subtree reads from outside itself, with the first
    /// field read on each (`data.displayName` → (`data`, `displayName`)).
    fn free_refs(&self, nodes: &[&Node], bound: &mut Vec<String>, out: &mut Vec<(String, Option<String>, usize)>) {
        let push_expr = |e: &Expr, bound: &Vec<String>, out: &mut Vec<(String, Option<String>, usize)>| {
            let mut v = Vec::new();
            e.paths(&mut v);
            let pos = match e {
                Expr::Path { pos, .. } => *pos,
                _ => 0,
            };
            for (r, f) in v {
                if !bound.contains(&r) {
                    out.push((r, f, pos));
                }
            }
        };
        for node in nodes {
            match node {
                Node::Text(_) | Node::Slot { .. } => {}
                Node::Expr { expr, .. } => push_expr(expr, bound, out),
                Node::Element(e) => {
                    for a in &e.attrs {
                        match &a.value {
                            AttrValue::Parts(parts) => {
                                for p in parts {
                                    if let Part::Expr(x) = p {
                                        push_expr(x, bound, out);
                                    }
                                }
                            }
                            AttrValue::Handler { js, pos } => {
                                for u in script::idents(js) {
                                    if !u.is_property && !bound.contains(&u.name) {
                                        out.push((u.name, u.field, *pos));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    self.free_refs(&e.children.iter().collect::<Vec<_>>(), bound, out);
                }
                Node::If { branches, els, .. } => {
                    for (c, body) in branches {
                        push_expr(c, bound, out);
                        self.free_refs(&body.iter().collect::<Vec<_>>(), bound, out);
                    }
                    if let Some(body) = els {
                        self.free_refs(&body.iter().collect::<Vec<_>>(), bound, out);
                    }
                }
                Node::Each { expr, item, index, body, .. } => {
                    push_expr(expr, bound, out);
                    let n = bound.len();
                    bound.push(item.clone());
                    bound.extend(index.clone());
                    self.free_refs(&body.iter().collect::<Vec<_>>(), bound, out);
                    bound.truncate(n);
                }
                Node::Source { bind, body, .. } => {
                    let n = bound.len();
                    bound.push(bind.clone());
                    self.free_refs(&body.iter().collect::<Vec<_>>(), bound, out);
                    bound.truncate(n);
                }
            }
        }
    }

    /// Records how the browser rebuilds the scope of a server-rendered block or
    /// handler, and which server-loaded fields it must therefore receive.
    fn handoff(&mut self, key: &str, refs: Vec<(String, Option<String>, usize)>, scope: &Scope) -> Result<(), Diag> {
        let mut entries = Vec::new();
        for (root, field, pos) in refs {
            match lookup(scope, &root) {
                Some(Binding::ServerSource(src)) => {
                    let p = self.projections.entry(src.clone()).or_default();
                    match (field, &mut *p) {
                        (_, Projection::All) => {}
                        (Some(f), Projection::Fields(set)) => {
                            set.insert(f);
                        }
                        (Some(f), Projection::None) => *p = Projection::Fields(BTreeSet::from([f])),
                        (None, _) => *p = Projection::All,
                    }
                    let entry = format!("{root}: P.s[{}]", js_str(src));
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }
                Some(Binding::Param) => {
                    self.param_in_js = true;
                    let entry = format!("{root}: P.params[{}]", js_str(&root));
                    if !entries.contains(&entry) {
                        entries.push(entry);
                    }
                }
                Some(Binding::Loop) => {
                    return Err(Diag::new(pos, format!("`{root}` comes from a server-rendered `{{#each}}`, so the browser cannot see it here"))
                        .help("make the loop itself depend on state or load its source with `load=\"client\"`, or move this interaction outside the loop"));
                }
                Some(Binding::ClientSource(_)) | None => {}
            }
        }
        self.ssr_scopes.insert(key.to_owned(), format!("(P) => ({{ {} }})", entries.join(", ")));
        Ok(())
    }

    fn ssr_scope(&mut self, key: &str, node: &Node, scope: &Scope) -> Result<(), Diag> {
        let mut refs = Vec::new();
        self.free_refs(&[node], &mut Vec::new(), &mut refs);
        self.handoff(key, refs, scope)
    }

    fn ssr_scope_handler(&mut self, key: &str, js: &str, pos: usize, scope: &Scope) -> Result<(), Diag> {
        let refs = script::idents(js).into_iter().filter(|u| !u.is_property).map(|u| (u.name, u.field, pos)).collect();
        self.handoff(key, refs, scope)
    }

    // ----------------------------------------------------------- JavaScript

    fn js_val(&self, e: &Expr, scope: &Scope) -> Result<String, Diag> {
        Ok(match e {
            Expr::Str(s) => js_str(s),
            Expr::Num(n) => format!("{n}"),
            Expr::Bool(b) => b.to_string(),
            Expr::Null => "null".into(),
            Expr::Not(x) => format!("!({})", self.js_val(x, scope)?),
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.js_val(a, scope)?, self.js_val(b, scope)?);
                match op {
                    Op::And => format!("(!!({a}) && !!({b}))"),
                    Op::Or => format!("(!!({a}) || !!({b}))"),
                    Op::Eq => format!("(({a}) === ({b}))"),
                    Op::Ne => format!("(({a}) !== ({b}))"),
                    Op::Lt => format!("(({a}) < ({b}))"),
                    Op::Le => format!("(({a}) <= ({b}))"),
                    Op::Gt => format!("(({a}) > ({b}))"),
                    Op::Ge => format!("(({a}) >= ({b}))"),
                }
            }
            Expr::Path { root, segs, pos } => {
                let base = if self.is_state(root, scope) {
                    format!("$s.{root}")
                } else if lookup(scope, root).is_some() {
                    root.clone()
                } else {
                    return Err(self.unknown(root, *pos));
                };
                // Optional chaining keeps a missing intermediate value as `undefined`,
                // which renders as empty text, as on the server.
                std::iter::once(base).chain(segs.iter().cloned()).collect::<Vec<_>>().join("?.")
            }
        })
    }

    fn destructure(scope: &Scope, cvar: &str) -> String {
        let mut names: Vec<&str> = Vec::new();
        for (n, _) in scope.iter().rev() {
            if !names.contains(&n.as_str()) {
                names.push(n);
            }
        }
        if names.is_empty() {
            String::new()
        } else {
            format!("const {{ {} }} = {cvar}; ", names.join(", "))
        }
    }

    /// Registers the browser render function for a block and returns its key.
    fn js_block(&mut self, node: &Node, scope: &Scope) -> Result<String, Diag> {
        let key = format!("b{}", Self::node_id(node));
        if self.blocks.contains_key(&key) {
            return Ok(key);
        }
        self.blocks.insert(key.clone(), String::new());
        let mut body = String::new();
        let entry = match node {
            Node::Source {
                source,
                load: Load::Client,
                bind,
                body: children,
                pos,
                ..
            } => {
                self.check_source(source, *pos)?;
                self.client_sources.insert(source.clone());
                let mut inner = scope.clone();
                inner.push((bind.clone(), Binding::ClientSource(source.clone())));
                self.js_nodes(children, &inner, "c", &mut body)?;
                format!(
                    "{{ deps: [], source: {}, bind: {}, render: (c, r) => {{ {}let h = \"\";\n{body}return h; }} }}",
                    js_str(source),
                    js_str(bind),
                    Self::destructure(&inner, "c")
                )
            }
            _ => {
                let deps = self.state_deps(&Self::own_exprs(node), scope);
                self.js_inner(node, scope, "c", &mut body)?;
                format!(
                    "{{ deps: [{}], render: (c, r) => {{ {}let h = \"\";\n{body}return h; }} }}",
                    deps.iter().map(|d| js_str(d)).collect::<Vec<_>>().join(", "),
                    Self::destructure(scope, "c")
                )
            }
        };
        self.blocks.insert(key.clone(), entry);
        Ok(key)
    }

    fn handler(&mut self, el: usize, attr: usize, name: &str, js: &str, _pos: usize, scope: &Scope) -> Result<String, Diag> {
        if let Some(k) = self.handler_keys.get(&(el, attr)) {
            return Ok(k.clone());
        }
        let key = format!("h{}", self.handlers.len());
        let names: Vec<String> = self.state.iter().map(|(n, _)| n.clone()).collect();
        let shadowed: Vec<String> = names.into_iter().filter(|n| lookup(scope, n).is_none()).collect();
        let body = script::rewrite_state(js.trim(), &shadowed);
        self.handlers.insert(key.clone(), format!("(c) => {{ {}return ({body}); }}", Self::destructure(scope, "c")));
        self.handler_keys.insert((el, attr), key.clone());
        self.events.insert(name[2..].to_owned());
        Ok(key)
    }

    fn js_nodes(&mut self, nodes: &[Node], scope: &Scope, cvar: &str, out: &mut String) -> Result<(), Diag> {
        for n in nodes {
            self.js_node(n, scope, cvar, out)?;
        }
        Ok(())
    }

    fn js_node(&mut self, node: &Node, scope: &Scope, cvar: &str, out: &mut String) -> Result<(), Diag> {
        if let Node::Text(t) = node {
            let _ = writeln!(out, "h += {};", js_str(t));
            return Ok(());
        }
        if let Node::Slot { pos } = node {
            return Err(Diag::new(*pos, "`<slot>` is only valid in `layout.rk`"));
        }
        if let Node::Source { load: Load::Server, pos, .. } = node {
            return Err(Diag::new(*pos, "a `load=\"server\"` source cannot appear inside a region the browser renders")
                .help("this region depends on browser state, so it may first appear after the HTTP response; use `load=\"client\"` here"));
        }
        if self.is_dynamic(node, scope) {
            let key = self.js_block(node, scope)?;
            let _ = writeln!(out, "h += r.block(\"{key}\", {cvar});");
            return Ok(());
        }
        self.js_inner(node, scope, cvar, out)
    }

    fn js_inner(&mut self, node: &Node, scope: &Scope, cvar: &str, out: &mut String) -> Result<(), Diag> {
        match node {
            Node::Text(_) | Node::Source { .. } | Node::Slot { .. } => unreachable!(),
            Node::Expr { expr, .. } => {
                let _ = writeln!(out, "h += r.esc({});", self.js_val(expr, scope)?);
            }
            Node::Element(e) => self.js_element(e, scope, cvar, out)?,
            Node::If { branches, els, .. } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "} else if" };
                    let _ = writeln!(out, "{kw} ({}) {{", self.js_val(cond, scope)?);
                    self.js_nodes(body, scope, cvar, out)?;
                }
                if let Some(body) = els {
                    out.push_str("} else {\n");
                    self.js_nodes(body, scope, cvar, out)?;
                }
                out.push_str("}\n");
            }
            Node::Each { expr, item, index, body, pos, .. } => {
                if !matches!(expr, Expr::Path { .. }) {
                    return Err(Diag::new(*pos, "`{#each}` needs a list field, such as `data.items`"));
                }
                self.tmp += 1;
                let c2 = format!("c{}", self.tmp);
                let i = index.clone().unwrap_or_else(|| format!("i{}", self.tmp));
                let _ = writeln!(out, "for (const [{i}, {item}] of Array.from({} ?? []).entries()) {{", self.js_val(expr, scope)?);
                let mut inner = scope.clone();
                inner.push((item.clone(), Binding::Loop));
                let mut bound = vec![item.as_str()];
                if let Some(ix) = index {
                    inner.push((ix.clone(), Binding::Loop));
                    bound.push(ix);
                }
                let _ = writeln!(out, "const {c2} = {{ ...{cvar}, {} }};", bound.join(", "));
                self.js_nodes(body, &inner, &c2, out)?;
                out.push_str("}\n");
            }
        }
        Ok(())
    }

    fn js_element(&mut self, e: &Element, scope: &Scope, cvar: &str, out: &mut String) -> Result<(), Diag> {
        let _ = writeln!(out, "h += \"<{}\";", e.tag);
        let mut handlers = Vec::new();
        for (ai, a) in e.attrs.iter().enumerate() {
            match &a.value {
                AttrValue::Bare => {
                    let _ = writeln!(out, "h += {};", js_str(&format!(" {}", a.name)));
                }
                AttrValue::Static(v) => {
                    let _ = writeln!(out, "h += {};", js_str(&static_attr(&a.name, v)));
                }
                AttrValue::Parts(parts) => {
                    if let [Part::Expr(x)] = parts.as_slice() {
                        let _ = writeln!(out, "h += r.attr({}, {});", js_str(&a.name), self.js_val(x, scope)?);
                    } else {
                        let _ = writeln!(out, "h += {};", js_str(&format!(" {}=\"", a.name)));
                        for p in parts {
                            match p {
                                Part::Text(t) => {
                                    let _ = writeln!(out, "h += {};", js_str(&t.replace('"', "&quot;")));
                                }
                                Part::Expr(x) => {
                                    let _ = writeln!(out, "h += r.esc({});", self.js_val(x, scope)?);
                                }
                            }
                        }
                        out.push_str("h += '\"';\n");
                    }
                }
                AttrValue::Handler { js, pos } => {
                    let key = self.handler(e.id, ai, &a.name, js, *pos, scope)?;
                    handlers.push(format!("r.on({}, \"{key}\", {cvar})", js_str(&a.name[2..])));
                }
            }
        }
        if !handlers.is_empty() {
            let _ = writeln!(out, "h += ' data-rk-on=\"' + [{}].join(\" \") + '\"';", handlers.join(", "));
        }
        out.push_str("h += \">\";\n");
        if is_post_form(e) {
            self.csrf_in_js = true;
            out.push_str("h += '<input type=\"hidden\" name=\"_csrf\" value=\"' + r.esc(r.csrf()) + '\">';\n");
        }
        if !e.void {
            self.js_nodes(&e.children, scope, cvar, out)?;
            let _ = writeln!(out, "h += \"</{}>\";", e.tag);
        }
        Ok(())
    }

    // -------------------------------------------------------------- output

    fn rust_module(&self, body: &str, needs_js: bool, title: &str) -> String {
        let st = self.opts.state_type;
        let mut out = String::new();
        if let Some(name) = self.param {
            let ident = raw_ident(name);
            let _ = writeln!(out, "pub struct Params {{\n    pub {ident}: ::std::string::String,\n}}\n");
        }
        let _ = writeln!(
            out,
            "pub async fn render(ctx: &::rustkit::RequestContext<{st}>, __param: Option<&str>) -> ::std::result::Result<::rustkit::Page, ::rustkit::Error> {{"
        );
        if let Some(name) = self.param {
            let ident = raw_ident(name);
            let _ = writeln!(out, "let params = Params {{ {ident}: __param.expect(\"route param\").to_owned() }};");
        }
        out.push_str("let __t0 = ::std::time::Instant::now();\n");
        if !self.server_sources.is_empty() {
            let names: Vec<String> = self.server_sources.iter().map(|s| format!("src_{s}")).collect();
            let calls: Vec<String> = self.server_sources.iter().map(|s| self.source_call(s)).collect();
            // Independent reads start together; each source is read once per request.
            let _ = writeln!(out, "let ({},) = ::rustkit::__private::try_join!({})?;", names.join(", "), calls.join(", "));
        }
        out.push_str("let __sources_time = __t0.elapsed();\nlet __t1 = ::std::time::Instant::now();\n");
        out.push_str("let mut out = ::std::string::String::with_capacity(4096);\n");
        out.push_str(&coalesce(body, "out.push_str(\"", "\");"));

        let client_param = self.param.is_some() && self.client_sources.iter().any(|s| self.param_sources.contains(s));
        let param_payload = self.param.is_some() && (self.param_in_js || client_param);
        if needs_js && (!self.projections.is_empty() || self.csrf_in_js || param_payload) {
            out.push_str("let __payload = {\nuse ::rustkit::__private::serde_json::{Map, Value, to_value};\nlet mut s = Map::new();\n");
            for (src, proj) in &self.projections {
                match proj {
                    Projection::None => {}
                    Projection::All => {
                        let _ = writeln!(out, "s.insert({}.into(), to_value(&src_{src})?);", rs_str(src));
                    }
                    Projection::Fields(fields) => {
                        out.push_str("{ let mut m = Map::new();\n");
                        for f in fields {
                            let _ = writeln!(out, "m.insert({}.into(), to_value(&src_{src}.{})?);", rs_str(f), snake(f));
                        }
                        let _ = writeln!(out, "s.insert({}.into(), Value::Object(m)); }}", rs_str(src));
                    }
                }
            }
            out.push_str("let mut p = Map::new();\np.insert(\"s\".into(), Value::Object(s));\n");
            if self.csrf_in_js {
                out.push_str("p.insert(\"csrf\".into(), Value::String(ctx.csrf_token()));\n");
            }
            if param_payload {
                let name = self.param.unwrap();
                let ident = raw_ident(name);
                let _ = writeln!(
                    out,
                    "p.insert(\"params\".into(), {{ let mut m = Map::new(); m.insert({}.into(), Value::String(params.{ident}.clone())); Value::Object(m) }});",
                    rs_str(name)
                );
            }
            out.push_str("Some(::rustkit::js::html_safe_json(&Value::Object(p)))\n};\n");
        } else {
            out.push_str("let __payload: Option<String> = None;\n");
        }
        let _ = writeln!(out, "let html = ::rustkit::document({}, CSS, &out, __payload.as_deref(), JS);", rs_str(title));
        out.push_str("Ok(::rustkit::Page { html, sources_time: __sources_time, render_time: __t1.elapsed() })\n}\n");
        let _ = writeln!(
            out,
            "pub fn render_boxed<'a>(ctx: &'a ::rustkit::RequestContext<{st}>, __param: Option<&'a str>) -> ::rustkit::BoxFut<'a, ::std::result::Result<::rustkit::Page, ::rustkit::Error>> {{ Box::pin(render(ctx, __param)) }}"
        );
        out
    }

    fn js_module(&self, user_js: &str) -> String {
        let mut out = String::new();
        out.push_str("import { state, start } from \"./__RUNTIME__\";\n");
        let init: Vec<String> = self.state.iter().map(|(n, v)| format!("{}: {}", js_str(n), self.js_val(v, &Vec::new()).unwrap())).collect();
        let _ = writeln!(out, "const $s = state({{ {} }});", init.join(", "));
        if !user_js.trim().is_empty() {
            out.push_str("// page script\n");
            out.push_str(user_js.trim());
            out.push('\n');
        }
        out.push_str("// compiled template\nstart($s, {\n");
        let _ = writeln!(out, "events: [{}],", self.events.iter().map(|e| js_str(e)).collect::<Vec<_>>().join(", "));
        out.push_str("blocks: {\n");
        for (k, v) in &self.blocks {
            let _ = writeln!(out, "{k}: {},", coalesce(v, "h += \"", "\";").trim_end());
        }
        out.push_str("},\nhandlers: {\n");
        for (k, v) in &self.handlers {
            let _ = writeln!(out, "{k}: {v},");
        }
        out.push_str("},\nssr: {\n");
        for (k, v) in &self.ssr_scopes {
            let _ = writeln!(out, "{k}: {v},");
        }
        out.push_str("},\n});\n");
        out
    }
}

//! Shared components.
//!
//! A page or layout imports one with `import Header from "$lib/Header.rk"`
//! (a file in `src/lib`, next to `src/routes`) or a relative path ending in
//! `.rk`. The tag `<Header>` is replaced with the component's template.
//! Props are the attributes the component reads. A single `<slot>` receives
//! the tag's children. `$state`, source imports and CSS are merged into the
//! page the way a layout's are: one `$state` name per composed page, shared
//! by every use of that component.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Diag;
use crate::expr::Expr;
use crate::parse::{AttrValue, Node, Part};
use crate::script::{self, ComponentImport};

#[derive(Clone)]
pub struct CompImport {
    pub name: String,
    pub path: PathBuf,
    pub spec: String,
    pub pos: usize,
}

pub struct Component {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub display: String,
    pub src: String,
    pub nodes: Vec<Node>,
    pub script_body: String,
    pub style: Option<String>,
    pub style_global: bool,
    pub style_at: Option<usize>,
    /// `$state` names declared in this file, in source order.
    pub state: Vec<String>,
    /// Sources this file imports directly.
    pub sources: Vec<String>,
    /// Names the template reads that are not state or a local binding.
    pub props: BTreeSet<String>,
    /// Identifiers handlers read that are not state or a local binding.
    pub handler_names: BTreeSet<String>,
    /// State names in this component and the ones it inlines.
    pub protected: BTreeSet<String>,
    pub has_slot: bool,
    pub imports: Vec<CompImport>,
    /// Nested components first, then this file. Filled by [`Set::inline`].
    pub chain: Vec<PathBuf>,
}

pub struct Set {
    pub by_path: BTreeMap<PathBuf, Component>,
}

/// `$lib` is the `lib` directory next to the routes directory (`src/lib`
/// when routes live in `src/routes`).
pub fn lib_dir(routes_dir: &Path) -> PathBuf {
    routes_dir.parent().unwrap_or(routes_dir).join("lib")
}

pub fn display_path(routes_dir: &Path, path: &Path) -> String {
    let root = routes_dir.parent().unwrap_or(routes_dir);
    if let Ok(rel) = path.strip_prefix(root) { if root.file_name().is_some_and(|n| n == "src") { format!("src/{}", rel.display()) } else { rel.display().to_string() } } else { path.display().to_string() }
}

pub fn resolve(from_file: &Path, spec: &str, lib: &Path, pos: usize) -> Result<PathBuf, Diag> {
    let path = if let Some(rest) = spec.strip_prefix("$lib/") {
        if rest.is_empty() || rest.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
            return Err(Diag::new(pos, format!("`{spec}` is not a path inside `$lib`")).help("`$lib` is `src/lib`, next to `src/routes`. Write `$lib/Header.rk` or `$lib/ui/Card.rk`"));
        }
        lib.join(rest)
    } else if spec.starts_with("./") || spec.starts_with("../") {
        normalize(&from_file.parent().unwrap_or(Path::new("")).join(spec))
    } else {
        return Err(Diag::new(pos, format!("`{spec}` is not a component path")).help("use `$lib/Header.rk` for a file in `src/lib`, or a relative path ending in `.rk`"));
    };
    if path.extension().is_none_or(|e| e != "rk") {
        return Err(Diag::new(pos, format!("`{spec}` is not a component path")).help("component files end in `.rk`"));
    }
    Ok(path)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Loads every component reachable from `roots` (route-file imports).
/// Nodes are still in each file's own coordinates; the caller shifts them
/// into the diagnostic buffer and then calls [`Set::inline`].
pub fn load(routes_dir: &Path, roots: &[(PathBuf, CompImport)], errors: &mut String) -> Set {
    let lib = lib_dir(routes_dir);
    let mut by_path = BTreeMap::new();
    let mut stack = Vec::new();
    for (from, import) in roots {
        let as_script = ComponentImport { name: import.name.clone(), spec: import.spec.clone(), pos: import.pos };
        if let Err(d) = load_one(routes_dir, &lib, from, &as_script, Some(import.path.clone()), &mut by_path, &mut stack, errors) {
            errors.push_str(&d);
        }
    }
    Set { by_path }
}

fn load_one(routes_dir: &Path, lib: &Path, from: &Path, import: &ComponentImport, resolved: Option<PathBuf>, by_path: &mut BTreeMap<PathBuf, Component>, stack: &mut Vec<PathBuf>, errors: &mut String) -> Result<(), String> {
    let path = match resolved {
        Some(path) => path,
        None => resolve(from, &import.spec, lib, import.pos).map_err(|d| {
            let src = fs::read_to_string(from).unwrap_or_default();
            let display = display_path(routes_dir, from);
            d.render(&display, &src)
        })?,
    };
    if stack.iter().any(|p| p == &path) {
        let cycle = stack_display(routes_dir, stack, &path);
        let src = fs::read_to_string(from).unwrap_or_default();
        let d = Diag::new(import.pos, format!("component cycle: {cycle}")).help("a component cannot import itself, even through another component");
        return Err(d.render(&display_path(routes_dir, from), &src));
    }
    if by_path.contains_key(&path) {
        return Ok(());
    }
    let src = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            let from_src = fs::read_to_string(from).unwrap_or_default();
            let d = Diag::new(import.pos, format!("cannot read component `{}`", import.spec)).help(format!("{e}. `$lib` is `src/lib`, next to `src/routes`"));
            return Err(d.render(&display_path(routes_dir, from), &from_src));
        }
    };
    let display = display_path(routes_dir, &path);
    let page = match crate::parse::parse(&src) {
        Ok(p) => p,
        Err(d) => return Err(d.render(&display, &src)),
    };
    if let Some(title_at) = page.title.as_ref().map(|_| src.find("<title>").unwrap_or(0)) {
        let d = Diag::new(title_at, "a component cannot set `<title>`").help("put the title on the page, or on a layout");
        return Err(d.render(&display, &src));
    }
    if let Some((body, pos)) = &page.module_script {
        if let Some(i) = body.find("export const prerender = true") {
            let d = Diag::new(pos + i, "`prerender` belongs on the page, not on a component");
            return Err(d.render(&display, &src));
        }
    }
    let slots = slot_positions(&page.nodes);
    if slots.len() > 1 {
        let d = Diag::new(slots[1], "a component can have only one `<slot>`");
        return Err(d.render(&display, &src));
    }
    let (script_body, state, sources, imports) = match &page.script {
        Some((body, pos)) => match script::parse(body, *pos) {
            Ok(s) => (script_without_components(body), s.state.into_iter().map(|(n, _)| n).collect::<Vec<_>>(), s.sources, s.components),
            Err(d) => return Err(d.render(&display, &src)),
        },
        None => (String::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    let state_set: BTreeSet<String> = state.iter().cloned().collect();
    let (props, handler_names) = free_names(&page.nodes, &state_set);
    // Insert before walking imports so a cycle can see this file.
    let has_slot = !slots.is_empty();
    by_path.insert(
        path.clone(),
        Component {
            path: path.clone(),
            display: display.clone(),
            src,
            nodes: page.nodes,
            script_body,
            style: page.style,
            style_global: page.style_global,
            style_at: page.style_at,
            state,
            sources,
            props,
            handler_names,
            protected: state_set,
            has_slot,
            imports: Vec::new(),
            chain: vec![path.clone()],
        },
    );
    stack.push(path.clone());
    let mut resolved = Vec::new();
    for nested in imports {
        match load_one(routes_dir, lib, &path, &nested, None, by_path, stack, errors) {
            Ok(()) => {
                if let Ok(nested_path) = resolve(&path, &nested.spec, lib, nested.pos) {
                    resolved.push(CompImport { name: nested.name, path: nested_path, spec: nested.spec, pos: nested.pos });
                }
            }
            Err(msg) => errors.push_str(&msg),
        }
    }
    stack.pop();
    if let Some(c) = by_path.get_mut(&path) {
        c.imports = resolved;
    }
    Ok(())
}

fn stack_display(routes_dir: &Path, stack: &[PathBuf], next: &Path) -> String {
    let mut parts: Vec<String> = stack.iter().map(|p| format!("`{}`", display_path(routes_dir, p))).collect();
    parts.push(format!("`{}`", display_path(routes_dir, next)));
    parts.join(" imports ")
}

fn script_without_components(src: &str) -> String {
    let mut out = String::new();
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        let component = t.starts_with("import ") && !t.contains("$sources") && (t.contains("$lib/") || t.contains(".rk"));
        if !component {
            out.push_str(line);
        }
    }
    out
}

impl Set {
    /// Inlines nested components into each component body. Call after the
    /// caller has shifted every component's nodes into the diagnostic buffer.
    pub fn inline(&mut self, css_scope: bool) -> Result<(), Vec<Diag>> {
        let paths: Vec<PathBuf> = self.by_path.keys().cloned().collect();
        let mut done = BTreeSet::new();
        let mut diags = Vec::new();
        for path in paths {
            self.inline_rec(&path, css_scope, &mut done, &mut Vec::new(), &mut diags);
        }
        if diags.is_empty() { Ok(()) } else { Err(diags) }
    }

    fn inline_rec(&mut self, path: &Path, css_scope: bool, done: &mut BTreeSet<PathBuf>, stack: &mut Vec<PathBuf>, diags: &mut Vec<Diag>) {
        if done.contains(path) || !self.by_path.contains_key(path) {
            return;
        }
        if stack.iter().any(|p| p == path) {
            diags.push(Diag::new(0, "component cycle"));
            return;
        }
        stack.push(path.to_path_buf());
        let deps: Vec<PathBuf> = self.by_path.get(path).map(|c| c.imports.iter().map(|i| i.path.clone()).collect()).unwrap_or_default();
        for dep in &deps {
            self.inline_rec(dep, css_scope, done, stack, diags);
        }
        stack.pop();
        let imports = match self.by_path.get(path) {
            Some(c) => c.imports.clone(),
            None => return,
        };
        let mut nodes = match self.by_path.get_mut(path) {
            Some(c) => std::mem::take(&mut c.nodes),
            None => return,
        };
        expand_nodes(&mut nodes, &imports, self, diags);
        let mut chain = Vec::new();
        let mut protected = BTreeSet::new();
        for dep in &deps {
            if let Some(nested) = self.by_path.get(dep) {
                for p in &nested.chain {
                    if !chain.contains(p) {
                        if let Some(n) = self.by_path.get(p) {
                            protected.extend(n.state.iter().cloned());
                        }
                        chain.push(p.clone());
                    }
                }
            }
        }
        chain.push(path.to_path_buf());
        if let Some(c) = self.by_path.get_mut(path) {
            protected.extend(c.state.iter().cloned());
            c.nodes = nodes;
            c.chain = chain;
            c.protected = protected;
            if css_scope {
                if let Err(err) = crate::css::apply(&mut c.style, c.style_global, &mut c.nodes) {
                    let pos = c.style_at.unwrap_or(0) + err.pos;
                    diags.push(Diag::new(pos, err.msg).help("selectors are scoped to this file. Wrap one in `:global(...)`, or mark the block `<style global>`"));
                }
            }
        }
        done.insert(path.to_path_buf());
    }

    /// Replaces `<Name>` in a route file with that component's template.
    pub fn expand(&self, nodes: &mut Vec<Node>, imports: &[CompImport], diags: &mut Vec<Diag>) {
        expand_nodes(nodes, imports, self, diags);
    }
}

fn expand_nodes(nodes: &mut Vec<Node>, imports: &[CompImport], set: &Set, diags: &mut Vec<Diag>) {
    let mut i = 0;
    while i < nodes.len() {
        expand_inside(&mut nodes[i], imports, set, diags);
        let rendered = match &nodes[i] {
            Node::Use { name, attrs, children, pos, .. } => render_use(name, attrs, children, *pos, imports, set, diags),
            _ => None,
        };
        if let Some(rendered) = rendered {
            let mut rest = nodes.split_off(i);
            rest.remove(0);
            let n = rendered.len();
            nodes.extend(rendered);
            nodes.extend(rest);
            i += n;
        } else {
            i += 1;
        }
    }
}

fn expand_inside(node: &mut Node, imports: &[CompImport], set: &Set, diags: &mut Vec<Diag>) {
    match node {
        Node::Element(e) => expand_nodes(&mut e.children, imports, set, diags),
        Node::If { branches, els, .. } => {
            for (_, body) in branches {
                expand_nodes(body, imports, set, diags);
            }
            if let Some(body) = els {
                expand_nodes(body, imports, set, diags);
            }
        }
        Node::Each { body, .. } | Node::Source { body, .. } => expand_nodes(body, imports, set, diags),
        Node::Use { children, .. } => expand_nodes(children, imports, set, diags),
        Node::Text(_) | Node::Expr { .. } | Node::Slot { .. } => {}
    }
}

fn render_use(name: &str, attrs: &[crate::parse::Attr], children: &[Node], pos: usize, imports: &[CompImport], set: &Set, diags: &mut Vec<Diag>) -> Option<Vec<Node>> {
    let Some(import) = imports.iter().find(|i| i.name == name) else {
        let known: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
        let help =
            if known.is_empty() { format!("add `import {name} from \"$lib/{name}.rk\"` on its own line") } else { format!("this file imports {}. Add `import {name} from \"$lib/{name}.rk\"`, or use one of those tags", known.join(", ")) };
        diags.push(Diag::new(pos, format!("unknown component `<{name}>`")).help(help));
        return Some(Vec::new());
    };
    let Some(comp) = set.by_path.get(&import.path) else {
        diags.push(Diag::new(pos, format!("unknown component `<{name}>`")));
        return Some(Vec::new());
    };
    let mut provided: BTreeMap<String, Expr> = BTreeMap::new();
    for a in attrs {
        if matches!(a.value, AttrValue::Handler { .. }) || is_rejected_event(&a.name) {
            diags.push(Diag::new(pos, format!("`<{name}>` does not take `{0}`", a.name)).help("write the handler inside the component; component tags take props, not events"));
            continue;
        }
        if comp.protected.contains(&a.name) {
            diags.push(Diag::new(pos, format!("`{}` is `$state` in `{}`, not a prop", a.name, comp.display)));
            continue;
        }
        if provided.contains_key(&a.name) {
            diags.push(Diag::new(pos, format!("prop `{}` is set twice on `<{name}>`", a.name)));
            continue;
        }
        match &a.value {
            AttrValue::Bare => {
                diags.push(Diag::new(pos, format!("prop `{}` needs a value", a.name)).help(format!("write `{0}=\"…\"` or `{0}={{expr}}`", a.name)));
            }
            AttrValue::Static(s) => {
                provided.insert(a.name.clone(), Expr::Str(s.clone()));
            }
            AttrValue::Parts(parts) => match parts.as_slice() {
                [Part::Expr(e)] => {
                    provided.insert(a.name.clone(), e.clone());
                }
                _ => {
                    diags.push(Diag::new(pos, format!("prop `{}` must be a string or one `{{expression}}`", a.name)));
                }
            },
            AttrValue::Handler { .. } => {}
        }
    }
    for prop in &comp.props {
        if !provided.contains_key(prop) {
            diags.push(Diag::new(pos, format!("`<{name}>` needs a `{prop}` prop")).help(format!("write `<{name} {prop}=\"…\">` or `<{name} {prop}={{expr}}>`")));
        }
    }
    for name_attr in provided.keys() {
        if !comp.props.contains(name_attr) && !comp.handler_names.contains(name_attr) {
            diags.push(Diag::new(pos, format!("`<{name}>` has no prop `{name_attr}`")).help("a prop is a name the component's template reads"));
        }
    }
    if !comp.has_slot && meaningful(children) {
        diags.push(Diag::new(pos, format!("`<{name}>` has no `<slot>` for its contents")).help("put `<slot />` in the component where the children should go, or remove the children"));
        return Some(Vec::new());
    }
    let mut nodes = comp.nodes.clone();
    if let Err(d) = subst_nodes(&mut nodes, &provided, &comp.protected, &mut Vec::new()) {
        diags.push(d);
    }
    if comp.has_slot {
        let mut child = Some(children.to_vec());
        if !fill_slot(&mut nodes, &mut child) {
            diags.push(Diag::new(pos, format!("`<{name}>` has a `<slot>`, but it could not be filled")));
        }
    }
    Some(nodes)
}

fn is_rejected_event(name: &str) -> bool {
    matches!(
        name,
        "onclick"
            | "ondblclick"
            | "oninput"
            | "onchange"
            | "onsubmit"
            | "onreset"
            | "oninvalid"
            | "onkeydown"
            | "onkeyup"
            | "onkeypress"
            | "onfocus"
            | "onblur"
            | "onmouseenter"
            | "onmouseleave"
            | "onmouseover"
            | "onmouseout"
            | "onmousedown"
            | "onmouseup"
            | "onpointerdown"
            | "onpointerup"
            | "oncontextmenu"
            | "onscroll"
            | "onwheel"
            | "ondragstart"
            | "ondrop"
    )
}

fn meaningful(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Text(t) => !t.trim().is_empty(),
        _ => true,
    })
}

fn subst_nodes(nodes: &mut [Node], props: &BTreeMap<String, Expr>, protected: &BTreeSet<String>, bound: &mut Vec<String>) -> Result<(), Diag> {
    for n in nodes {
        match n {
            Node::Text(_) | Node::Slot { .. } => {}
            Node::Expr { expr, .. } => subst_expr(expr, props, protected, bound)?,
            Node::Element(e) => {
                for a in &mut e.attrs {
                    match &mut a.value {
                        AttrValue::Parts(parts) => {
                            for p in parts {
                                if let Part::Expr(e) = p {
                                    subst_expr(e, props, protected, bound)?;
                                }
                            }
                        }
                        AttrValue::Handler { js, pos } => {
                            *js = rewrite_handler(js, props, *pos)?;
                        }
                        AttrValue::Bare | AttrValue::Static(_) => {}
                    }
                }
                subst_nodes(&mut e.children, props, protected, bound)?;
            }
            Node::If { branches, els, .. } => {
                for (cond, body) in branches {
                    subst_expr(cond, props, protected, bound)?;
                    subst_nodes(body, props, protected, bound)?;
                }
                if let Some(body) = els {
                    subst_nodes(body, props, protected, bound)?;
                }
            }
            Node::Each { expr, item, index, body, .. } => {
                subst_expr(expr, props, protected, bound)?;
                let n = bound.len();
                bound.push(item.clone());
                if let Some(i) = index {
                    bound.push(i.clone());
                }
                subst_nodes(body, props, protected, bound)?;
                bound.truncate(n);
            }
            Node::Source { body, bind, .. } => {
                let n = bound.len();
                bound.push(bind.clone());
                subst_nodes(body, props, protected, bound)?;
                bound.truncate(n);
            }
            Node::Use { .. } => {}
        }
    }
    Ok(())
}

fn subst_expr(expr: &mut Expr, props: &BTreeMap<String, Expr>, protected: &BTreeSet<String>, bound: &[String]) -> Result<(), Diag> {
    match expr {
        Expr::Path { root, segs, pos } => {
            if bound.iter().any(|b| b == root) || !props.contains_key(root) {
                return Ok(());
            }
            if protected.contains(root) {
                return Err(Diag::new(*pos, format!("`{root}` is `$state` in a component, so it cannot be a prop")));
            }
            if !segs.is_empty() {
                return Err(Diag::new(*pos, format!("prop `{root}` has no fields")).help("pass the field from the parent, such as `name={data.name}`"));
            }
            *expr = props[root].clone();
            Ok(())
        }
        Expr::Not(inner) => subst_expr(inner, props, protected, bound),
        Expr::Bin(_, a, b) => {
            subst_expr(a, props, protected, bound)?;
            subst_expr(b, props, protected, bound)
        }
        Expr::Str(_) | Expr::Num(_) | Expr::Bool(_) | Expr::Null => Ok(()),
    }
}

/// Pastes a prop into a handler. Only literals and field paths have a
/// JavaScript spelling that matches the template.
fn rewrite_handler(js: &str, props: &BTreeMap<String, Expr>, pos: usize) -> Result<String, Diag> {
    let mut out = String::with_capacity(js.len());
    let mut last = 0;
    for u in script::idents(js) {
        if u.is_property {
            continue;
        }
        let Some(expr) = props.get(&u.name) else {
            continue;
        };
        if !inlineable(expr) {
            return Err(Diag::new(pos, format!("`{}` in this handler has to be a literal, a variable or a field", u.name)).help("the prop's value is pasted into the handler. Pass `name={user}` or `name=\"Ada\"`, not `name={a && b}`"));
        }
        out.push_str(&js[last..u.start]);
        let src = expr_source(expr);
        if u.shorthand {
            out.push_str(&format!("{}: {src}", u.name));
        } else {
            out.push_str(&src);
        }
        last = u.end;
    }
    out.push_str(&js[last..]);
    Ok(out)
}

fn inlineable(e: &Expr) -> bool {
    matches!(e, Expr::Str(_) | Expr::Num(_) | Expr::Bool(_) | Expr::Null | Expr::Path { .. })
}

fn expr_source(e: &Expr) -> String {
    match e {
        Expr::Str(s) => js_string(s),
        Expr::Num(n) => {
            if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e15 {
                format!("{}", *n as i64)
            } else {
                format!("{n}")
            }
        }
        Expr::Bool(b) => b.to_string(),
        Expr::Null => "null".into(),
        Expr::Path { root, segs, .. } => std::iter::once(root.clone()).chain(segs.iter().cloned()).collect::<Vec<_>>().join("."),
        Expr::Not(inner) => format!("!({})", expr_source(inner)),
        Expr::Bin(op, a, b) => {
            let op = match op {
                crate::expr::Op::And => "&&",
                crate::expr::Op::Or => "||",
                crate::expr::Op::Eq => "===",
                crate::expr::Op::Ne => "!==",
                crate::expr::Op::Lt => "<",
                crate::expr::Op::Le => "<=",
                crate::expr::Op::Gt => ">",
                crate::expr::Op::Ge => ">=",
            };
            format!("({} {op} {})", expr_source(a), expr_source(b))
        }
    }
}

fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn free_names(nodes: &[Node], state: &BTreeSet<String>) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut props = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    let mut bound = Vec::new();
    collect(nodes, state, &mut bound, &mut props, &mut handlers);
    (props, handlers)
}

fn collect(nodes: &[Node], state: &BTreeSet<String>, bound: &mut Vec<String>, props: &mut BTreeSet<String>, handlers: &mut BTreeSet<String>) {
    for n in nodes {
        match n {
            Node::Text(_) | Node::Slot { .. } => {}
            Node::Expr { expr, .. } => expr_roots(expr, state, bound, props),
            Node::Element(e) => {
                walk_attrs(&e.attrs, state, bound, props, handlers);
                collect(&e.children, state, bound, props, handlers);
            }
            Node::If { branches, els, .. } => {
                for (cond, body) in branches {
                    expr_roots(cond, state, bound, props);
                    collect(body, state, bound, props, handlers);
                }
                if let Some(body) = els {
                    collect(body, state, bound, props, handlers);
                }
            }
            Node::Each { expr, item, index, body, .. } => {
                expr_roots(expr, state, bound, props);
                let n = bound.len();
                bound.push(item.clone());
                if let Some(i) = index {
                    bound.push(i.clone());
                }
                collect(body, state, bound, props, handlers);
                bound.truncate(n);
            }
            Node::Source { bind, body, .. } => {
                let n = bound.len();
                bound.push(bind.clone());
                collect(body, state, bound, props, handlers);
                bound.truncate(n);
            }
            Node::Use { attrs, children, .. } => {
                walk_attrs(attrs, state, bound, props, handlers);
                collect(children, state, bound, props, handlers);
            }
        }
    }
}

fn walk_attrs(attrs: &[crate::parse::Attr], state: &BTreeSet<String>, bound: &[String], props: &mut BTreeSet<String>, handlers: &mut BTreeSet<String>) {
    for a in attrs {
        match &a.value {
            AttrValue::Parts(parts) => {
                for p in parts {
                    if let Part::Expr(e) = p {
                        expr_roots(e, state, bound, props);
                    }
                }
            }
            AttrValue::Handler { js, .. } => {
                for u in script::idents(js) {
                    if !u.is_property && !bound.iter().any(|b| b == &u.name) && !state.contains(&u.name) {
                        handlers.insert(u.name.clone());
                    }
                }
            }
            AttrValue::Bare | AttrValue::Static(_) => {}
        }
    }
}

fn expr_roots(expr: &Expr, state: &BTreeSet<String>, bound: &[String], props: &mut BTreeSet<String>) {
    let mut paths = Vec::new();
    expr.paths(&mut paths);
    for (root, _) in paths {
        if !bound.iter().any(|b| b == &root) && !state.contains(&root) {
            props.insert(root);
        }
    }
}

fn slot_positions(nodes: &[Node]) -> Vec<usize> {
    let mut out = Vec::new();
    slot_into(nodes, &mut out);
    out
}

fn slot_into(nodes: &[Node], out: &mut Vec<usize>) {
    for n in nodes {
        match n {
            Node::Slot { pos } => out.push(*pos),
            Node::Element(e) => slot_into(&e.children, out),
            Node::If { branches, els, .. } => {
                for (_, body) in branches {
                    slot_into(body, out);
                }
                if let Some(body) = els {
                    slot_into(body, out);
                }
            }
            Node::Each { body, .. } | Node::Source { body, .. } | Node::Use { children: body, .. } => slot_into(body, out),
            Node::Text(_) | Node::Expr { .. } => {}
        }
    }
}

fn fill_slot(nodes: &mut Vec<Node>, child: &mut Option<Vec<Node>>) -> bool {
    let mut i = 0;
    while i < nodes.len() {
        if matches!(nodes[i], Node::Slot { .. }) {
            let kids = child.take().unwrap_or_default();
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
            Node::Each { body, .. } | Node::Source { body, .. } | Node::Use { children: body, .. } => fill_slot(body, child),
            _ => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

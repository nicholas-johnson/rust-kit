//! Parses a page file: `<script>`, `<script context="module">`, `<style>`, and
//! the markup template with `{expr}`, `{#if}`, `{#each}` and `<DataSource>`.

use crate::Diag;
use crate::expr::{self, Expr};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Load {
    Server,
    Client,
}

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    Expr {
        id: usize,
        expr: Expr,
    },
    Element(Element),
    If {
        id: usize,
        branches: Vec<(Expr, Vec<Node>)>,
        els: Option<Vec<Node>>,
    },
    Each {
        id: usize,
        expr: Expr,
        item: String,
        index: Option<String>,
        body: Vec<Node>,
        pos: usize,
    },
    Source {
        id: usize,
        source: String,
        load: Load,
        bind: String,
        body: Vec<Node>,
        pos: usize,
    },
    /// `<slot>` in a `layout.rk`. Replaced with the wrapped page when routes are composed.
    Slot {
        pos: usize,
    },
}

#[derive(Debug, Clone)]
pub struct Element {
    pub id: usize,
    pub tag: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
    pub void: bool,
}

#[derive(Debug, Clone)]
pub struct Attr {
    pub name: String,
    pub value: AttrValue,
}

#[derive(Debug, Clone)]
pub enum AttrValue {
    Bare,
    /// Written as-is by the author (entities included).
    Static(String),
    Parts(Vec<Part>),
    /// `on<event>={…}`: arbitrary JavaScript evaluating to a function.
    Handler {
        js: String,
        pos: usize,
    },
}

#[derive(Debug, Clone)]
pub enum Part {
    Text(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Default)]
pub struct PageFile {
    pub script: Option<(String, usize)>,
    pub module_script: Option<(String, usize)>,
    pub style: Option<String>,
    pub title: Option<String>,
    pub nodes: Vec<Node>,
}

const VOID: [&str; 13] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];

enum Stop {
    Eof,
    Close(String, usize),
    Else(String, usize),
    EndIf(usize),
    EndEach(usize),
}

struct Parser<'a> {
    src: &'a str,
    i: usize,
    next_id: usize,
    depth: usize,
    page: PageFile,
}

pub fn parse(src: &str) -> Result<PageFile, Diag> {
    let mut p = Parser {
        src,
        i: 0,
        next_id: 0,
        depth: 0,
        page: PageFile::default(),
    };
    let (nodes, stop) = p.nodes()?;
    match stop {
        Stop::Eof => {}
        Stop::Close(tag, pos) => {
            return Err(Diag::new(pos, format!("unexpected closing tag `</{tag}>`")));
        }
        Stop::Else(_, pos) => return Err(Diag::new(pos, "`{:else}` outside `{#if}`")),
        Stop::EndIf(pos) => return Err(Diag::new(pos, "`{/if}` without `{#if}`")),
        Stop::EndEach(pos) => return Err(Diag::new(pos, "`{/each}` without `{#each}`")),
    }
    let mut page = p.page;
    page.nodes = hoist_title(nodes, &mut page.title);
    Ok(page)
}

fn hoist_title(nodes: Vec<Node>, title: &mut Option<String>) -> Vec<Node> {
    nodes
        .into_iter()
        .filter(|n| match n {
            Node::Element(e) if e.tag == "title" => {
                *title = Some(e.children.iter().filter_map(|c| if let Node::Text(t) = c { Some(t.as_str()) } else { None }).collect());
                false
            }
            _ => true,
        })
        .collect()
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.i..]
    }

    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn skip_ws(&mut self) {
        while self.rest().starts_with(|c: char| c.is_whitespace()) {
            self.i += self.rest().chars().next().unwrap().len_utf8();
        }
    }

    fn nodes(&mut self) -> Result<(Vec<Node>, Stop), Diag> {
        let mut nodes = Vec::new();
        loop {
            let r = self.rest();
            if r.is_empty() {
                return Ok((nodes, Stop::Eof));
            }
            if r.starts_with("<!--") {
                let end = r.find("-->").ok_or_else(|| Diag::new(self.i, "unterminated comment"))?;
                self.i += end + 3;
            } else if r.starts_with("</") {
                let pos = self.i;
                let end = r.find('>').ok_or_else(|| Diag::new(pos, "unterminated closing tag"))?;
                let tag = r[2..end].trim().to_owned();
                self.i += end + 1;
                return Ok((nodes, Stop::Close(tag, pos)));
            } else if r.starts_with('<') && r[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
                if let Some(n) = self.element()? {
                    nodes.push(n);
                }
            } else if r.starts_with('{') {
                let pos = self.i;
                let (inner, inner_pos) = self.braced()?;
                let t = inner.trim_start();
                let lead = inner.len() - t.len();
                let t = t.trim_end();
                if let Some(cond) = t.strip_prefix("#if ") {
                    nodes.push(self.if_block(cond, inner_pos + lead + 4)?);
                } else if let Some(rest) = t.strip_prefix("#each ") {
                    nodes.push(self.each_block(rest, inner_pos + lead + 6, pos)?);
                } else if let Some(rest) = t.strip_prefix(":else") {
                    return Ok((nodes, Stop::Else(rest.trim().to_owned(), inner_pos + lead + 5)));
                } else if t == "/if" {
                    return Ok((nodes, Stop::EndIf(pos)));
                } else if t == "/each" {
                    return Ok((nodes, Stop::EndEach(pos)));
                } else if t.starts_with(['#', '/', ':', '@']) {
                    return Err(Diag::new(pos, format!("unknown block `{{{t}}}`")));
                } else {
                    let id = self.id();
                    nodes.push(Node::Expr {
                        id,
                        expr: expr::parse(&inner, inner_pos)?,
                    });
                }
            } else {
                let end = r.find(['<', '{']).unwrap_or(r.len());
                let end = if end == 0 { 1 } else { end };
                nodes.push(Node::Text(r[..end].to_owned()));
                self.i += end;
            }
        }
    }

    /// Reads `{…}` with balanced braces, skipping JS strings. Returns the inner
    /// text and its byte offset.
    fn braced(&mut self) -> Result<(String, usize), Diag> {
        let start = self.i;
        let b = self.src.as_bytes();
        let mut depth = 0usize;
        let mut j = self.i;
        while j < b.len() {
            match b[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.i = j + 1;
                        return Ok((self.src[start + 1..j].to_owned(), start + 1));
                    }
                }
                q @ (b'"' | b'\'' | b'`') => {
                    j += 1;
                    while j < b.len() && b[j] != q {
                        if b[j] == b'\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        Err(Diag::new(start, "unclosed `{`"))
    }

    fn if_block(&mut self, cond: &str, cond_pos: usize) -> Result<Node, Diag> {
        let id = self.id();
        let mut branches = vec![(expr::parse(cond, cond_pos)?, Vec::new())];
        let mut els = None;
        loop {
            let (body, stop) = self.nodes()?;
            match stop {
                Stop::Else(rest, pos) if els.is_none() => {
                    let target = branches.last_mut().unwrap();
                    target.1 = body;
                    if let Some(c) = rest.strip_prefix("if ") {
                        branches.push((expr::parse(c, pos + 4)?, Vec::new()));
                    } else if rest.is_empty() {
                        els = Some(Vec::new());
                    } else {
                        return Err(Diag::new(pos, "expected `{:else}` or `{:else if …}`"));
                    }
                }
                Stop::EndIf(_) => {
                    match els.as_mut() {
                        Some(e) => *e = body,
                        None => branches.last_mut().unwrap().1 = body,
                    }
                    return Ok(Node::If { id, branches, els });
                }
                _ => return Err(Diag::new(cond_pos, "`{#if}` is not closed with `{/if}`")),
            }
        }
    }

    fn each_block(&mut self, spec: &str, spec_pos: usize, pos: usize) -> Result<Node, Diag> {
        let id = self.id();
        let (list, binding) = spec.rsplit_once(" as ").ok_or_else(|| Diag::new(spec_pos, "expected `{#each list as item}`"))?;
        let expr = expr::parse(list, spec_pos)?;
        let mut names = binding.split(',').map(|s| s.trim().to_owned());
        let item = names.next().filter(|s| is_ident(s)).ok_or_else(|| Diag::new(spec_pos, "invalid item name"))?;
        let index = names.next();
        if index.as_deref().is_some_and(|s| !is_ident(s)) {
            return Err(Diag::new(spec_pos, "invalid index name"));
        }
        let (body, stop) = self.nodes()?;
        if !matches!(stop, Stop::EndEach(_)) {
            return Err(Diag::new(pos, "`{#each}` is not closed with `{/each}`"));
        }
        Ok(Node::Each { id, expr, item, index, body, pos })
    }

    fn element(&mut self) -> Result<Option<Node>, Diag> {
        let pos = self.i;
        self.i += 1;
        let r = self.rest();
        let name_len = r.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(r.len());
        let tag = r[..name_len].to_owned();
        self.i += name_len;

        let mut attrs = Vec::new();
        let self_closing;
        loop {
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("/>") {
                self.i += 2;
                self_closing = true;
                break;
            }
            if r.starts_with('>') {
                self.i += 1;
                self_closing = false;
                break;
            }
            if r.is_empty() {
                return Err(Diag::new(pos, format!("unterminated `<{tag}>`")));
            }
            attrs.push(self.attr()?);
        }

        if (tag == "script" || tag == "style") && !self_closing {
            let close = format!("</{tag}>");
            let end = self.rest().find(&close).ok_or_else(|| Diag::new(pos, format!("unclosed `<{tag}>`")))?;
            let body = self.rest()[..end].to_owned();
            let body_pos = self.i;
            self.i += end + close.len();
            if self.depth > 0 {
                return Err(Diag::new(pos, format!("`<{tag}>` must be at the top level of a page")));
            }
            let module = attrs.iter().any(|a| a.name == "context" && matches!(&a.value, AttrValue::Static(v) if v == "module"));
            let slot = match (tag.as_str(), module) {
                ("style", _) => {
                    self.page.style = Some(body);
                    return Ok(None);
                }
                (_, true) => &mut self.page.module_script,
                (_, false) => &mut self.page.script,
            };
            if slot.is_some() {
                return Err(Diag::new(pos, "a page can have only one script of each kind"));
            }
            *slot = Some((body, body_pos));
            return Ok(None);
        }

        let void = VOID.contains(&tag.as_str());
        let children = if self_closing || void {
            Vec::new()
        } else {
            self.depth += 1;
            let (children, stop) = self.nodes()?;
            self.depth -= 1;
            match stop {
                Stop::Close(t, _) if t == tag => children,
                Stop::Close(t, p) => {
                    return Err(Diag::new(p, format!("`</{t}>` does not match `<{tag}>`")));
                }
                _ => return Err(Diag::new(pos, format!("`<{tag}>` is not closed"))),
            }
        };

        if tag == "slot" {
            if !attrs.is_empty() {
                return Err(Diag::new(pos, "`<slot>` takes no attributes"));
            }
            if !children.is_empty() {
                return Err(Diag::new(pos, "`<slot>` cannot wrap content").help("the layout's `<slot>` is where the page is rendered"));
            }
            return Ok(Some(Node::Slot { pos }));
        }
        if tag.starts_with(|c: char| c.is_ascii_uppercase()) {
            return self.component(tag, attrs, children, pos).map(Some);
        }
        let id = self.id();
        Ok(Some(Node::Element(Element { id, tag, attrs, children, void })))
    }

    fn component(&mut self, tag: String, attrs: Vec<Attr>, body: Vec<Node>, pos: usize) -> Result<Node, Diag> {
        if tag != "DataSource" {
            return Err(Diag::new(pos, format!("unknown component `<{tag}>`")).help("the first release has one built-in component, `<DataSource>`"));
        }
        let mut source = None;
        let mut load = None;
        let mut bind = "data".to_owned();
        for a in attrs {
            match (a.name.as_str(), a.value) {
                ("source", AttrValue::Parts(p)) => match p.as_slice() {
                    [Part::Expr(Expr::Path { root, segs, .. })] if segs.is_empty() => source = Some(root.clone()),
                    _ => {
                        return Err(Diag::new(pos, "`source` must name an import from \"$sources\""));
                    }
                },
                ("load", AttrValue::Static(v)) => {
                    load = Some(match v.as_str() {
                        "server" => Load::Server,
                        "client" => Load::Client,
                        _ => return Err(Diag::new(pos, "`load` must be \"server\" or \"client\"")),
                    })
                }
                (n, AttrValue::Bare) if n.starts_with("let:") => bind = n[4..].to_owned(),
                (n, _) => {
                    return Err(Diag::new(pos, format!("unexpected attribute `{n}` on <DataSource>")));
                }
            }
        }
        let source = source.ok_or_else(|| Diag::new(pos, "<DataSource> needs `source={…}`"))?;
        let load = load.ok_or_else(|| Diag::new(pos, "<DataSource> needs `load=\"server\"` or `load=\"client\"`"))?;
        if !is_ident(&bind) {
            return Err(Diag::new(pos, "invalid `let:` name"));
        }
        let id = self.id();
        Ok(Node::Source { id, source, load, bind, body, pos })
    }

    fn attr(&mut self) -> Result<Attr, Diag> {
        let pos = self.i;
        let r = self.rest();
        let len = r.find(|c: char| c.is_whitespace() || c == '=' || c == '>' || c == '/').unwrap_or(r.len());
        if len == 0 {
            return Err(Diag::new(pos, "expected an attribute name"));
        }
        let name = r[..len].to_owned();
        self.i += len;
        let is_handler = name.len() > 2 && name.starts_with("on");
        if !self.rest().starts_with('=') {
            return Ok(Attr { name, value: AttrValue::Bare });
        }
        self.i += 1;
        let r = self.rest();
        let value = if r.starts_with('{') {
            let (inner, p) = self.braced()?;
            if is_handler {
                AttrValue::Handler { js: inner, pos: p }
            } else {
                AttrValue::Parts(vec![Part::Expr(expr::parse(&inner, p)?)])
            }
        } else if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
            let end = r[1..].find(q).ok_or_else(|| Diag::new(pos, "unterminated attribute value"))?;
            let raw = r[1..=end].to_owned();
            let base = self.i + 1;
            self.i += end + 2;
            let parts = split_parts(&raw, base)?;
            match parts.as_slice() {
                [] => AttrValue::Static(String::new()),
                [Part::Text(t)] => AttrValue::Static(t.clone()),
                _ if is_handler => {
                    return Err(Diag::new(pos, "event handlers take a single `{…}` value"));
                }
                _ => AttrValue::Parts(parts),
            }
        } else {
            let len = r.find(|c: char| c.is_whitespace() || c == '>').unwrap_or(r.len());
            self.i += len;
            AttrValue::Static(r[..len].to_owned())
        };
        if is_handler && !matches!(value, AttrValue::Handler { .. }) {
            return Err(Diag::new(pos, format!("`{name}` needs a function, for example `{name}={{() => …}}`")));
        }
        Ok(Attr { name, value })
    }
}

fn split_parts(raw: &str, base: usize) -> Result<Vec<Part>, Diag> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let r = &raw[i..];
        if r.starts_with('{') {
            let end = r.find('}').ok_or_else(|| Diag::new(base + i, "unclosed `{` in attribute"))?;
            parts.push(Part::Expr(expr::parse(&r[1..end], base + i + 1)?));
            i += end + 1;
        } else {
            let end = r.find('{').unwrap_or(r.len());
            parts.push(Part::Text(r[..end].to_owned()));
            i += end;
        }
    }
    Ok(parts)
}

pub fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || f == '$') && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

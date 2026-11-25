//! Scoped CSS.
//!
//! A `<style>` block is scoped unless it is `<style global>` or the app sets
//! `css_scope = false` in `rustkit.toml`. Each element from that file gets
//! `data-rk="…"`, a hash of the style text, and each selector is rewritten so
//! it only matches those elements. `:global(…)` and a `:global { … }` block
//! stay unscoped. `html`, `body` and `:root` are left alone: the document
//! shell is not part of any template, so a scope attribute would never match.

use crate::parse::{Node, ScopeMark};

#[derive(Debug)]
pub struct CssError {
    pub pos: usize,
    pub msg: String,
}

/// Rewrites `style` and marks `nodes` when the block is scoped.
/// A file with no scoped style is marked plain, so a parent file does not
/// stamp its own scope onto these elements.
pub fn apply(style: &mut Option<String>, global: bool, nodes: &mut [Node]) -> Result<(), CssError> {
    let scoping = style.as_ref().is_some_and(|css| !global && !css.trim().is_empty());
    if scoping {
        let css = style.as_deref().unwrap();
        let id = crate::hash(css);
        let rewritten = rewrite(css, Some(&id))?;
        *style = Some(rewritten);
        stamp(nodes, Some(&id));
    } else {
        stamp(nodes, None);
    }
    Ok(())
}

fn stamp(nodes: &mut [Node], id: Option<&str>) {
    for node in nodes {
        stamp_node(node, id);
    }
}

fn stamp_node(node: &mut Node, id: Option<&str>) {
    match node {
        Node::Element(e) => {
            if e.scope == ScopeMark::Open {
                e.scope = match id {
                    Some(id) => ScopeMark::Scoped(id.to_owned()),
                    None => ScopeMark::Plain,
                };
            }
            stamp(&mut e.children, id);
        }
        Node::If { branches, els, .. } => {
            for (_, body) in branches {
                stamp(body, id);
            }
            if let Some(body) = els {
                stamp(body, id);
            }
        }
        Node::Each { body, .. } | Node::Source { body, .. } => stamp(body, id),
        Node::Use { children, .. } => stamp(children, id),
        Node::Text(_) | Node::Expr { .. } | Node::Slot { .. } => {}
    }
}

/// `scope` is the `data-rk` value. `None` emits selectors without one, which
/// is how a `:global { … }` block is rewritten.
pub fn rewrite(css: &str, scope: Option<&str>) -> Result<String, CssError> {
    rewrite_at(css, scope, 0)
}

fn rewrite_at(css: &str, scope: Option<&str>, base: usize) -> Result<String, CssError> {
    let mut p = Scan { s: css, i: 0 };
    let mut out = String::with_capacity(css.len() + 16);
    loop {
        p.copy_ws_and_comments(&mut out)?;
        if p.eof() {
            break;
        }
        if p.peek() == Some('}') {
            return Err(p.err(base, "unexpected `}`"));
        }
        if p.peek() == Some('@') {
            p.at_rule(&mut out, scope, base)?;
        } else {
            p.style_rule(&mut out, scope, base)?;
        }
    }
    Ok(out)
}

struct Scan<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Scan<'a> {
    fn err(&self, base: usize, msg: impl Into<String>) -> CssError {
        CssError { pos: base + self.i, msg: msg.into() }
    }

    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }

    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn copy_ws_and_comments(&mut self, out: &mut String) -> Result<(), CssError> {
        loop {
            if self.s[self.i..].starts_with("/*") {
                let end = self.s[self.i..].find("*/").ok_or_else(|| self.err(0, "unterminated comment in `<style>`"))?;
                out.push_str(&self.s[self.i..self.i + end + 2]);
                self.i += end + 2;
            } else if self.peek().is_some_and(|c| c.is_whitespace()) {
                out.push(self.bump().unwrap());
            } else {
                break;
            }
        }
        Ok(())
    }

    fn at_rule(&mut self, out: &mut String, scope: Option<&str>, base: usize) -> Result<(), CssError> {
        let start = self.i;
        self.bump();
        let name_at = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            self.bump();
        }
        if self.i == name_at {
            return Err(self.err(base, "expected an at-rule name"));
        }
        let name = self.s[name_at..self.i].to_ascii_lowercase();
        self.read_until(|c| c == '{' || c == ';', base)?;
        match self.peek() {
            Some(';') => {
                self.bump();
                out.push_str(&self.s[start..self.i]);
                Ok(())
            }
            Some('{') => {
                let brace = self.i;
                let inner_at = brace + 1;
                let inner = self.read_block(base)?;
                if raw_at_rule(&name) {
                    out.push_str(&self.s[start..self.i]);
                } else {
                    out.push_str(&self.s[start..=brace]);
                    let rewritten = rewrite_at(&inner, scope, base + inner_at)?;
                    out.push_str(&rewritten);
                    out.push('}');
                }
                Ok(())
            }
            _ => Err(self.err(base, "unclosed at-rule")),
        }
    }

    fn style_rule(&mut self, out: &mut String, scope: Option<&str>, base: usize) -> Result<(), CssError> {
        let sel_at = self.i;
        self.read_until(|c| c == '{', base)?;
        if self.peek() != Some('{') {
            return Err(self.err(base, "unclosed selector: expected `{`"));
        }
        let sel = self.s[sel_at..self.i].to_owned();
        let inner_at = self.i + 1;
        let inner = self.read_block(base)?;
        let trimmed = strip_comments(&sel);
        let trimmed = trimmed.trim();
        if trimmed == ":global" {
            let rewritten = rewrite_at(&inner, None, base + inner_at)?;
            out.push_str(&rewritten);
            return Ok(());
        }
        let selectors = rewrite_selectors(&sel, scope).map_err(|mut e| {
            e.pos = base + sel_at + e.pos;
            e
        })?;
        let body = inner.trim();
        out.push_str(&selectors);
        out.push_str(" {");
        if !body.is_empty() {
            out.push(' ');
            out.push_str(body);
            out.push(' ');
        }
        out.push('}');
        Ok(())
    }

    /// Copies up to but not including `end` at depth 0. Strings, comments and
    /// brackets are skipped.
    fn read_until(&mut self, end: impl Fn(char) -> bool, base: usize) -> Result<(), CssError> {
        let mut depth = 0i32;
        while let Some(c) = self.peek() {
            if c == '"' || c == '\'' {
                self.skip_string(base)?;
                continue;
            }
            if self.s[self.i..].starts_with("/*") {
                self.skip_comment(base)?;
                continue;
            }
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                c if depth == 0 && end(c) => return Ok(()),
                _ => {}
            }
            self.bump();
        }
        Ok(())
    }

    /// Consumes `{ … }` and returns the inside.
    fn read_block(&mut self, base: usize) -> Result<String, CssError> {
        if self.bump() != Some('{') {
            return Err(self.err(base, "expected `{`"));
        }
        let start = self.i;
        let mut depth = 1i32;
        while let Some(c) = self.peek() {
            if c == '"' || c == '\'' {
                self.skip_string(base)?;
                continue;
            }
            if self.s[self.i..].starts_with("/*") {
                self.skip_comment(base)?;
                continue;
            }
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner = self.s[start..self.i].to_owned();
                        self.bump();
                        return Ok(inner);
                    }
                }
                _ => {}
            }
            self.bump();
        }
        Err(CssError { pos: base + start, msg: "unclosed `{` in `<style>`".into() })
    }

    fn skip_string(&mut self, base: usize) -> Result<(), CssError> {
        let quote = self.bump().unwrap();
        while let Some(c) = self.bump() {
            if c == '\\' {
                self.bump();
            } else if c == quote {
                return Ok(());
            }
        }
        Err(self.err(base, "unterminated string in `<style>`"))
    }

    fn skip_comment(&mut self, base: usize) -> Result<(), CssError> {
        let end = self.s[self.i..].find("*/").ok_or_else(|| self.err(base, "unterminated comment in `<style>`"))?;
        self.i += end + 2;
        Ok(())
    }
}

fn raw_at_rule(name: &str) -> bool {
    matches!(
        name,
        "keyframes"
            | "-webkit-keyframes"
            | "-moz-keyframes"
            | "font-face"
            | "import"
            | "charset"
            | "namespace"
            | "page"
            | "property"
            | "counter-style"
            | "font-feature-values"
            | "color-profile"
            | "font-palette-values"
    )
}

fn rewrite_selectors(list: &str, scope: Option<&str>) -> Result<String, CssError> {
    let mut out = Vec::new();
    for (at, sel) in split_commas(list)? {
        let sel = strip_comments(sel);
        let sel = sel.trim();
        if sel.is_empty() {
            return Err(CssError { pos: at, msg: "empty selector".into() });
        }
        out.push(scope_selector(sel, scope).map_err(|mut e| {
            e.pos += at;
            e
        })?);
    }
    Ok(out.join(", "))
}

fn scope_selector(sel: &str, scope: Option<&str>) -> Result<String, CssError> {
    if has_global(sel) {
        return unwrap_global(sel);
    }
    let Some(id) = scope else {
        return Ok(sel.to_owned());
    };
    let (prefix, last) = split_last(sel);
    let scoped = scope_compound(last.trim(), id);
    let prefix = prefix.trim_end();
    if prefix.is_empty() {
        Ok(scoped)
    } else {
        Ok(format!("{prefix} {scoped}"))
    }
}

fn scope_compound(compound: &str, id: &str) -> String {
    if compound.is_empty() || is_shell(compound) {
        return compound.to_owned();
    }
    let attr = format!("[data-rk=\"{id}\"]");
    match find_pseudo_element(compound) {
        Some(pos) => format!("{}{attr}{}", &compound[..pos], &compound[pos..]),
        None => format!("{compound}{attr}"),
    }
}

/// `html`, `body` and `:root`, with pseudo-classes or pseudo-elements, match
/// the document shell. A scope attribute on them would match nothing.
fn is_shell(compound: &str) -> bool {
    let mut s = compound.trim();
    loop {
        if matches!(s, "body" | "html" | ":root") {
            return true;
        }
        match rfind_pseudo(s) {
            Some(0) | None => return false,
            Some(pos) => s = s[..pos].trim_end(),
        }
    }
}

fn rfind_pseudo(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = s.len();
    while i > 0 {
        i -= 1;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        let c = s[i..].chars().next().unwrap();
        match c {
            ')' | ']' => depth += 1,
            '(' | '[' => depth -= 1,
            ':' if depth == 0 => {
                let start = if i > 0 && s.as_bytes()[i - 1] == b':' { i - 1 } else { i };
                return Some(start);
            }
            _ => {}
        }
    }
    None
}

fn find_pseudo_element(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c == '"' || c == '\'' {
            break;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ':' if depth == 0 && s[i..].starts_with("::") => return Some(i),
            _ => {}
        }
        i += c.len_utf8();
    }
    None
}

fn has_global(sel: &str) -> bool {
    find_global(sel).is_some()
}

fn find_global(sel: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = 0;
    while i < sel.len() {
        if sel[i..].starts_with("/*") {
            if let Some(end) = sel[i..].find("*/") {
                i += end + 2;
                continue;
            }
            break;
        }
        let c = sel[i..].chars().next().unwrap();
        if c == '"' || c == '\'' {
            i = skip_string_at(sel, i);
            continue;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ':' if depth == 0 && sel[i..].starts_with(":global(") => return Some(i),
            _ => {}
        }
        i += c.len_utf8();
    }
    None
}

fn unwrap_global(sel: &str) -> Result<String, CssError> {
    let mut out = String::new();
    let mut rest_at = 0;
    let mut cursor = sel;
    while let Some(at) = find_global(cursor) {
        out.push_str(&cursor[..at]);
        let open = at + ":global".len();
        let close = match_paren(cursor, open).ok_or_else(|| CssError { pos: rest_at + at, msg: "unclosed `:global(`".into() })?;
        out.push_str(cursor[open + 1..close].trim());
        rest_at += close + 1;
        cursor = &cursor[close + 1..];
    }
    out.push_str(cursor);
    Ok(out)
}

fn match_paren(s: &str, open: usize) -> Option<usize> {
    if !s[open..].starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut i = open;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            '"' | '\'' => {
                i = skip_string_at(s, i);
                continue;
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    None
}

fn split_commas(list: &str) -> Result<Vec<(usize, &str)>, CssError> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < list.len() {
        if list[i..].starts_with("/*") {
            let end = list[i..].find("*/").ok_or_else(|| CssError { pos: i, msg: "unterminated comment in `<style>`".into() })?;
            i += end + 2;
            continue;
        }
        let c = list[i..].chars().next().unwrap();
        if c == '"' || c == '\'' {
            i = skip_string_at(list, i);
            continue;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push((start, &list[start..i]));
                start = i + c.len_utf8();
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    out.push((start, &list[start..]));
    Ok(out)
}

fn split_last(sel: &str) -> (String, String) {
    let mut last = 0;
    let mut depth = 0i32;
    let mut i = 0;
    while i < sel.len() {
        if sel[i..].starts_with("/*") {
            if let Some(end) = sel[i..].find("*/") {
                i += end + 2;
                continue;
            }
        }
        let c = sel[i..].chars().next().unwrap();
        let n = c.len_utf8();
        if c == '"' || c == '\'' {
            i = skip_string_at(sel, i);
            continue;
        }
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '>' | '+' | '~' if depth == 0 => {
                let mut j = i + n;
                while j < sel.len() && sel.as_bytes()[j].is_ascii_whitespace() {
                    j += 1;
                }
                last = j;
            }
            c if c.is_whitespace() && depth == 0 => {
                let mut j = i;
                while j < sel.len() && sel[j..].chars().next().is_some_and(|d| d.is_whitespace()) {
                    j += sel[j..].chars().next().unwrap().len_utf8();
                }
                if j < sel.len() {
                    last = j;
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += n;
    }
    (sel[..last].to_owned(), sel[last..].to_owned())
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with("/*") {
            if let Some(end) = s[i..].find("*/") {
                i += end + 2;
                out.push(' ');
                continue;
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn skip_string_at(s: &str, start: usize) -> usize {
    let quote = s[start..].chars().next().unwrap();
    let mut i = start + quote.len_utf8();
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        i += c.len_utf8();
        if c == '\\' {
            if let Some(n) = s[i..].chars().next() {
                i += n.len_utf8();
            }
        } else if c == quote {
            break;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped(css: &str) -> String {
        rewrite(css, Some("ab")).unwrap()
    }

    #[test]
    fn selectors_gain_the_scope_attribute_on_the_last_compound() {
        assert_eq!(scoped(".copy { color: #036; }"), ".copy[data-rk=\"ab\"] { color: #036; }");
        assert_eq!(scoped("h1, h2 { margin: 0; }"), "h1[data-rk=\"ab\"], h2[data-rk=\"ab\"] { margin: 0; }");
        assert_eq!(scoped("#site a:hover { color: inherit; }"), "#site a:hover[data-rk=\"ab\"] { color: inherit; }");
        assert_eq!(scoped("a::before { content: \"\"; }"), "a[data-rk=\"ab\"]::before { content: \"\"; }");
        assert_eq!(scoped(".a > .b { color: red; }"), ".a > .b[data-rk=\"ab\"] { color: red; }");
    }

    #[test]
    fn global_selectors_and_the_document_shell_are_not_scoped() {
        assert_eq!(scoped(":global(.banner) { font-weight: 700; }"), ".banner { font-weight: 700; }");
        assert_eq!(scoped(":global(h1, h2) { margin: 0; }"), "h1, h2 { margin: 0; }");
        let mixed = scoped(".copy { color: #036; }\n:global { body { margin: 0; } .banner { font-weight: 700; } }\n");
        assert!(mixed.contains(".copy[data-rk=\"ab\"]"), "{mixed}");
        assert!(mixed.contains("body { margin: 0; }"), "{mixed}");
        assert!(mixed.contains(".banner { font-weight: 700; }"), "{mixed}");
        assert!(!mixed.contains(".banner[data-rk"), "{mixed}");
        assert_eq!(scoped("body { margin: 0; }"), "body { margin: 0; }");
        assert_eq!(scoped(":root { color-scheme: light dark; }"), ":root { color-scheme: light dark; }");
        assert_eq!(scoped("body:hover { color: red; }"), "body:hover { color: red; }");
    }

    #[test]
    fn at_rules_scope_their_rules_and_keyframes_are_copied() {
        let css = scoped("@media (min-width: 40rem) { .copy { color: red; } }\n@keyframes in { from { opacity: 0; } to { opacity: 1; } }\n.box { animation: in 1s; }");
        assert!(css.contains("@media (min-width: 40rem) { .copy[data-rk=\"ab\"] { color: red; } }"), "{css}");
        assert!(css.contains("@keyframes in { from { opacity: 0; } to { opacity: 1; } }"), "{css}");
        assert!(css.contains(".box[data-rk=\"ab\"] { animation: in 1s; }"), "{css}");
        assert!(!css.contains("from[data-rk"), "{css}");
        let imported = scoped("@import \"other.css\";\n.a { color: red; }");
        assert!(imported.starts_with("@import \"other.css\";"), "{imported}");
        assert!(imported.contains(".a[data-rk=\"ab\"]"), "{imported}");
    }

    #[test]
    fn a_broken_style_block_is_an_error() {
        let err = rewrite(".a { color: red;", Some("ab")).unwrap_err();
        assert!(err.msg.contains("unclosed"), "{}", err.msg);
        let err = rewrite("/* oops\n.a { color: red; }", Some("ab")).unwrap_err();
        assert!(err.msg.contains("unterminated comment"), "{}", err.msg);
    }
}

//! The page `<script>`: `$sources` imports, `$state` declarations, and the
//! JavaScript left over, plus a small JS scanner used to rewrite state reads
//! and find which template names a handler uses.

use crate::Diag;
use crate::expr::{self, Expr};

/// `import Name from "$lib/….rk"` or a relative `.rk` path.
#[derive(Clone)]
pub struct ComponentImport {
    pub name: String,
    pub spec: String,
    /// Byte offset of the import line in the file.
    pub pos: usize,
}

pub struct Script {
    pub sources: Vec<String>,
    /// Default imports of `.rk` components, in source order.
    pub components: Vec<ComponentImport>,
    /// `let name = $state(literal)`, in declaration order.
    pub state: Vec<(String, Expr)>,
    /// Everything else, untouched apart from the removed lines.
    pub rest: String,
}

pub fn parse(src: &str, base: usize) -> Result<Script, Diag> {
    let mut sources = Vec::new();
    let mut components = Vec::new();
    let mut state = Vec::new();
    let mut rest = String::new();
    let mut offset = base;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        let lead = line.len() - line.trim_start().len();
        let pos = offset + lead;
        if t.starts_with("import") && (t.contains("\"$sources\"") || t.contains("'$sources'")) {
            let open = t.find('{').ok_or_else(|| Diag::new(pos, "expected `import { … } from \"$sources\"`"))?;
            let close = t.find('}').ok_or_else(|| Diag::new(pos, "expected `}`"))?;
            for name in t[open + 1..close].split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if name.contains(" as ") {
                    return Err(Diag::new(pos, "renaming a source import is not supported yet"));
                }
                sources.push(name.to_owned());
            }
        } else if looks_like_component_import(t) {
            let import = component_import(t, pos)?;
            if components.iter().any(|c: &ComponentImport| c.name == import.name) {
                return Err(Diag::new(pos, format!("`{}` is already imported", import.name)));
            }
            components.push(import);
        } else if let Some(decl) = t.strip_prefix("let ").filter(|d| d.contains("$state(")) {
            let (name, init) = decl.split_once('=').ok_or_else(|| Diag::new(offset + lead, "expected `let name = $state(…)`"))?;
            let name = name.trim();
            let init = init.trim().trim_end_matches(';').trim();
            let arg = init.strip_prefix("$state(").and_then(|s| s.strip_suffix(')')).ok_or_else(|| Diag::new(offset + lead, "expected `$state(…)` to be the whole initialiser"))?;
            let arg_pos = offset + line.find("$state(").unwrap() + 7;
            let lit = if arg.trim().is_empty() { Expr::Null } else { expr::parse(arg, arg_pos)? };
            if !matches!(lit, Expr::Str(_) | Expr::Num(_) | Expr::Bool(_) | Expr::Null) {
                return Err(Diag::new(arg_pos, "initial state must be a literal string, number, boolean or null").help("the server renders the page with this initial value, so it must mean the same thing in Rust and JavaScript"));
            }
            state.push((name.to_owned(), lit));
        } else {
            rest.push_str(line);
        }
        offset += line.len();
    }
    Ok(Script { sources, components, state, rest })
}

/// A default import of an `.rk` file. Other `import` lines stay in the page
/// script; they are not bundled.
fn looks_like_component_import(line: &str) -> bool {
    let line = line.trim().trim_end_matches(';').trim();
    let Some(rest) = line.strip_prefix("import ") else {
        return false;
    };
    if rest.trim_start().starts_with('{') {
        return false;
    }
    let Some((_, spec)) = split_from(rest) else {
        return false;
    };
    spec.starts_with("$lib/") || spec.starts_with("./") || spec.starts_with("../") || spec.ends_with(".rk")
}

fn component_import(line: &str, pos: usize) -> Result<ComponentImport, Diag> {
    let raw = line.trim().trim_end_matches(';').trim();
    let rest = raw.strip_prefix("import ").ok_or_else(|| Diag::new(pos, "expected `import Name from \"$lib/….rk\"`"))?;
    if rest.trim_start().starts_with('{') {
        return Err(Diag::new(pos, "a component import names one component").help("write `import Header from \"$lib/Header.rk\"`"));
    }
    let (name, spec) = split_from(rest).ok_or_else(|| Diag::new(pos, "expected `import Name from \"$lib/….rk\"`"))?;
    if !is_component_name(name) {
        return Err(Diag::new(pos, format!("`{name}` cannot be a component tag")).help("the name starts with an uppercase letter and is used as `<Name>`"));
    }
    if !(spec.starts_with("$lib/") || spec.starts_with("./") || spec.starts_with("../")) || !spec.ends_with(".rk") {
        return Err(Diag::new(pos, format!("`{spec}` is not a component path")).help("use `$lib/Header.rk` for a file in `src/lib`, or a relative path ending in `.rk`"));
    }
    Ok(ComponentImport { name: name.to_owned(), spec: spec.to_owned(), pos })
}

fn split_from(rest: &str) -> Option<(&str, &str)> {
    let (name, spec) = rest.split_once(" from ")?;
    let name = name.trim();
    let spec = spec.trim();
    if name.is_empty() || spec.contains(" from ") {
        return None;
    }
    let quote = spec.chars().next()?;
    if (quote != '"' && quote != '\'') || spec.len() < 2 || !spec.ends_with(quote) {
        return None;
    }
    Some((name, &spec[1..spec.len() - 1]))
}

fn is_component_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}

/// One identifier in a piece of JavaScript.
pub struct IdentUse {
    pub start: usize,
    pub end: usize,
    pub name: String,
    /// True for `x.name` and `{ name: … }` keys, which are not variable reads.
    pub is_property: bool,
    /// The property read right after it, as in `name.field`.
    pub field: Option<String>,
    /// True when written as an object shorthand, `{ name }`.
    pub shorthand: bool,
}

/// Lists identifiers outside strings and comments. Template literal holes are
/// scanned; regular expression literals are not recognised.
pub fn idents(js: &str) -> Vec<IdentUse> {
    let b = js.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    // Stack of brace depths at which an enclosing template literal resumes.
    let mut tpl: Vec<usize> = Vec::new();
    let mut depth = 0usize;
    let mut in_tpl = false;
    let mut prev = b' ';
    while i < b.len() {
        let c = b[i];
        if in_tpl {
            if c == b'\\' {
                i += 2;
            } else if c == b'`' {
                in_tpl = false;
                i += 1;
                prev = b'`';
            } else if c == b'$' && b.get(i + 1) == Some(&b'{') {
                tpl.push(depth);
                depth += 1;
                in_tpl = false;
                i += 2;
                prev = b'(';
            } else {
                i += 1;
            }
            continue;
        }
        match c {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = js[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            }
            b'"' | b'\'' => {
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                prev = c;
            }
            b'`' => {
                in_tpl = true;
                i += 1;
            }
            b'{' => {
                depth += 1;
                i += 1;
                prev = c;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
                if tpl.last() == Some(&depth) {
                    tpl.pop();
                    in_tpl = true;
                } else {
                    prev = c;
                }
            }
            _ if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                let s = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                    i += 1;
                }
                let after = js[i..].trim_start();
                let field = after.strip_prefix('.').and_then(|f| {
                    let f = f.trim_start();
                    let n = f.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')).unwrap_or(f.len());
                    (n > 0).then(|| f[..n].to_owned())
                });
                let next = after.as_bytes().first().copied();
                let in_object = prev == b'{' || prev == b',';
                let is_key = in_object && next == Some(b':') && !after.starts_with("::");
                out.push(IdentUse { start: s, end: i, name: js[s..i].to_owned(), is_property: prev == b'.' || is_key, field, shorthand: in_object && matches!(next, Some(b'}' | b',')) });
                prev = b'a';
            }
            _ => {
                if !c.is_ascii_whitespace() {
                    prev = c;
                }
                i += 1;
            }
        }
    }
    out
}

/// Rewrites reads and writes of state variables `x` to `$s.x`, so assignments
/// reach the reactive store.
pub fn rewrite_state(js: &str, state: &[String]) -> String {
    let mut out = String::with_capacity(js.len() + 16);
    let mut last = 0;
    for u in idents(js) {
        if u.is_property || !state.contains(&u.name) {
            continue;
        }
        out.push_str(&js[last..u.start]);
        if u.shorthand {
            out.push_str(&format!("{}: $s.{}", u.name, u.name));
        } else {
            out.push_str("$s.");
            out.push_str(&u.name);
        }
        last = u.end;
    }
    out.push_str(&js[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_state_but_not_properties_or_strings() {
        let s = vec!["editing".to_owned(), "n".to_owned()];
        assert_eq!(rewrite_state("() => editing = !editing", &s), "() => $s.editing = !$s.editing");
        assert_eq!(rewrite_state("x.editing + 'editing' + `${n}`", &s), "x.editing + 'editing' + `${$s.n}`");
        assert_eq!(rewrite_state("f({ editing, n: 1 })", &s), "f({ editing: $s.editing, n: 1 })");
    }

    #[test]
    fn finds_field_reads() {
        let u = idents("() => save(data.displayName)");
        let d = u.iter().find(|u| u.name == "data").unwrap();
        assert_eq!(d.field.as_deref(), Some("displayName"));
    }
}

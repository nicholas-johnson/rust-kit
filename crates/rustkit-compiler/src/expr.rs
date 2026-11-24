//! The initial-template expression language.
//!
//! Every expression here has defined behaviour in both targets: field reads,
//! `.length`, literals, `!`, `&&`, `||`, strict equality and relational
//! comparison. Calls are rejected with a diagnostic that points the author at a
//! derived field on the resolver. Event handlers are not parsed here; they are
//! arbitrary JavaScript and only ever run in the browser.

use crate::Diag;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `root.seg.seg`; `pos` is the byte offset of `root` in the page file.
    Path { root: String, segs: Vec<String>, pos: usize },
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Expr {
    /// Every `root` this expression reads, with the first field segment if any.
    pub fn paths(&self, out: &mut Vec<(String, Option<String>)>) {
        match self {
            Expr::Path { root, segs, .. } => out.push((root.clone(), segs.first().cloned())),
            Expr::Not(e) => e.paths(out),
            Expr::Bin(_, a, b) => {
                a.paths(out);
                b.paths(out);
            }
            _ => {}
        }
    }

    pub fn roots(&self) -> Vec<String> {
        let mut v = Vec::new();
        self.paths(&mut v);
        v.into_iter().map(|(r, _)| r).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String, usize),
    Str(String),
    Num(f64),
    Punct(&'static str, usize),
}

fn tokenize(src: &str, base: usize) -> Result<Vec<Tok>, Diag> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                i += 1;
            }
            out.push(Tok::Ident(src[s..i].to_owned(), base + s));
        } else if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let n = src[s..i]
                .parse()
                .map_err(|_| Diag::new(base + s, format!("invalid number `{}`", &src[s..i])))?;
            out.push(Tok::Num(n));
        } else if c == b'"' || c == b'\'' {
            let s = i;
            i += 1;
            let mut val = String::new();
            loop {
                match b.get(i) {
                    None => return Err(Diag::new(base + s, "unterminated string")),
                    Some(&q) if q == c => break,
                    Some(b'\\') => {
                        let esc = src[i + 1..].chars().next().ok_or_else(|| Diag::new(base + i, "bad escape"))?;
                        val.push(match esc {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                        i += 1 + esc.len_utf8();
                    }
                    Some(_) => {
                        let ch = src[i..].chars().next().unwrap();
                        val.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
            i += 1;
            out.push(Tok::Str(val));
        } else {
            const PUNCT: [&str; 14] = ["===", "!==", "==", "!=", "<=", ">=", "&&", "||", "<", ">", "!", ".", "(", ")"];
            let rest = &src[i..];
            let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
                let ch = rest.chars().next().unwrap();
                return Err(unsupported(base + i, &format!("`{ch}`")));
            };
            out.push(Tok::Punct(p, base + i));
            i += p.len();
        }
    }
    Ok(out)
}

fn unsupported(pos: usize, what: &str) -> Diag {
    Diag::new(pos, format!("{what} is not supported in a template expression"))
        .help("template expressions are rendered by Rust on the server and by JavaScript in the browser, so they are limited to field reads, `.length`, literals, `!`, `&&`, `||`, `===`, `!==`, `<`, `<=`, `>`, `>=`")
}

struct Parser {
    toks: Vec<Tok>,
    i: usize,
    end: usize,
}

/// Parses `src`, which starts at byte offset `base` in the page file.
pub fn parse(src: &str, base: usize) -> Result<Expr, Diag> {
    let toks = tokenize(src, base)?;
    if toks.is_empty() {
        return Err(Diag::new(base, "empty expression"));
    }
    let mut p = Parser { toks, i: 0, end: base + src.len() };
    let e = p.or()?;
    if p.i < p.toks.len() {
        return Err(Diag::new(p.pos(), "unexpected token in expression"));
    }
    Ok(e)
}

impl Parser {
    fn pos(&self) -> usize {
        match self.toks.get(self.i) {
            Some(Tok::Ident(_, p) | Tok::Punct(_, p)) => *p,
            _ => self.end,
        }
    }

    fn eat(&mut self, punct: &str) -> bool {
        if matches!(self.toks.get(self.i), Some(Tok::Punct(p, _)) if *p == punct) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Result<Expr, Diag> {
        let mut e = self.and()?;
        while self.eat("||") {
            e = Expr::Bin(Op::Or, Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }

    fn and(&mut self) -> Result<Expr, Diag> {
        let mut e = self.eq()?;
        while self.eat("&&") {
            e = Expr::Bin(Op::And, Box::new(e), Box::new(self.eq()?));
        }
        Ok(e)
    }

    fn eq(&mut self) -> Result<Expr, Diag> {
        let mut e = self.rel()?;
        loop {
            let pos = self.pos();
            let op = if self.eat("===") {
                Op::Eq
            } else if self.eat("!==") {
                Op::Ne
            } else if self.eat("==") || self.eat("!=") {
                return Err(Diag::new(pos, "loose equality is not supported").help("use `===` or `!==`"));
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.rel()?));
        }
    }

    fn rel(&mut self) -> Result<Expr, Diag> {
        let mut e = self.unary()?;
        loop {
            let op = if self.eat("<=") {
                Op::Le
            } else if self.eat(">=") {
                Op::Ge
            } else if self.eat("<") {
                Op::Lt
            } else if self.eat(">") {
                Op::Gt
            } else {
                return Ok(e);
            };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, Diag> {
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, Diag> {
        let pos = self.pos();
        let tok = self.toks.get(self.i).cloned();
        self.i += 1;
        match tok {
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Punct("(", _)) => {
                let e = self.or()?;
                if !self.eat(")") {
                    return Err(Diag::new(self.pos(), "expected `)`"));
                }
                Ok(e)
            }
            Some(Tok::Ident(name, p)) => match name.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" | "undefined" => Ok(Expr::Null),
                _ => {
                    let mut segs = Vec::new();
                    while self.eat(".") {
                        match self.toks.get(self.i).cloned() {
                            Some(Tok::Ident(s, _)) => {
                                segs.push(s);
                                self.i += 1;
                            }
                            _ => return Err(Diag::new(self.pos(), "expected a field name after `.`")),
                        }
                    }
                    if matches!(self.toks.get(self.i), Some(Tok::Punct("(", _))) {
                        let callee = std::iter::once(name.as_str())
                            .chain(segs.iter().map(String::as_str))
                            .collect::<Vec<_>>()
                            .join(".");
                        return Err(Diag::new(
                            p,
                            format!("`{callee}(…)` cannot be rendered by the Rust server renderer"),
                        )
                        .help("Rust has to produce exactly the same text as the browser. Compute this value in the source's resolver and expose it as a field on the view (for example `data.formattedDate`), then read that field here"));
                    }
                    Ok(Expr::Path { root: name, segs, pos: p })
                }
            },
            _ => Err(unsupported(pos, "this token")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_and_operators() {
        let e = parse("!editing && data.items.length > 0", 0).unwrap();
        assert!(matches!(e, Expr::Bin(Op::And, _, _)));
    }

    #[test]
    fn rejects_calls_with_help() {
        let d = parse("format(data.joined)", 10).unwrap_err();
        assert_eq!(d.pos, 10);
        assert!(d.msg.contains("`format(…)`"));
        assert!(d.help.unwrap().contains("resolver"));
        assert!(parse("data.name.toUpperCase()", 0).unwrap_err().msg.contains("toUpperCase"));
    }
}

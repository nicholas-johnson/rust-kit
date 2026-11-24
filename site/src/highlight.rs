//! A small build-time syntax highlighter. Output is HTML with short class
//! names (`k` keyword, `s` string, `c` comment, `n` number, `f` function or
//! macro, `y` type, `t` tag, `a` attribute, `e` template expression, `p` punct).

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn span(out: &mut String, class: &str, text: &str) {
    if class.is_empty() {
        out.push_str(&esc(text));
    } else {
        out.push_str(&format!("<span class=\"{class}\">{}</span>", esc(text)));
    }
}

struct Lang {
    keywords: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: bool,
    quotes: &'static [char],
    rust: bool,
}

const RUST: Lang = Lang {
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "false", "fn", "for",
        "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
        "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
    ],
    line_comment: &["//"],
    block_comment: true,
    quotes: &['"'],
    rust: true,
};

const JS: Lang = Lang {
    keywords: &[
        "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do", "else",
        "export", "false", "finally", "for", "from", "function", "if", "import", "in", "instanceof", "let", "new",
        "null", "of", "return", "switch", "this", "throw", "true", "try", "typeof", "undefined", "var", "while",
    ],
    line_comment: &["//"],
    block_comment: true,
    quotes: &['"', '\'', '`'],
    rust: false,
};

const BASH: Lang = Lang {
    keywords: &["cd", "export", "if", "then", "fi", "for", "do", "done", "echo"],
    line_comment: &["#"],
    block_comment: false,
    quotes: &['"', '\''],
    rust: false,
};

const TOML: Lang = Lang {
    keywords: &["true", "false"],
    line_comment: &["#"],
    block_comment: false,
    quotes: &['"', '\''],
    rust: false,
};

const CSS: Lang = Lang { keywords: &[], line_comment: &[], block_comment: true, quotes: &['"', '\''], rust: false };

fn code(src: &str, lang: &Lang, out: &mut String) {
    let mut i = 0;
    let b = src.as_bytes();
    while i < src.len() {
        let rest = &src[i..];
        if let Some(lc) = lang.line_comment.iter().find(|c| rest.starts_with(**c)) {
            // `#` only starts a comment at a word boundary in bash and toml.
            let ok = *lc != "#" || i == 0 || b[i - 1].is_ascii_whitespace();
            if ok {
                let end = rest.find('\n').unwrap_or(rest.len());
                span(out, "c", &rest[..end]);
                i += end;
                continue;
            }
        }
        if lang.block_comment && rest.starts_with("/*") {
            let end = rest.find("*/").map_or(rest.len(), |e| e + 2);
            span(out, "c", &rest[..end]);
            i += end;
            continue;
        }
        let c = rest.chars().next().unwrap();
        if lang.quotes.contains(&c) {
            let mut j = c.len_utf8();
            while j < rest.len() {
                let ch = rest[j..].chars().next().unwrap();
                if ch == '\\' {
                    j += 1 + rest[j + 1..].chars().next().map_or(0, char::len_utf8);
                    continue;
                }
                j += ch.len_utf8();
                if ch == c {
                    break;
                }
            }
            span(out, "s", &rest[..j]);
            i += j;
            continue;
        }
        if lang.rust && rest.starts_with("#[") {
            let end = rest.find(']').map_or(rest.len(), |e| e + 1);
            span(out, "a", &rest[..end]);
            i += end;
            continue;
        }
        if c.is_ascii_digit() {
            let end = rest.find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.' || ch == '_')).unwrap_or(rest.len());
            span(out, "n", &rest[..end]);
            i += end;
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let end = rest
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$' || (ch == '-' && !lang.rust && lang.quotes.len() < 3)))
                .unwrap_or(rest.len());
            let word = &rest[..end];
            let after = &rest[end..];
            let class = if lang.keywords.contains(&word) {
                "k"
            } else if lang.rust && after.starts_with('!') {
                "f"
            } else if after.starts_with('(') {
                "f"
            } else if word.starts_with(|ch: char| ch.is_ascii_uppercase()) {
                "y"
            } else {
                ""
            };
            if class == "f" && lang.rust && after.starts_with('!') {
                span(out, "f", &format!("{word}!"));
                i += end + 1;
                continue;
            }
            span(out, class, word);
            i += end;
            continue;
        }
        span(out, "", &c.to_string());
        i += c.len_utf8();
    }
}

fn bash(src: &str, out: &mut String) {
    for (n, line) in src.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        let lead = &line[..line.len() - trimmed.len()];
        out.push_str(lead);
        if trimmed.starts_with('#') {
            span(out, "c", trimmed);
            continue;
        }
        let cmd_end = trimmed.find(' ').unwrap_or(trimmed.len());
        span(out, "f", &trimmed[..cmd_end]);
        let mut rest = String::new();
        code(&trimmed[cmd_end..], &BASH, &mut rest);
        out.push_str(&rest);
    }
}

fn css(src: &str, out: &mut String) {
    // Selectors outside braces, `property: value` inside.
    let mut depth = 0;
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with("/*") {
            let end = rest.find("*/").map_or(rest.len(), |e| e + 2);
            span(out, "c", &rest[..end]);
            i += end;
            continue;
        }
        let c = rest.chars().next().unwrap();
        match c {
            '{' => {
                depth += 1;
                span(out, "p", "{");
                i += 1;
            }
            '}' => {
                depth -= 1;
                span(out, "p", "}");
                i += 1;
            }
            _ if depth == 0 => {
                let end = rest.find(['{', '/']).unwrap_or(rest.len()).max(1);
                span(out, "t", &rest[..end]);
                i += end;
            }
            _ => {
                let end = rest.find([';', '}']).unwrap_or(rest.len());
                let decl = &rest[..end];
                if let Some((prop, val)) = decl.split_once(':') {
                    span(out, "a", prop);
                    out.push(':');
                    let mut v = String::new();
                    code(val, &CSS, &mut v);
                    out.push_str(&v);
                } else {
                    out.push_str(&esc(decl));
                }
                i += end;
                if src[i..].starts_with(';') {
                    out.push(';');
                    i += 1;
                }
            }
        }
    }
}

/// HTML and Rust-kit page files: tags, attributes, `{…}` expressions, and
/// embedded `<script>` / `<style>`.
fn markup(src: &str, out: &mut String) {
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(rest.len(), |e| e + 3);
            span(out, "c", &rest[..end]);
            i += end;
        } else if rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/') {
            let end = tag_end(rest);
            let tag_src = &rest[..end];
            tag(tag_src, out);
            i += end;
            let name: String = tag_src[1..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            if (name == "script" || name == "style") && !tag_src.ends_with("/>") {
                let close = format!("</{name}>");
                let body_end = src[i..].find(&close).unwrap_or(src.len() - i);
                let body = &src[i..i + body_end];
                if name == "script" {
                    code(body, &JS, out);
                } else {
                    css(body, out);
                }
                i += body_end;
            }
        } else if rest.starts_with('{') {
            let end = brace_end(rest);
            expr(&rest[..end], out);
            i += end;
        } else {
            let end = rest.find(['<', '{']).unwrap_or(rest.len()).max(1);
            out.push_str(&esc(&rest[..end]));
            i += end;
        }
    }
}

fn tag_end(s: &str) -> usize {
    let b = s.as_bytes();
    let mut depth = 0;
    let mut quote = 0u8;
    for (i, &c) in b.iter().enumerate() {
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            continue;
        }
        match c {
            b'"' | b'\'' if depth == 0 => quote = c,
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b'>' if depth == 0 => return i + 1,
            _ => {}
        }
    }
    s.len()
}

fn brace_end(s: &str) -> usize {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    s.len()
}

fn expr(s: &str, out: &mut String) {
    out.push_str("<span class=\"e\">");
    let inner = &s[1..s.len().saturating_sub(1).max(1)];
    span(out, "p", "{");
    if let Some(kw) = ["#if", "#each", ":else if", ":else", "/if", "/each"].iter().find(|k| inner.starts_with(**k)) {
        span(out, "k", kw);
        let mut r = String::new();
        code(&inner[kw.len()..], &JS, &mut r);
        out.push_str(&r.replace("<span class=\"\">as</span>", "<span class=\"k\">as</span>"));
    } else {
        code(inner, &JS, out);
    }
    if s.ends_with('}') {
        span(out, "p", "}");
    }
    out.push_str("</span>");
}

fn tag(s: &str, out: &mut String) {
    let close = s.starts_with("</");
    let start = if close { 2 } else { 1 };
    let name_end = s[start..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').map_or(s.len(), |e| start + e);
    span(out, "p", &s[..start]);
    let name = &s[start..name_end];
    span(out, if name.starts_with(|c: char| c.is_ascii_uppercase()) { "y" } else { "t" }, name);
    let mut i = name_end;
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().unwrap();
        if c.is_whitespace() {
            out.push(c);
            i += c.len_utf8();
        } else if rest.starts_with("/>") || rest.starts_with('>') {
            span(out, "p", rest);
            break;
        } else if c == '{' {
            let end = brace_end(rest);
            expr(&rest[..end], out);
            i += end;
        } else if c == '=' {
            out.push('=');
            i += 1;
        } else if c == '"' || c == '\'' {
            let end = rest[1..].find(c).map_or(rest.len(), |e| e + 2);
            span(out, "s", &rest[..end]);
            i += end;
        } else {
            let end = rest.find(|ch: char| ch.is_whitespace() || ch == '=' || ch == '>' || ch == '/').unwrap_or(rest.len()).max(1);
            span(out, "a", &rest[..end]);
            i += end;
        }
    }
}

/// Highlights `src` for the language named in a fenced code block.
pub fn highlight(lang: &str, src: &str) -> String {
    let mut out = String::with_capacity(src.len() * 2);
    match lang {
        "rust" | "rs" => code(src, &RUST, &mut out),
        "js" | "javascript" | "ts" => code(src, &JS, &mut out),
        "bash" | "sh" | "shell" | "console" => bash(src, &mut out),
        "toml" => code(src, &TOML, &mut out),
        "json" => code(src, &TOML, &mut out),
        "css" => css(src, &mut out),
        "rk" | "html" | "svelte" => markup(src, &mut out),
        _ => out.push_str(&esc(src)),
    }
    out
}

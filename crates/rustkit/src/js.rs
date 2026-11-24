//! The value semantics shared by the Rust renderer and the browser runtime.
//!
//! Template expressions that run on the server are evaluated through [`Val`],
//! which follows the JavaScript rules for truthiness, strict equality,
//! relational comparison and number formatting. The browser evaluates the same
//! expressions natively, so both targets produce the same text.

use std::borrow::Cow;
use std::cmp::Ordering;

/// A JavaScript-shaped view of a Rust value. Strings are borrowed where possible.
#[derive(Debug, Clone, PartialEq)]
pub enum Val<'a> {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(Cow<'a, str>),
}

impl<'a> Val<'a> {
    pub fn str(s: &'a str) -> Self {
        Val::Str(Cow::Borrowed(s))
    }

    pub fn from_len(n: usize) -> Val<'static> {
        Val::Num(n as f64)
    }

    /// JavaScript `Boolean(v)`.
    pub fn truthy(&self) -> bool {
        match self {
            Val::Undefined | Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => !(*n == 0.0 || n.is_nan()),
            Val::Str(s) => !s.is_empty(),
        }
    }

    /// JavaScript `Number(v)` for the primitive cases the template language allows.
    pub fn to_number(&self) -> f64 {
        match self {
            Val::Undefined => f64::NAN,
            Val::Null => 0.0,
            Val::Bool(b) => f64::from(u8::from(*b)),
            Val::Num(n) => *n,
            Val::Str(s) => {
                let t = s.trim();
                if t.is_empty() {
                    0.0
                } else if t == "Infinity" || t == "+Infinity" {
                    f64::INFINITY
                } else if t == "-Infinity" {
                    f64::NEG_INFINITY
                } else if t.contains(|c: char| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
                    f64::NAN
                } else {
                    t.parse().unwrap_or(f64::NAN)
                }
            }
        }
    }

    /// The text a template interpolation shows. `null` and `undefined` render as
    /// empty text in both targets.
    pub fn to_text(&self) -> Cow<'_, str> {
        match self {
            Val::Undefined | Val::Null => Cow::Borrowed(""),
            Val::Bool(true) => Cow::Borrowed("true"),
            Val::Bool(false) => Cow::Borrowed("false"),
            Val::Num(n) => Cow::Owned(number_to_string(*n)),
            Val::Str(s) => Cow::Borrowed(s),
        }
    }
}

/// JavaScript `Number.prototype.toString()` for base 10.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if n == 0.0 {
        return "0".into();
    }
    let abs = n.abs();
    if (1e-6..1e21).contains(&abs) {
        // Rust's shortest round-trip formatting matches ECMAScript in this range.
        format!("{n}")
    } else {
        let s = format!("{n:e}");
        match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        }
    }
}

/// Conversion into the template's value model. Implemented for strings, numbers,
/// booleans and `Option`. A view type that is not one of these cannot be
/// interpolated, and rustc reports the missing implementation at build time.
pub trait ToJs {
    fn to_js(&self) -> Val<'_>;
}

impl ToJs for str {
    fn to_js(&self) -> Val<'_> {
        Val::str(self)
    }
}
impl ToJs for String {
    fn to_js(&self) -> Val<'_> {
        Val::str(self)
    }
}
impl ToJs for bool {
    fn to_js(&self) -> Val<'_> {
        Val::Bool(*self)
    }
}
impl<T: ToJs + ?Sized> ToJs for &T {
    fn to_js(&self) -> Val<'_> {
        (**self).to_js()
    }
}
impl<T: ToJs + ?Sized> ToJs for Box<T> {
    fn to_js(&self) -> Val<'_> {
        (**self).to_js()
    }
}
impl<T: ToJs> ToJs for Option<T> {
    fn to_js(&self) -> Val<'_> {
        match self {
            Some(v) => v.to_js(),
            None => Val::Null,
        }
    }
}
impl<'a> ToJs for Val<'a> {
    fn to_js(&self) -> Val<'_> {
        self.clone()
    }
}
macro_rules! num_to_js {
    ($($t:ty),*) => {$(
        impl ToJs for $t {
            #[allow(clippy::cast_lossless, clippy::cast_precision_loss)]
            fn to_js(&self) -> Val<'_> { Val::Num(*self as f64) }
        }
    )*};
}
num_to_js!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

// Floats render as the value the browser receives. Server data reaches the
// browser through `serde_json::to_value`, and JSON has no NaN or Infinity, so
// those become `null`. (`to_value` also widens an `f32` to `f64`, so `0.1f32`
// travels as `0.10000000149011612`, which is what `f64::from` gives here.)
impl ToJs for f64 {
    fn to_js(&self) -> Val<'_> {
        if self.is_finite() { Val::Num(*self) } else { Val::Null }
    }
}
impl ToJs for f32 {
    fn to_js(&self) -> Val<'_> {
        if self.is_finite() { Val::Num(f64::from(*self)) } else { Val::Null }
    }
}

/// Values with a JavaScript `.length`: strings count UTF-16 code units, as in
/// the browser; lists count elements.
pub trait JsLength {
    fn js_length(&self) -> usize;
}
impl JsLength for str {
    fn js_length(&self) -> usize {
        self.encode_utf16().count()
    }
}
impl JsLength for String {
    fn js_length(&self) -> usize {
        self.encode_utf16().count()
    }
}
impl<T> JsLength for Vec<T> {
    fn js_length(&self) -> usize {
        self.len()
    }
}
impl<T> JsLength for [T] {
    fn js_length(&self) -> usize {
        self.len()
    }
}
impl<T: JsLength + ?Sized> JsLength for &T {
    fn js_length(&self) -> usize {
        (**self).js_length()
    }
}

pub fn length<T: JsLength + ?Sized>(v: &T) -> Val<'static> {
    Val::from_len(v.js_length())
}

/// JavaScript `a === b`.
pub fn strict_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Undefined, Val::Undefined) | (Val::Null, Val::Null) => true,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Num(x), Val::Num(y)) => x == y,
        (Val::Str(x), Val::Str(y)) => x == y,
        _ => false,
    }
}

/// The abstract relational comparison behind `<`, `<=`, `>` and `>=`.
/// `None` means the comparison involves `NaN` and every operator is false.
fn compare(a: &Val, b: &Val) -> Option<Ordering> {
    if let (Val::Str(x), Val::Str(y)) = (a, b) {
        // JavaScript compares UTF-16 code units, not UTF-8 bytes.
        return Some(x.encode_utf16().cmp(y.encode_utf16()));
    }
    a.to_number().partial_cmp(&b.to_number())
}

pub fn lt(a: &Val, b: &Val) -> bool {
    compare(a, b) == Some(Ordering::Less)
}
pub fn gt(a: &Val, b: &Val) -> bool {
    compare(a, b) == Some(Ordering::Greater)
}
pub fn le(a: &Val, b: &Val) -> bool {
    matches!(compare(a, b), Some(Ordering::Less | Ordering::Equal))
}
pub fn ge(a: &Val, b: &Val) -> bool {
    matches!(compare(a, b), Some(Ordering::Greater | Ordering::Equal))
}

/// Escapes text for an HTML text node or a double-quoted attribute value.
/// The browser runtime uses the same five replacements.
pub fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(&mut out, s);
    out
}

/// Writes an interpolated value as escaped text.
pub fn text(out: &mut String, v: &Val) {
    escape_into(out, &v.to_text());
}

/// Writes a bound attribute. `true` produces a bare attribute, `false`, `null`
/// and `undefined` omit it, anything else is written as escaped text.
pub fn attr(out: &mut String, name: &str, v: &Val) {
    match v {
        Val::Bool(false) | Val::Null | Val::Undefined => {}
        Val::Bool(true) => {
            out.push(' ');
            out.push_str(name);
        }
        _ => {
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            escape_into(out, &v.to_text());
            out.push('"');
        }
    }
}

/// Serialises a JSON value so it can sit inside `<script type="application/json">`.
pub fn html_safe_json(v: &serde_json::Value) -> String {
    let s = v.to_string();
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_format_like_javascript() {
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(123456.0), "123456");
        assert_eq!(number_to_string(f64::NAN), "NaN");
    }

    #[test]
    fn truthiness_and_comparison_follow_javascript() {
        assert!(!Val::str("").truthy());
        assert!(Val::str("0").truthy());
        assert!(!Val::Num(f64::NAN).truthy());
        assert!(!strict_eq(&Val::Num(1.0), &Val::str("1")));
        assert!(lt(&Val::str("10"), &Val::str("9")));
        assert!(gt(&Val::str("10"), &Val::Num(9.0)));
        assert!(!le(&Val::Num(f64::NAN), &Val::Num(1.0)));
    }

    #[test]
    fn floats_render_as_their_json_value() {
        let text = |v: &dyn ToJs| v.to_js().to_text().into_owned();
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(v.to_js(), Val::Null, "{v}");
            assert_eq!(serde_json::to_value(v).unwrap(), serde_json::Value::Null);
        }
        assert_eq!(text(&f32::NAN), "");
        // The payload is built with `to_value`, which widens f32 to f64.
        let sent = serde_json::to_value(0.1f32).unwrap().to_string();
        assert_eq!(sent, "0.10000000149011612");
        assert_eq!(text(&0.1f32), sent);
        assert_eq!(text(&(0.1f64 + 0.2)), "0.30000000000000004");
    }

    #[test]
    fn json_payload_cannot_close_the_script_element() {
        let v = serde_json::json!({ "name": "</script><script>alert(1)</script>" });
        assert!(!html_safe_json(&v).contains('<'));
    }
}

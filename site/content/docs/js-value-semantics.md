---
title: JS value semantics
description: How the Rust renderer evaluates template expressions with JavaScript's rules, so the server and the browser produce the same text.
section: Reference
order: 4
---

A Rust-kit template expression can run twice: once in Rust, when the server renders the page, and again in JavaScript, when the browser re-renders a [block](/docs/blocks-and-updates/) after state changes. If the two disagreed, a page would change its text the moment the browser took over. The `rustkit::js` module exists to prevent that. It gives the Rust renderer a small model of JavaScript values and implements the rules the template language needs.

This page describes those rules. The function signatures are in the [Runtime API](/docs/runtime-api/#the-js-module).

## Values

Every expression evaluates to a `Val`:

```rust
pub enum Val<'a> {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(Cow<'a, str>),
}
```

There are no objects or arrays. A template can read fields *through* structs and loop *over* lists, but the value it finally shows or compares is always one of these primitives.

### From Rust types

Field reads go through the `ToJs` trait:

| Rust type | `Val` |
|---|---|
| `String`, `str` | `Str`, borrowed, not copied |
| `bool` | `Bool` |
| `i8` to `i64`, `isize`, `u8` to `u64`, `usize`, `f32`, `f64` | `Num`, converted with `as f64` |
| `Option<T>` | `None` is `Null`; `Some(v)` is `v`'s value |
| `&T`, `Box<T>` | the inner value |
| `Val` | itself |

Anything else, such as a `Vec`, a struct or a `chrono` date, has no `ToJs` implementation, so interpolating it is a rustc error at build time. Format dates and money in the resolver and expose the result as a `String` field.

> [!WARNING]
> Use `f64` for fractional numbers in views, not `f32`. The server converts `0.1f32` to `f64` and renders `0.10000000149011612`, while `serde_json` sends the same field to the browser as `0.1`. The two renderings then differ.

Integers above 2^53 lose precision when converted to `f64`, exactly as they do in the browser's `JSON.parse`. Send large ids as strings.

### Literals

String, number, `true`, `false` and `null` literals map directly. `undefined` is accepted and treated as `null` in both targets, so `x === undefined` compiles to a comparison with `null`. There are no negative literals: `-1` is rejected because `-` is not an operator in the language.

## Truthiness

Conditions in `{#if}` and the operands of `!`, `&&` and `||` use JavaScript's `Boolean(v)`:

| Value | Truthy? |
|---|---|
| `undefined`, `null` | no |
| `false` | no |
| `0`, `-0`, `NaN` | no |
| any other number, including `Infinity` | yes |
| `""` | no |
| any other string, including `"0"` and `"false"` | yes |

## Logical operators return booleans

This is the one place the template language departs from JavaScript on purpose. `a && b` and `a || b` always produce `true` or `false`, never one of their operands. Rust compiles them to `Val::Bool(a.truthy() && b.truthy())`, and the browser code is `!!(a) && !!(b)`, so the targets still agree.

The consequence is that the common JavaScript fallback idiom shows the word `true`:

```rk title=fallback.rk
<script>
  let name = $state("");
</script>
<p>{name || "Anonymous"}</p>
<p>{#if name}{name}{:else}Anonymous{/if}</p>
```

The first paragraph renders `true`. Use `{#if}` with `{:else}` as in the second, or compute the fallback in the resolver.

## Strict equality

`===` and `!==` follow JavaScript's strict equality. Values of different types are never equal, so `1 === "1"` is false.

| Pair | Equal when |
|---|---|
| `undefined`, `undefined` | always |
| `null`, `null` | always |
| two booleans | same value |
| two numbers | numerically equal; `NaN` equals nothing, and `0 === -0` |
| two strings | same characters |
| anything else | never |

`==` and `!=` are rejected at build time with "loose equality is not supported".

## Relational comparison

`<`, `<=`, `>` and `>=` use JavaScript's abstract relational comparison:

1. If both sides are strings, compare them by UTF-16 code unit, as the browser does. Rust's own string ordering compares UTF-8 bytes, which differs for characters outside the Basic Multilingual Plane, so the renderer re-encodes both sides. `"10" < "9"` is true, because `"1"` sorts before `"9"`.
2. Otherwise, convert both sides to numbers and compare. If either side is `NaN`, every operator returns false, including `<=` and `>=`.

Number conversion follows `Number(v)` for the cases the language can produce:

| Value | Number |
|---|---|
| `undefined` | `NaN` |
| `null` | `0` |
| `false`, `true` | `0`, `1` |
| `""` or whitespace only | `0` |
| `"Infinity"`, `"+Infinity"`, `"-Infinity"` | the infinity |
| a decimal or exponent string such as `" 12.5 "`, `"1e3"` | its value, ignoring surrounding whitespace |
| anything else | `NaN` |

So `"10" > 9` is true, since the string converts to 10, and `null < 1` is true.

> [!NOTE]
> Hexadecimal, octal and binary strings such as `"0x10"` convert to `NaN` in Rust but to a number in the browser. Comparing such strings against numbers is not consistent between targets yet. Convert them in the resolver.

## .length

A path ending in `.length` reads the length of the value before it:

| Rust type | `.length` |
|---|---|
| `String`, `str` | UTF-16 code units, as in JavaScript. `"héllo"` is 5 and `"😀"` is 2. |
| `Vec<T>`, `[T]` | number of elements |

Other types, including `Option<String>`, have no `JsLength` implementation and fail to compile. `.length` is only special as the last segment of a path, as in `data.items.length`.

## Number formatting

Interpolated numbers use JavaScript's `Number.prototype.toString()`:

| Number | Text |
|---|---|
| `1.0` | `1` |
| `-0.0` | `0` |
| `0.1 + 0.2` | `0.30000000000000004` |
| `123456.0` | `123456` |
| `1e21` | `1e+21` |
| `1.5e-7` | `1.5e-7` |
| `NaN`, `Infinity`, `-Infinity` | `NaN`, `Infinity`, `-Infinity` |

Between `1e-6` and `1e21` the renderer uses Rust's shortest round-trip formatting, which matches ECMAScript in that range. Outside it, numbers use exponent notation with an explicit `+` on positive exponents. There is no locale formatting such as thousands separators; do that in the resolver.

## Rendering text

An interpolation such as `<p>{data.name}</p>` writes `to_text()` of the value, escaped:

- `null` and `undefined` render as empty text.
- `true` and `false` render as `true` and `false`.
- Numbers render as above.
- The five characters `&`, `<`, `>`, `"` and `'` become `&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#39;`.

There is no way to insert raw HTML from an expression; `{@html}` does not exist.

## Rendering attributes

An attribute whose whole value is one expression, such as `disabled={busy}` or `value={data.name}`, follows these rules:

| Value | Output |
|---|---|
| `true` | bare attribute: ` disabled` |
| `false`, `null`, `undefined` | attribute omitted |
| anything else | ` name="…"` with the text escaped as above |

An attribute that mixes text and expressions, such as `class="card {kind}"`, always writes the attribute and renders each expression as text, so `false` there becomes the word `false` and `null` becomes nothing.

Static attribute values are written as you typed them, entities included, with any `"` replaced by `&quot;`.

## The browser side

The browser does not reimplement any of this. It evaluates the same expressions natively, and the runtime's helpers mirror the Rust output:

| Rust | Browser runtime |
|---|---|
| `js::text` | `esc(v)`: `null` and `undefined` become `""`, otherwise `String(v)`, with the same five replacements |
| `js::attr` | `attr(name, v)`: same rules for `true`, `false`, `null` and `undefined` |
| a field path `data.a.b` | `data?.a?.b`, so a missing value is `undefined` and renders as empty text |

The last row explains a common symptom. In Rust, a field the view does not have is a compile error. In the browser, a field the JSON does not have is silently `undefined`. If a block shows text from the server and goes blank after an update, the JSON field name probably differs from the template's; views must serialise with `#[serde(rename_all = "camelCase")]`. See [Troubleshooting](/docs/troubleshooting/#text-disappears-after-an-update).

## Safe JSON in the page

When the browser needs server data to rebuild a block, the server embeds it in a `<script type="application/json">` element (the [handoff payload](/docs/handoff-payload/)). `html_safe_json` writes `<`, `>` and `&` as `<`, `>` and `&`, and escapes U+2028 and U+2029, so no string value can close the element or be read as markup.

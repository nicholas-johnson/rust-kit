---
title: Expressions
description: The small expression language used inside template braces, why it is small, and what to do instead of calling a function.
section: Core concepts
order: 3
---

Everything inside `{…}` in a template, in an `{#if}` condition, in an `{#each}` list, or in a bound attribute is a **template expression**. Template expressions use a deliberately small language. This page lists all of it and explains the reason for the limits.

Event handlers (`onclick={…}`) are *not* template expressions. They are ordinary JavaScript; see [State and events](/docs/state-and-events/).

## Why the language is small

A template is rendered in two places. Rust renders it for the first response; the browser re-renders parts of it after state changes. The visitor must see **exactly the same text** either way. Otherwise the page would visibly change when JavaScript takes over, or a test against server HTML would pass while the browser showed something else.

So every template expression has to mean the same thing in Rust and in JavaScript. That is easy for reading a field or comparing two numbers. It is impossible in general for calling a function: `formatDate(x)` in the browser depends on the visitor's locale, time zone and JavaScript engine, and Rust cannot run it at all.

Rust-kit draws the line at the operations it can guarantee. In Rust they are evaluated through `rustkit::js::Val`, which implements JavaScript's rules for truthiness, strict equality, relational comparison, `.length`, and number-to-string conversion. The browser evaluates the same expression natively. See [JS value semantics](/docs/js-value-semantics/) for the details.

## What you can write

| Kind | Examples |
| --- | --- |
| Names | `count`, `data`, `item`, `i` |
| Field reads | `data.displayName`, `item.author.name` |
| Length | `data.items.length`, `data.displayName.length` |
| Strings | `"hello"`, `'hello'` (escapes `\n`, `\t`, `\"`, `\'`, `\\`) |
| Numbers | `0`, `42`, `3.5` |
| Literals | `true`, `false`, `null`, `undefined` |
| Not | `!editing` |
| Logic | `a && b`, `a \|\| b` |
| Equality | `step === 2`, `status !== "ok"` |
| Comparison | `count < 10`, `a <= b`, `a > b`, `a >= b` |
| Grouping | `(a \|\| b) && c` |

Operator precedence, from tightest to loosest: `!`, then `<` `<=` `>` `>=`, then `===` `!==`, then `&&`, then `||`. Use parentheses whenever it helps readability.

The names available are `$state` variables declared in the page script, the `let:` binding of an enclosing `<DataSource>` (default `data`), and the item and index of an enclosing `{#each}`. An inner binding shadows an outer one with the same name.

```rk title=expressions.rk
<script>
  import { cart } from "$sources";
  let showDetails = $state(false);
</script>

<DataSource source={cart} load="server" let:data>
  {#if data.items.length === 0}
    <p>Your cart is empty.</p>
  {:else if data.items.length > 10 && !data.isWholesale}
    <p>That is a lot of items. Have you considered a wholesale account?</p>
  {:else}
    <p>{data.items.length} items, {data.totalLabel} in total.</p>
  {/if}
</DataSource>

<button onclick={() => showDetails = !showDetails}>Details</button>
{#if showDetails}
  <p>Prices include tax.</p>
{/if}
```

## What you cannot write

| Not supported | Do this instead |
| --- | --- |
| Calls: `format(x)`, `name.toUpperCase()` | Add a field to the view, computed in the resolver |
| Arithmetic: `a + b`, `price * 2`, `-1` | Compute it in the resolver; for counters, store the number in state |
| Loose equality: `==`, `!=` | Use `===` and `!==` |
| Ternary: `a ? b : c` | Use `{#if a}…{:else}…{/if}` |
| Indexing: `items[0]`, `map[key]` | Expose `firstItem` on the view |
| Template literals, `typeof`, `?.`, `??` | Not available |

Each of these is a compile error that points at the offending token. Here is the most common one:

```rk title=account.rk fail
<script>
  import { account } from "$sources";
</script>

<DataSource source={account} load="server" let:data>
  <h1>Joined {formatDate(data.joined)}</h1>
</DataSource>
```

And loose equality, which has its own hint:

```rk title=status.rk fail
<script>
  let status = $state("idle");
</script>

{#if status == "idle"}<p>Ready.</p>{/if}
```

## Compute values in the resolver

The fix for "I need a function here" is nearly always the same: compute the value once, in Rust, in the source's resolver, and expose it as a field on the view.

```rust title=src/sources.rs
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub display_name: String,
    /// Already formatted for display, e.g. "March 2024".
    pub joined_label: String,
}

pub async fn account(ctx: &Ctx) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(&user.id).await?;
    Ok(AccountView {
        display_name: record.display_name,
        joined_label: format_month(record.joined),
    })
}
```

```rk title=account.rk
<script>
  import { account } from "$sources";
</script>

<DataSource source={account} load="server" let:data>
  <h1>Joined {data.joinedLabel}</h1>
</DataSource>
```

This is better than it might first appear. The formatting runs once per request, in one place, under your control, with one locale. The browser receives a ready-made string, and a test can assert on it.

## Gotchas

### `&&` and `||` return booleans

In JavaScript, `a || b` returns one of its operands. In a Rust-kit template, `&&` and `||` always produce `true` or `false`, in both renderers. So this does **not** do what it would in JavaScript:

```rk title=fallback.rk
<script>
  let name = $state("");
</script>

<p>{name || "Anonymous"}</p>
```

It renders `true`. Write the fallback as a branch instead:

```rk title=fallback-fixed.rk
<script>
  let name = $state("");
</script>

<p>{#if name}{name}{:else}Anonymous{/if}</p>
```

Inside `{#if}` conditions and boolean attributes this difference never matters, which is where you will use `&&` and `||` almost every time.

### `.length` must come last

`.length` is recognised as the last segment of a path. It counts UTF-16 code units for strings, as the browser does, and elements for lists. On the Rust side it works on `String`, `&str`, `Vec<T>` and slices.

### Optional and nested data

A field read on a Rust `Option` that is `None` renders as empty text. Reading *through* an `Option` (`data.address.city` where `address` is `Option<Address>`) does not compile on the Rust side. Flatten what the page needs into the view.

### Comparisons follow JavaScript

`"10" < "9"` is true (strings compare character by character), `"10" > 9` is true (a string compared with a number is converted to a number), and any comparison involving `NaN` is false. These match the browser exactly because the Rust side implements the same algorithm.

### `undefined` means `null`

`undefined` is accepted as a literal and treated as `null`. Use `null` in new code; it says what you mean.

## Related

- [Template syntax](/docs/template-syntax/) for where expressions can appear.
- [Diagnostics](/docs/diagnostics/) for every compile error and its fix.
- [Data sources](/docs/data-sources/) for writing views and resolvers.

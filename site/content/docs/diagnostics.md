---
title: Compiler diagnostics
description: Every error the Rust-kit compiler can report, what causes it, and how to fix it.
section: Reference
order: 1
---

The compiler stops at the first problem in a page and reports it in rustc style: the message, the file, line and column, the source line with a caret, and sometimes a `help:` line. When you build through `build.rs`, every failing page is reported and the build exits with status 1.

```text
error: loose equality is not supported
 --> src/routes/counter.rk:4:12
  |
4 | {#if count == 0}<p>Nothing yet.</p>{/if}
  |            ^
  = help: use `===` or `!==`
```

The samples on this page are real. Each one marked as failing is fed to the compiler when this site is built, and the output under it is the compiler's own text.

> [!NOTE]
> Diagnostics come from four stages, in this order: parsing the markup (`parse.rs`), parsing template expressions (`expr.rs`), reading the page `<script>` (`script.rs`), and generating code (`codegen.rs`). Errors that rustc reports in the generated code, such as a missing field on a view struct, are listed at the end under [Errors from rustc](#errors-from-rustc).

## Expressions

Template expressions run in Rust on the server and in JavaScript in the browser, so the language is small: field reads, `.length`, string, number, boolean and `null` literals, `!`, `&&`, `||`, `===`, `!==`, `<`, `<=`, `>`, `>=` and parentheses. See [Expressions](/docs/expressions/).

### Calls

```rk fail title=joined.rk
<script>
  import { account } from "$sources";
</script>
<DataSource source={account} load="server" let:data>
  <h1>Joined {formatDate(data.joined)}</h1>
</DataSource>
```

Any call, including a method call such as `data.name.toUpperCase()`, is rejected because the Rust renderer cannot run it and still promise the same text as the browser. **Fix:** compute the value in the source's resolver and expose it as a field on the view, then read the field.

### Loose equality

```rk fail title=counter.rk
<script>
  let count = $state(0);
</script>
{#if count == 0}<p>Nothing yet.</p>{/if}
```

`==` and `!=` are rejected because their coercion rules are not implemented in Rust. **Fix:** use `===` or `!==`.

### Other expression errors

| Message | Cause | Fix |
|---|---|---|
| `` `X` is not supported in a template expression `` | A character outside the language, such as `+`, `-`, `?`, `[`, `` ` `` or `;`. A leading minus sign counts, so `-1` is rejected. | Derive the value in the resolver, or restructure the condition. |
| `this token is not supported in a template expression` | A token in the wrong place, such as `)` or `.` where a value was expected. | Check the expression for a missing operand. |
| `empty expression` | `{}` or `{ }` in the template. | Remove it or fill it in. |
| `unexpected token in expression` | Two values side by side, such as `{a b}`. | Add an operator or split into two interpolations. |
| `` expected `)` `` | An unclosed parenthesis. | Close it. |
| `` expected a field name after `.` `` | `data.` or `data.1`. | Field names must be identifiers. |
| `` invalid number `X` `` | A number with two dots, such as `1.2.3`. | Fix the literal. |
| `unterminated string` | A string literal without its closing quote. | Close the quote. |
| `bad escape` | A backslash at the very end of a string. | Remove the backslash or escape it. |

## Markup and blocks

| Message | Cause | Fix |
|---|---|---|
| `` unexpected closing tag `</X>` `` | A closing tag at the top level with no matching opener. | Remove it or add the opening tag. |
| `` `</X>` does not match `<Y>` `` | Elements are closed in the wrong order. | Close the inner element first. |
| `` `<X>` is not closed `` | The file ends, or a block ends, before `</X>`. | Add the closing tag. Void elements such as `<input>` and `<br>` need none. |
| `` unterminated `<X>` `` | The file ends inside a start tag. | Finish the tag with `>`. |
| `unterminated closing tag` | `</div` with no `>`. | Add `>`. |
| `unterminated comment` | `<!--` with no `-->`. | Close the comment. |
| `expected an attribute name` | A stray character, such as `=`, where an attribute should start. | Remove it. |
| `unterminated attribute value` | A quoted attribute value with no closing quote. | Close the quote. |
| `` unclosed `{` in attribute `` | `class="a {x"`. | Close the brace. |
| `` unclosed `{` `` | A `{` in text or an attribute with no matching `}`. An unclosed string inside braces also ends up here, because the scanner skips strings. | Close it. To show a literal brace, use `&#123;`. |
| `` unknown block `{…}` `` | A tag starting with `#`, `/`, `:` or `@` that is not `#if`, `#each`, `:else`, `/if` or `/each`. `{@html …}` and `{#await}` do not exist. | Use one of the supported blocks. |
| `` `{#if}` is not closed with `{/if}` `` | Missing `{/if}`, a `{/each}` in its place, or a second `{:else}` after the final `{:else}`. | Close the block correctly. |
| `` expected `{:else}` or `{:else if …}` `` | Anything else after `:else`, such as `{:else unless x}`. | Use `{:else}` or `{:else if cond}`. |
| `` `{:else}` outside `{#if}` `` | `{:else}` with no open `{#if}`. | Move it inside the `{#if}`. |
| `` `{/if}` without `{#if}` `` / `` `{/each}` without `{#each}` `` | A closing block with nothing to close. | Remove it. |
| `` expected `{#each list as item}` `` | The `as` keyword is missing. | Write `{#each data.items as item}`. |
| `invalid item name` / `invalid index name` | `{#each list as 1}` or `{#each list as item, 2}`. | Use identifiers: `{#each list as item, i}`. |
| `` `{#each}` is not closed with `{/each}` `` | Missing `{/each}`. | Add it. |
| `` `{#each}` needs a list field, such as `data.items` `` | The list is a literal or an operator expression. | Iterate over a field. |

### Scripts and styles

| Message | Cause | Fix |
|---|---|---|
| `` `<script>` must be at the top level of a page `` (or `<style>`) | A script or style nested inside an element. | Move it to the top level. |
| `a page can have only one script of each kind` | Two `<script>` or two `<script context="module">` blocks. | Merge them. |
| `` unclosed `<script>` `` (or `<style>`) | No closing tag. | Add it. |

A page with two `<style>` blocks is not an error: the last one wins.

## Components and data sources

```rk fail title=card.rk
<Card title="Hello" />
```

A tag that starts with an uppercase letter is a component. `<DataSource>` is built in. Any other name comes from an import on its own line, `import Header from "$lib/Header.rk"`. See [Components](/docs/pages-and-routes/#components).

| Message | Cause | Fix |
|---|---|---|
| `` unknown component `<X>` `` | A capitalised tag with no matching import. | Add `import X from "$lib/X.rk"` on its own line, or use a lowercase tag. |
| `` `<X>` needs a `prop` prop `` | The component template reads `prop` and the tag does not pass it. | Write `prop="…"` or `prop={expr}`. |
| `` `<X>` has no prop `name` `` | The tag sets a name the template does not read. | Remove it, or read `{name}` in the component. |
| `` prop `name` must be a string or one `{expression}` `` | A bare attribute, or text mixed with an expression. | Write `name="…"` or `name={expr}`. |
| `` `<X>` does not take `onclick` `` | An event attribute on a component tag. | Write the handler inside the component. |
| `` `<X>` has no `<slot>` for its contents `` | The tag has children and the component has no `<slot />`. | Add a slot, or remove the children. |
| `` a component can have only one `<slot>` `` | Two slots in one component file. | Keep one. |
| `component cycle: …` | A component imports itself, directly or through another file. | Break the cycle. |
| `` `$state` `name` is already declared in `…` `` | The same `$state` name in the page, a layout and a component on one page. | Rename one of them. |
| `` a component cannot set `<title>` `` | A `<title>` in a component file. | Put the title on the page, or on a layout. |
| `` `prerender` belongs on the page, not on a component `` | `export const prerender = true` in a component. | Put it on the page. |
| `` <DataSource> needs `source={…}` `` | `source` is missing. | Add `source={name}`. |
| `` `source` must name an import from "$sources" `` | `source={data.x}` or another non-name expression. | Write `source={account}`: braces and a bare name. |
| `` <DataSource> needs `load="server"` or `load="client"` `` | `load` is missing. | Add it. There is no default. |
| `` `load` must be "server" or "client" `` | Any other value. | Use one of the two. |
| `` unexpected attribute `X` on <DataSource> `` | Anything other than `source={…}`, `load="…"` and `let:name`. A quoted `source="account"` or a braced `load={x}` also lands here. | Remove it, or fix its form. A route parameter is not an attribute on `<DataSource>`. |
| `` invalid `let:` name `` | `let:1x` or similar. | Use an identifier. Without `let:`, the binding is `data`. |
| `` `X` is not imported from "$sources" `` | The page uses a source it never imported. | Add `import { X } from "$sources";` to the script. |

```rk fail title=account.rk
<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
</DataSource>
```

## The page script

The script is read line by line. An `import … from "$sources"` line, an `import Name from "$lib/….rk"` line (or a relative path ending in `.rk`), and each `let x = $state(…)` line must stand alone; everything else is passed to the browser untouched. See [State and events](/docs/state-and-events/).

```rk fail title=two-per-line.rk
<script>
  let open = $state(false); let count = $state(0);
</script>
<p>{count}</p>
```

The whole line is read as one declaration whose initialiser is `false); let count = $state(0`, so the error points at a character that looks fine. **Fix:** one declaration per line.

An import split across lines is not an error in itself: the lines do not match the import pattern, so they are passed to the browser as JavaScript and the first `<DataSource>` that uses the source reports `` `X` is not imported from "$sources" ``. Keep each `$sources` import on one line.

| Message | Cause | Fix |
|---|---|---|
| `` expected `import { … } from "$sources"` `` | A `$sources` import without braces, such as a default import. | Use named imports. A default import of a `.rk` path is a component import, not this error. |
| `` `Name` is already imported `` | Two component imports use the same name. | Import it once. |
| `` `name` cannot be a component tag `` | The component import name does not start with an uppercase letter. | Use `Header`, then write `<Header>`. |
| `` `spec` is not a component path `` | The spec is not `$lib/….rk` or a relative path ending in `.rk`. | Fix the path. `$lib` is `src/lib`, next to `src/routes`. |
| `` cannot read component `spec` `` | The file is not there. | Check the path. |
| `` expected `}` `` | The import line has `{` and `"$sources"` but no `}`. | Close the braces. |
| `renaming a source import is not supported yet` | `import { account as acct }`. | Import the name as it is. |
| `` expected `let name = $state(…)` `` | A `let` line containing `$state(` but no `=`. | Write a normal declaration. |
| `` expected `$state(…)` to be the whole initialiser `` | `let n = $state(0) + 1` or `let n = 1 + $state(0)`. | Assign the call alone. |
| `initial state must be a literal string, number, boolean or null` | `$state(other)`: a name or expression. | Use a literal. Arrays and objects are not supported yet; `$state([])` fails earlier with ``` `[` is not supported ```. |

## Scope and generation

These come from the code generator, which knows where each name comes from and which regions the browser renders.

| Message | Cause | Fix |
|---|---|---|
| `` `X` is not defined here `` | A template name that is not a `$state` variable, a `<DataSource>` binding in scope, or an `{#each}` item or index. Script `const`s and functions are not visible to templates. | Read a field from a source, or declare state. |
| `state values are primitives and have no fields` | `{open.label}` where `open` is state. | Read the state value itself. |
| `` `{#each}` over `$state` is not supported yet `` | State holds only primitives. | Iterate over a source field. |
| `` a `load="server"` source cannot appear inside a region the browser renders `` | A server source inside `{#if}` or `{#each}` that depends on state, or inside a client source. | Load it with `load="client"`, or move it outside the region. |
| `` `X` comes from a server-rendered `{#each}`, so the browser cannot see it here `` | A handler or state-dependent region inside a server-rendered loop reads the loop item. | See below. |

```rk fail title=feed.rk
<script>
  import { activity } from "$sources";
</script>
<DataSource source={activity} load="server">
  {#each data.items as item}
    <button onclick={() => alert(item.description)}>Show</button>
  {/each}
</DataSource>
```

The browser rebuilds a server-rendered region from the handoff payload, which carries source fields but not individual loop items. **Fix:** load the source with `load="client"` so the browser renders the loop, or move the interaction outside the loop.

```rk fail title=late.rk
<script>
  import { account } from "$sources";
  let open = $state(false);
</script>
<button onclick={() => open = true}>Open</button>
{#if open}
  <DataSource source={account} load="server">
    <p>{data.displayName}</p>
  </DataSource>
{/if}
```

### Event handler attributes

Any attribute longer than two characters that starts with `on` is an event handler.

| Message | Cause | Fix |
|---|---|---|
| `` `onX` needs a function, for example `onX={() => …}` `` | A quoted or unquoted string value, such as `onclick="save()"`. | Use a braced JavaScript expression. |
| ``event handlers take a single `{…}` value`` | `onclick="a {f}"`, a quoted value with text around the braces. | Use `onclick={f}`. |

## Errors from rustc

The compiler does not see your Rust types, so some mistakes surface when Cargo compiles the generated module. The error points into `rustkit_generated.rs` in `OUT_DIR`; see [Generated code](/docs/generated-code/) to read it.

| rustc error | Usually means |
|---|---|
| ``no field `display_name` on type …`` | The template reads `data.displayName` and the view has no `display_name` field. Template names are converted from camelCase to snake_case. |
| ``the trait bound `Vec<…>: ToJs` is not satisfied`` | You interpolated a list or struct. Only strings, numbers, booleans and `Option` of those can be shown. See [JS value semantics](/docs/js-value-semantics/). |
| ``the trait bound `…: JsLength` is not satisfied`` | `.length` on something that is not a `String`, `str`, `Vec` or slice, such as an `Option<String>`. |
| ``cannot find function `x` in module `crate::sources` `` | A page imports a source with no resolver of that name. |
| ``cannot find type `AppState` in the crate root`` | `build_script()` assumes `crate::AppState` and `crate::sources`. Use [`compile`](/docs/compiler-api/) to choose other paths. |

If the generated code compiles but the browser shows empty text where the server showed a value, the view probably serialises with snake_case names. See [Troubleshooting](/docs/troubleshooting/#text-disappears-after-an-update).

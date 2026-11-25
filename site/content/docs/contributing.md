---
title: Contributing
description: How the repository is laid out, how to run the tests, how the website is built, and how code samples in the docs are verified.
section: Project
order: 4
---

Rust-kit is small on purpose, and the whole codebase is readable in an afternoon. This page explains where things live, how to check your change, and how the documentation stays honest.

## Repository layout

```text
rust-kit/
├── Cargo.toml                     workspace: edition 2024, version 0.1.0
├── README.md
├── crates/
│   ├── rustkit-compiler/          build-time compiler
│   │   └── src/
│   │       ├── lib.rs             entry points, route mapping, asset hashing, manifest output
│   │       ├── parse.rs           .rk parser: scripts, style, title, template tree
│   │       ├── expr.rs            template expression parser and its diagnostics
│   │       ├── script.rs          page script: $sources imports, component imports, $state, JS identifier scanner
│   │       ├── component.rs       load and inline imported .rk components
│   │       ├── route.rs           layouts, the one dynamic segment, component composition
│   │       ├── codegen.rs         Rust and JavaScript generation, blocks, handoff payload
│   │       ├── runtime.js         the shared browser runtime
│   │       └── tests.rs           compiler tests
│   └── rustkit/                   runtime linked into your app
│       └── src/
│           ├── app.rs             App, router, actions, endpoints, document shell
│           ├── context.rs         RequestContext, users, CSRF, login/logout
│           ├── session.rs         in-memory session store, tokens
│           ├── error.rs           Error and its HTTP mapping
│           └── js.rs              JavaScript value semantics for the Rust renderer
├── examples/
│   └── account-demo/              reference app and acceptance tests
└── site/                          this website
    ├── content/docs/              documentation pages (Markdown)
    ├── content/tutorials/         tutorials (Markdown)
    ├── src/                       static site generator
    └── static/                    CSS, JS and images for the site
```

### How the pieces relate

- `rustkit-compiler` has **no dependencies**. It reads `.rk` text and writes Rust and JavaScript source as strings. Keep it that way; it runs in every user's build script.
- `rustkit` is what generated code links against. Anything the generated Rust calls must be public here. Items used only by generated code live under `rustkit::__private`.
- `runtime.js` is embedded into the compiler with `include_str!` and shipped to browsers. Its size matters; it is counted in every interactive page's JavaScript bytes.
- `examples/account-demo` is both documentation and the integration test bed. Its acceptance tests check the four properties from the [design proposal](/docs/design/).
- `site` depends on `rustkit-compiler` so it can compile every `rk` code sample in the docs.

## Building and testing

You need a Rust toolchain with edition 2024 support (1.85 or newer).

```bash
cargo test
```

This runs everything in the workspace:

| Tests | Where | What they check |
| --- | --- | --- |
| Compiler unit tests | `crates/rustkit-compiler/src/tests.rs`, plus `#[cfg(test)]` modules in `expr.rs` and `script.rs` | The proposal's account page compiles; static pages ship no JavaScript; key diagnostics fire with the right position and help text; the state rewrite |
| Runtime unit tests | `crates/rustkit/src/js.rs` | Number formatting, truthiness and comparison match JavaScript; the payload cannot close its `<script>` |
| Acceptance tests | `examples/account-demo/tests/acceptance.rs` | The real router, in process: prerendering, one upstream read per page, minimal payload, per-user isolation, 401/403 handling, CSRF and origin checks, escaping |

Run one crate's tests with `-p`:

```bash
cargo test -p rustkit-compiler
cargo test -p account-demo
```

The acceptance tests drive the router with `tower::ServiceExt::oneshot`, so they need no network and no browser. Browser behaviour (handler attachment, block updates, client loading) was checked by hand in Chromium for this release; there is no automated browser test yet. If your change affects `runtime.js`, say in your pull request how you tested it in a browser.

To see your change in a running app:

```bash
cargo run -p account-demo -- serve 127.0.0.1:3000
```

## Guidelines for changes

- **Keep both renderers in agreement.** Any change to template semantics must change the Rust walk and the JavaScript walk in `codegen.rs` together, and anything the Rust side evaluates must go through `rustkit::js` so it matches the browser. Add a test showing both produce the same text.
- **Prefer a diagnostic to a surprise.** If a construct cannot be supported correctly yet, reject it at compile time with a message and a `help` line that tells the author what to do instead. The existing diagnostics in `expr.rs` and `codegen.rs` show the style: say what is wrong, then why, then the fix.
- **Do not grow the payload or the endpoint surface by accident.** The acceptance tests assert exactly what reaches the browser. If your change sends more, explain why.
- **Update the docs.** If behaviour changes, update the relevant page under `site/content/docs/`, and the [roadmap](/docs/roadmap/) if a gap closes.

## Building the website

The site is generated by the `site` crate from Markdown in `site/content/`:

```bash
cargo run -p site -- build site/dist
```

The generator:

1. Reads each Markdown file's front matter (`title`, `description`, `section`, `order`). A missing front matter block or title, or a docs `section` that is not one of the known sections, fails the build.
2. Renders Markdown with heading anchors, callouts (`> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`), tables and highlighted code.
3. **Compiles every `rk` code sample** with the real compiler.
4. Checks every root-relative link and `#anchor` against the pages it built.
5. Writes HTML, a search index and a sitemap.

Open `site/dist/index.html` in a browser, or serve the directory with any static file server, to review the result.

### Writing a doc page

Create `site/content/docs/<slug>.md`:

```markdown
---
title: Page title
description: One sentence shown under the title and in search.
section: Core concepts
order: 3
---

Introductory paragraph.

## First section
```

Use `##` and `###` headings only; the title comes from front matter. Link to other pages with root-relative URLs such as `/docs/template-syntax/` or `/docs/template-syntax/#each-blocks`. An anchor is the heading text in lowercase, with every run of other characters replaced by `-`.

### How `rk` samples are verified

Every fenced block tagged `rk` is compiled when the site is built, with the same `compile_page` function your build script uses. The page shows the sample with the generated Rust and JavaScript underneath, or a note that the page ships no JavaScript. That is how these docs stay accurate as the compiler changes: a sample that stops compiling breaks the site build.

Flags after the language control this:

| Info string | Behaviour |
| --- | --- |
| `rk title=counter.rk` | Compiled; must succeed. The title labels the sample. |
| `rk title=bad.rk fail` | Compiled; must **fail**. The rendered diagnostic is shown under it. If it compiles, the build fails. |
| `rk partial` | Not compiled. Use for fragments that are not a whole page. |

A sample must be a complete page, but data sources do not need to exist: `import { anything } from "$sources"` compiles, because the samples are only checked by `rustkit-compiler`, not by `rustc`. That also means a sample can reference view fields that no real resolver has. Keep samples realistic anyway; readers copy them.

`compile_page` checks one file. It does not read `src/lib`, so a sample that imports a component must be marked `partial` (or `fail`, if the point is the diagnostic). Compiling a routes directory is what loads component files.

## Reporting problems

When you report a bug, include the `.rk` file (or the smallest page that reproduces it), the full diagnostic or the wrong output, and what you expected. For a mismatch between the server HTML and what the browser renders after an update, include both; that class of bug is the one Rust-kit most wants to hear about.

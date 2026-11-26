# {{name}}

A [Rust-kit](https://rust-kit.com) application.

```bash
rustkit dev      # run on http://127.0.0.1:3000, rebuilding and reloading on save
rustkit check    # compile every page and report errors
rustkit routes   # list routes and the JS and CSS each ships
cargo test       # check what the server sends
rustkit build    # prerender into dist/
rustkit serve    # run a release build
```

```text
build.rs              compiles src/routes into Rust and JavaScript
src/lib.rs            AppState and the app
src/main.rs           `serve [addr]` and `build [dir]`
src/sources.rs        data-source resolvers, imported by pages from "$sources"
src/routes/*.rk       one page per file; layout.rk wraps the pages in its folder
src/lib/*.rk          components, imported from "$lib/Name.rk"
tests/pages.rs        requests against the real router
```

Add more with `rustkit add page about`, `rustkit add page blog/[slug]`,
`rustkit add layout blog` and `rustkit add component Button`.

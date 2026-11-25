use super::*;

const OPTS: Options = Options {
    state_type: "crate::AppState",
    sources_module: "crate::sources",
};

/// The account page exactly as written in the design proposal.
const PROPOSAL_PAGE: &str = r#"<script>
  import { account, activity } from "$sources";
  let editing = $state(false);
  let showActivity = $state(false);
</script>

<DataSource source={account} load="server" let:data>
  <h1>Hello, {data.displayName}</h1>
  <button onclick={() => editing = !editing}>Edit profile</button>

  {#if editing}
    <form method="post" action="/account/profile">
      <label>Display name <input name="name" value={data.displayName} /></label>
      <button>Save</button>
    </form>
  {/if}
</DataSource>

<button onclick={() => showActivity = true}>Show recent activity</button>
{#if showActivity}
  <DataSource source={activity} load="client" let:data>
    {#each data.items as item}
      <p>{item.description}</p>
    {/each}
  </DataSource>
{/if}

<style>
  h1 { margin-block-end: 1rem; }
</style>
"#;

#[test]
fn proposal_page_compiles() {
    let p = compile_page(PROPOSAL_PAGE, &OPTS).unwrap_or_else(|d| panic!("{}", d.render("account.rk", PROPOSAL_PAGE)));
    assert_eq!(p.server_sources, ["account"]);
    assert_eq!(p.client_sources, ["activity"]);
    let js = p.js.expect("interactive page has a module");
    // Handlers assign through the reactive store.
    assert!(js.contains("$s.editing = !$s.editing"), "{js}");
    // The editor's block is rebuilt from the payload, not by reading the account again.
    assert!(js.contains("data: P.s[\"account\"]"), "{js}");
    assert!(!js.contains("/_rk/source/account"));
    // Only the field the browser needs crosses into the page.
    assert!(p.rust.contains("m.insert(\"displayName\".into(), to_value(&src_account.display_name)?)"), "{}", p.rust);
    // The form posted from a browser-rendered region still carries a CSRF token.
    assert!(js.contains("r.csrf()"));
    assert!(p.css.unwrap().contains("margin-block-end"));
}

#[test]
fn static_page_ships_no_javascript() {
    let src = "<script context=\"module\">export const prerender = true;</script>\n<title>Notes</title><article><h1>Static</h1><p>Text.</p></article>";
    let p = compile_page(src, &OPTS).unwrap();
    assert!(p.js.is_none());
    assert!(p.prerender);
}

#[test]
fn formatter_in_initial_render_is_diagnosed() {
    let src = "<script>import { account } from \"$sources\";</script>\n<DataSource source={account} load=\"server\" let:data>\n  <h1>Joined {formatDate(data.joined)}</h1>\n</DataSource>";
    let d = compile_page(src, &OPTS).err().expect("must fail");
    let msg = d.render("account.rk", src);
    assert!(msg.contains("account.rk:3:15"), "{msg}");
    assert!(msg.contains("`formatDate(…)` cannot be rendered by the Rust server renderer"), "{msg}");
    assert!(msg.contains("resolver"), "{msg}");
}

#[test]
fn server_source_inside_browser_region_is_diagnosed() {
    let src =
        "<script>\nimport { account } from \"$sources\";\nlet open = $state(false);\n</script>\n{#if open}<DataSource source={account} load=\"server\"><p>{data.displayName}</p></DataSource>{/if}";
    let d = compile_page(src, &OPTS).err().expect("must fail");
    assert!(d.msg.contains("cannot appear inside a region the browser renders"), "{}", d.msg);
}

#[test]
fn handler_reading_a_server_loop_item_is_diagnosed() {
    let src = "<script>import { activity } from \"$sources\";</script>\n<DataSource source={activity} load=\"server\">{#each data.items as item}<button onclick={() => alert(item.id)}>x</button>{/each}</DataSource>";
    let d = compile_page(src, &OPTS).err().expect("must fail");
    assert!(d.msg.contains("server-rendered `{#each}`"), "{}", d.msg);
}

#[test]
fn unknown_names_and_missing_imports_are_diagnosed() {
    let d = compile_page("<p>{nope}</p>", &OPTS).err().unwrap();
    assert!(d.msg.contains("`nope` is not defined"));
    let d = compile_page("<DataSource source={account} load=\"server\"></DataSource>", &OPTS).err().unwrap();
    assert!(d.msg.contains("not imported"));
}

#[test]
fn production_runtime_leaves_out_dev_hooks() {
    let prod = crate::runtime_js(false);
    let dev = crate::runtime_js(true);
    assert!(!prod.contains("__rkDev") && !prod.contains("dev:"));
    assert!(dev.contains("__rkDev") && dev.contains("dev.attach(") && !dev.contains("// dev:start"));
    // Every hook line is in a marked region.
    assert_eq!(crate::RUNTIME_JS.matches("// dev:start").count(), crate::RUNTIME_JS.matches("// dev:end").count());
}

/// A stand-in Tailwind CLI: records its arguments and stdin, prints CSS.
#[cfg(unix)]
fn fake_tailwind(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let cli = dir.join("fake-tailwindcss");
    std::fs::write(&cli, "#!/bin/sh\necho \"$@\" > args.txt\ncat > stdin.txt\nprintf '.p-4{padding:1rem}'\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    cli
}

#[cfg(unix)]
#[test]
fn tailwind_stylesheet_is_linked_before_page_css() {
    let dir = std::env::temp_dir().join(format!("rk-tailwind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/routes")).unwrap();
    std::fs::write(dir.join("src/routes/index.rk"), "<p class=\"p-4\">Hi</p>\n<style>p { color: red; }</style>\n").unwrap();
    std::fs::write(dir.join("src/routes/plain.rk"), "<p class=\"p-4\">Hi</p>\n").unwrap();
    let cfg = Config {
        routes_dir: dir.join("src/routes"),
        out_dir: dir.join("out"),
        state_type: "crate::AppState".into(),
        sources_module: "crate::sources".into(),
        crate_dir: dir.clone(),
        tailwind: Some(Tailwind {
            cli: Some(fake_tailwind(&dir)),
            input: None,
            minify: true,
        }),
        dev: false,
    };
    let summaries = compile(&cfg).unwrap();

    // Ran once in the crate dir, reading the default input from stdin.
    assert_eq!(std::fs::read_to_string(dir.join("args.txt")).unwrap().trim(), "--input - --minify");
    assert!(std::fs::read_to_string(dir.join("stdin.txt")).unwrap().contains("@source \"./src/routes\""));

    let css = ".p-4{padding:1rem}";
    let name = format!("tailwind.{}.css", hash(css));
    assert_eq!(std::fs::read_to_string(dir.join("out/rk_assets").join(&name)).unwrap(), css);
    let generated = std::fs::read_to_string(dir.join("out/rustkit_generated.rs")).unwrap();
    let tw_url = format!("/_rk/assets/{name}");
    // A page with its own <style> links Tailwind first; a page without one links only Tailwind.
    assert!(generated.contains(&format!("const CSS: &[&str] = &[\"{tw_url}\", \"/_rk/assets/index.")));
    assert!(generated.contains(&format!("const CSS: &[&str] = &[\"{tw_url}\"];")));
    let plain = summaries.iter().find(|s| s.path == "/plain").unwrap();
    assert_eq!(plain.css_bytes, css.len());

    // A user stylesheet at src/tailwind.css is passed as a file instead.
    std::fs::write(dir.join("src/tailwind.css"), "@import \"tailwindcss\";\n").unwrap();
    compile(&cfg).unwrap();
    let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
    assert!(args.trim().ends_with("src/tailwind.css --minify"), "{args}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn compile_tree(label: &str, files: &[(&str, &str)]) -> Result<String, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("rk-route-{label}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, src) in files {
        let path = dir.join("routes").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, src).unwrap();
    }
    let cfg = Config {
        routes_dir: dir.join("routes"),
        out_dir: dir.join("out"),
        crate_dir: dir.clone(),
        state_type: "crate::AppState".into(),
        sources_module: "crate::sources".into(),
        dev: false,
        tailwind: None,
    };
    let result = compile(&cfg).map(|_| std::fs::read_to_string(dir.join("out/rustkit_generated.rs")).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[test]
fn layouts_wrap_pages_and_are_not_routes() {
    let generated = compile_tree(
        "layouts",
        &[
            ("layout.rk", "<header id=\"site\">Site</header>\n<slot />\n<style>\nbody { margin: 0; }\n</style>\n"),
            ("blog/layout.rk", "<nav id=\"blog\">Blog</nav>\n<slot />\n<title>From layout</title>\n"),
            ("blog/index.rk", "<title>Blog</title>\n<h1>Posts</h1>\n"),
            ("index.rk", "<title>Home</title>\n<p>Hello</p>\n"),
        ],
    )
    .unwrap();
    assert!(generated.contains("path: \"/blog\""), "{generated}");
    assert!(generated.contains("path: \"/\""));
    assert!(!generated.contains("path: \"/layout\""));
    assert!(!generated.contains("path: \"/blog/layout\""));
    let blog = generated.split("pub mod page_blog").nth(1).unwrap();
    let site = blog.find("Site").unwrap();
    let nav = blog.find("Blog").unwrap();
    let posts = blog.find("Posts").unwrap();
    assert!(site < nav && nav < posts, "outer layout, then inner, then the page");
    assert!(blog.contains("document(\"Blog\""));
    // The pages have no <style> of their own; the root layout's sheet is linked.
    assert!(generated.contains("const CSS: &[&str] = &[\"/_rk/assets/blog_index."));
    assert!(generated.contains("const CSS: &[&str] = &[\"/_rk/assets/index."));
    let home = generated.split("pub mod page_index").nth(1).unwrap();
    assert!(home.contains("Site"));
    assert!(!home.contains("id=\\\"blog\\\""));
}

#[test]
fn a_dynamic_segment_is_part_of_the_page_type() {
    let generated = compile_tree(
        "param",
        &[
            (
                "blog/[slug].rk",
                "<script>\nimport { post } from \"$sources\";\n</script>\n<DataSource source={post} load=\"server\" let:data>\n<p>{slug} {data.title}</p>\n</DataSource>\n",
            ),
            ("items/[type]/index.rk", "<p>{type}</p>\n"),
        ],
    )
    .unwrap();
    assert!(generated.contains("path: \"/blog/{slug}\""), "{generated}");
    assert!(generated.contains("param: Some(\"slug\")"));
    assert!(generated.contains("pub struct Params"));
    assert!(generated.contains("pub slug: ::std::string::String"));
    assert!(generated.contains("crate::sources::post(ctx, params.slug.as_str())"));
    assert!(generated.contains("__param: Option<&str>"));
    assert!(generated.contains("path: \"/items/{type}\""));
    assert!(generated.contains("params.r#type"));
}

#[test]
fn a_source_takes_the_parameter_only_when_every_import_does() {
    let err = compile_tree(
        "mixed",
        &[
            (
                "index.rk",
                "<script>\nimport { post } from \"$sources\";\n</script>\n<DataSource source={post} load=\"server\" let:data><p>{data.title}</p></DataSource>\n",
            ),
            (
                "blog/[slug].rk",
                "<script>\nimport { post } from \"$sources\";\n</script>\n<DataSource source={post} load=\"server\" let:data><p>{data.title}</p></DataSource>\n",
            ),
        ],
    )
    .unwrap_err();
    assert!(err.contains("source `post` is imported with"), "{err}");
    assert!(err.contains("no parameter"), "{err}");

    let err = compile_tree(
        "two-names",
        &[
            (
                "blog/[slug].rk",
                "<script>\nimport { post } from \"$sources\";\n</script>\n<DataSource source={post} load=\"server\" let:data><p>{data.title}</p></DataSource>\n",
            ),
            (
                "users/[id].rk",
                "<script>\nimport { post } from \"$sources\";\n</script>\n<DataSource source={post} load=\"server\" let:data><p>{data.title}</p></DataSource>\n",
            ),
        ],
    )
    .unwrap_err();
    assert!(err.contains("`slug` from"), "{err}");
    assert!(err.contains("`id` from"), "{err}");
}

#[test]
fn route_files_that_are_not_one_parameter_are_rejected() {
    let cases = [
        ("blog/[slug]/[id].rk", "only one `[param]`"),
        ("blog/[...slug].rk", "rest parameters"),
        ("blog/[[slug]].rk", "optional segments"),
        ("blog/[slug-name].rk", "not a valid parameter name"),
        ("blog/[self].rk", "not a valid parameter name"),
        ("layout.rk", "`layout.rk` needs a `<slot>`"),
    ];
    for (file, needle) in cases {
        let err = compile_tree("bad", &[(file, "<p>No</p>\n"), ("index.rk", "<p>Home</p>\n")]).unwrap_err();
        assert!(err.contains(needle), "{file}: {err}");
    }

    let err = compile_tree("two-slots", &[("layout.rk", "<slot />\n<slot />\n")]).unwrap_err();
    assert!(err.contains("only one `<slot>`"), "{err}");

    let err = compile_tree("page-slot", &[("index.rk", "<slot />\n")]).unwrap_err();
    assert!(err.contains("`<slot>` is only valid in `layout.rk`"), "{err}");

    let err = compile_tree("dup", &[("blog/[slug].rk", "<p>A</p>\n"), ("blog/[slug]/index.rk", "<p>B</p>\n")]).unwrap_err();
    assert!(err.contains("both serve `/blog/{slug}`"), "{err}");

    let err = compile_tree(
        "pre-layout",
        &[
            ("layout.rk", "<script context=\"module\">\nexport const prerender = true;\n</script>\n<slot />\n"),
            ("index.rk", "<p>Home</p>\n"),
        ],
    )
    .unwrap_err();
    assert!(err.contains("`prerender` belongs on the page"), "{err}");

    let err = compile_tree(
        "pre-dyn",
        &[("blog/[slug].rk", "<script context=\"module\">\nexport const prerender = true;\n</script>\n<p>{slug}</p>\n")],
    )
    .unwrap_err();
    assert!(err.contains("no single page to prerender"), "{err}");
}

#[test]
fn tailwind_without_a_cli_explains_how_to_install_it() {
    let dir = std::env::temp_dir().join(format!("rk-tailwind-missing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tw = Tailwind {
        cli: Some(dir.join("no-such-tailwindcss")),
        input: None,
        minify: true,
    };
    let err = tw.run(&dir).unwrap_err();
    assert!(err.contains("running"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

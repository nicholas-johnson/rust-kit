use super::*;

const OPTS: Options = Options { state_type: "crate::AppState", sources_module: "crate::sources" };

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
    let src = "<script>\nimport { account } from \"$sources\";\nlet open = $state(false);\n</script>\n{#if open}<DataSource source={account} load=\"server\"><p>{data.displayName}</p></DataSource>{/if}";
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
        tailwind: Some(Tailwind { cli: Some(fake_tailwind(&dir)), input: None, minify: true }),
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

#[test]
fn tailwind_without_a_cli_explains_how_to_install_it() {
    let dir = std::env::temp_dir().join(format!("rk-tailwind-missing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let tw = Tailwind { cli: Some(dir.join("no-such-tailwindcss")), input: None, minify: true };
    let err = tw.run(&dir).unwrap_err();
    assert!(err.contains("running"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

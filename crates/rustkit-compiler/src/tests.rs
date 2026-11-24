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

#[test]
fn production_runtime_leaves_out_dev_hooks() {
    let prod = crate::runtime_js(false);
    let dev = crate::runtime_js(true);
    assert!(!prod.contains("__rkDev") && !prod.contains("dev:"));
    assert!(dev.contains("__rkDev") && dev.contains("dev.attach(") && !dev.contains("// dev:start"));
    // Every hook line is in a marked region.
    assert_eq!(crate::RUNTIME_JS.matches("// dev:start").count(), crate::RUNTIME_JS.matches("// dev:end").count());
}

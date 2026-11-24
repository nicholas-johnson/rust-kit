//! The home page. The showcase code, the byte counts and the diagnostic are
//! produced by the real compiler when the site is built.

use std::path::Path;

use rustkit_compiler::{Options, compile_page, runtime_js};

use crate::markdown::indent;
use crate::templates::{Ctx, MARK_SVG, footer, head, header};
use crate::{esc, highlight::highlight};

/// The account page from the design proposal, verbatim.
const SHOWCASE: &str = r#"<script>
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
"#;

const BROKEN: &str = r#"<script>
  import { account } from "$sources";
</script>
<DataSource source={account} load="server" let:data>
  <h1>Joined {formatDate(data.joined)}</h1>
</DataSource>
"#;

fn code(lang: &str, label: &str, src: &str) -> String {
    format!(
        "<figure class=\"code\"><figcaption><span class=\"lang\">{}</span><button class=\"copy\" type=\"button\">Copy</button></figcaption><pre><code>{}</code></pre></figure>",
        esc(label),
        highlight(lang, src.trim_end())
    )
}

fn flow_svg() -> &'static str {
    r##"<svg viewBox="0 0 1000 330" role="img" aria-labelledby="flow-title flow-desc">
<title id="flow-title">One request through Rust-kit</title>
<desc id="flow-desc">The browser requests the account page. Rust checks the session, runs the account resolver and renders HTML, which the browser can read immediately. The page module then attaches handlers. Clicking Show recent activity makes one request to the generated activity endpoint, which runs the resolver and returns JSON that updates only that region.</desc>
<defs>
<marker id="ah-r" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 10 5 0 10z" fill="#ff7a2f"/></marker>
<marker id="ah-t" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0 10 5 0 10z" fill="#2dd4bf"/></marker>
</defs>
<rect x="0" y="20" width="1000" height="110" rx="16" fill="#ff7a2f" fill-opacity=".07" stroke="#ff7a2f" stroke-opacity=".3"/>
<rect x="0" y="200" width="1000" height="110" rx="16" fill="#2dd4bf" fill-opacity=".07" stroke="#2dd4bf" stroke-opacity=".3"/>
<text x="20" y="48" class="lane-label" fill="#ff7a2f">Server · Rust</text>
<text x="20" y="228" class="lane-label" fill="#2dd4bf">Browser · JavaScript</text>

<g class="pulse"><rect x="40" y="240" width="130" height="50" rx="10" fill="var(--panel-2)" stroke="#2dd4bf"/></g>
<text x="105" y="262" text-anchor="middle" class="lbl">GET /account</text><text x="105" y="280" text-anchor="middle" class="sub">cookie: rk_session</text>

<rect x="200" y="62" width="140" height="50" rx="10" fill="var(--panel-2)" stroke="#ff7a2f"/>
<text x="270" y="84" text-anchor="middle" class="lbl">Session + context</text><text x="270" y="102" text-anchor="middle" class="sub">require_user()</text>
<rect x="360" y="62" width="150" height="50" rx="10" fill="var(--panel-2)" stroke="#ff7a2f"/>
<text x="435" y="84" text-anchor="middle" class="lbl">Resolvers in parallel</text><text x="435" y="102" text-anchor="middle" class="sub">account(ctx)</text>
<rect x="530" y="62" width="140" height="50" rx="10" fill="var(--panel-2)" stroke="#ff7a2f"/>
<text x="600" y="84" text-anchor="middle" class="lbl">Native render</text><text x="600" y="102" text-anchor="middle" class="sub">escaped HTML</text>

<rect x="360" y="240" width="150" height="50" rx="10" fill="var(--panel-2)" stroke="#2dd4bf"/>
<text x="435" y="262" text-anchor="middle" class="lbl">Readable page</text><text x="435" y="280" text-anchor="middle" class="sub">before any script</text>
<rect x="530" y="240" width="140" height="50" rx="10" fill="var(--panel-2)" stroke="#2dd4bf"/>
<text x="600" y="262" text-anchor="middle" class="lbl">Handlers attach</text><text x="600" y="280" text-anchor="middle" class="sub">no second read</text>
<rect x="700" y="240" width="130" height="50" rx="10" fill="var(--panel-2)" stroke="#2dd4bf"/>
<text x="765" y="262" text-anchor="middle" class="lbl">Show activity</text><text x="765" y="280" text-anchor="middle" class="sub">click</text>
<rect x="760" y="62" width="200" height="50" rx="10" fill="var(--panel-2)" stroke="#ff7a2f"/>
<text x="860" y="84" text-anchor="middle" class="lbl">/_rk/source/activity</text><text x="860" y="102" text-anchor="middle" class="sub">same resolver, same session</text>
<rect x="850" y="240" width="130" height="50" rx="10" fill="var(--panel-2)" stroke="#2dd4bf"/>
<text x="915" y="262" text-anchor="middle" class="lbl">Region updates</text><text x="915" y="280" text-anchor="middle" class="sub">only that block</text>

<path class="wire" d="M172 258 C205 240 222 170 238 116" stroke="#2dd4bf" marker-end="url(#ah-t)"/>
<path class="wire" d="M342 87 H356" stroke="#ff7a2f" marker-end="url(#ah-r)"/>
<path class="wire" d="M512 87 H526" stroke="#ff7a2f" marker-end="url(#ah-r)"/>
<path class="wire" d="M600 114 C600 170 450 180 438 236" stroke="#ff7a2f" marker-end="url(#ah-r)"/>
<path class="wire" d="M512 265 H526" stroke="#2dd4bf" marker-end="url(#ah-t)"/>
<path class="wire" d="M672 265 H696" stroke="#2dd4bf" marker-end="url(#ah-t)"/>
<path class="wire" d="M770 238 C780 170 820 160 840 114" stroke="#2dd4bf" marker-end="url(#ah-t)"/>
<path class="wire" d="M900 114 C920 170 920 200 915 236" stroke="#ff7a2f" marker-end="url(#ah-r)"/>
<text x="690" y="160" class="sub">HTML</text><text x="152" y="180" class="sub">request</text><text x="935" y="180" class="sub">JSON</text>
</svg>"##
}

const ICONS: [&str; 6] = [
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h16M4 10h10M4 15h16M4 20h7"/></svg>"#,
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M6.5 17.5 17.5 6.5"/></svg>"#,
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h5m4 0h5M12 5v14"/><circle cx="12" cy="12" r="2.5"/></svg>"#,
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3 4 7v5c0 4.5 3.4 8.3 8 9 4.6-.7 8-4.5 8-9V7z"/><path d="m9 12 2 2 4-4"/></svg>"#,
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 4H5v16h4M15 4h4v16h-4"/><path d="M10 12h4"/></svg>"#,
    r#"<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 20h18"/><path d="M6 16v-5M11 16V7M16 16v-8M21 16V4"/></svg>"#,
];

pub fn render(ctx: &Ctx, _root: &Path) -> String {
    let opts = Options { state_type: "crate::AppState", sources_module: "crate::sources" };
    let page = compile_page(SHOWCASE, &opts).expect("showcase compiles");
    let js = page.js.clone().expect("showcase is interactive").replace("__RUNTIME__", "runtime.js");
    let rust = indent(&page.rust);
    let fields = page.rust.matches("m.insert(").count();
    let runtime = runtime_js(false).len();
    let diag = compile_page(BROKEN, &opts).err().expect("broken sample fails").render("src/routes/account.rk", BROKEN);

    let mut s = head(
        ctx,
        "",
        "Rust-kit compiles a page into a native Rust renderer and just enough browser JavaScript. Visitors read real HTML before a single script runs.",
        "/",
    );
    s.push_str("<body class=\"landing\">\n<input type=\"checkbox\" id=\"nav-open\" class=\"nav-open\" hidden>\n");
    s.push_str(&header(""));
    s.push_str("<main id=\"main\">");

    // Hero
    s.push_str(&format!(
        r##"<section class="hero"><div class="grid-bg"></div><div class="hero-inner">
<div>
  <a class="badge" href="/docs/roadmap/"><b>v0.1</b> First milestone of the design proposal →</a>
  <h1><span class="r">Rust renders.</span><br><span class="b">The browser responds.</span></h1>
  <p class="sub">Write a page as a template, a script and a stylesheet. Rust-kit compiles it into a native Rust renderer for the first response and just enough JavaScript for the parts that move.</p>
  <div class="cta"><a class="btn primary" href="/docs/getting-started/">Get started <span aria-hidden="true">→</span></a><a class="btn" href="/tutorials/">Take the tutorials</a></div>
  <div class="install"><span>$</span> cargo run -p account-demo -- serve</div>
</div>
<div class="hero-art" aria-hidden="true"><div class="orbit"></div><div class="big-mark">{MARK_SVG}</div>
  <span class="tag-float t1">fn render(ctx)</span><span class="tag-float t2">&lt;button onclick&gt;</span><span class="tag-float t3">0 B JS · static</span></div>
</div></section>"##
    ));

    // Stats
    s.push_str(&format!(
        r#"<section class="band reveal" aria-label="By the numbers"><div class="stats">
<div class="stat"><b class="r">0 B</b><span>of JavaScript on a page with no state or handlers</span></div>
<div class="stat"><b class="g">{} B</b><span>page module for the account page, plus a {:.1} KB shared runtime</span></div>
<div class="stat"><b class="r">{fields}</b><span>server field handed to the browser. The compiler sends only what browser code reads</span></div>
<div class="stat"><b class="g">1</b><span>upstream read per page view, asserted by the acceptance tests</span></div>
</div></section>"#,
        js.len(),
        runtime as f64 / 1024.0
    ));

    // Showcase
    s.push_str(r#"<section class="band reveal" id="one-file"><div class="band-head"><p class="eyebrow">One file, two targets</p><h2>Write the page once. Get a server and a browser out of it.</h2><p>This is the account page from the design proposal. On the right is exactly what the compiler produced for it when this site was built.</p></div><div class="showcase">"#);
    s.push_str(&code("rk", "src/routes/account.rk", SHOWCASE));
    s.push_str(&format!(
        r#"<div><div class="tabs"><input type="radio" name="out" id="tab-rs" checked><input type="radio" name="out" id="tab-js">
<div class="tab-labels"><label for="tab-rs">Rust renderer<small>{} lines</small></label><label for="tab-js">Browser module<small>{} B</small></label></div>
<div class="panel p-rs">{}</div><div class="panel p-js">{}</div></div>
<p class="showcase-note">The greeting is rendered by Rust and sent as HTML. Only the two <code>{{#if}}</code> blocks get browser render functions, and the editor reads <code>displayName</code> from the page instead of asking the server again.</p></div>"#,
        rust.lines().count(),
        js.len(),
        code("rust", "generated · render()", &rust),
        code("js", "generated · account.js", &indent(&js))
    ));
    s.push_str("</div></section>");

    // Flow
    s.push_str(r#"<section class="band reveal" id="lifecycle"><div class="band-head"><p class="eyebrow">The lifecycle</p><h2>Each language runs where it is best.</h2><p>Rust handles requests, sessions, service calls and the first render. Your JavaScript runs in the browser. Production needs no server-side JavaScript engine and no browser Wasm runtime.</p></div><div class="flow">"#);
    s.push_str(flow_svg());
    s.push_str(r#"<div class="legend"><span><i style="background:#ff7a2f"></i>Rust, on your server</span><span><i style="background:#2dd4bf"></i>JavaScript, in the visitor's browser</span></div></div></section>"#);

    // Features
    let features = [
        ("", "Readable before JavaScript", "Personalised pages arrive as finished HTML. A visitor on a slow phone reads their account before a single script is parsed.", "/docs/how-it-works/"),
        (" b", "Zero JavaScript by default", "A page with no state and no handlers compiles to HTML and CSS only. Interactivity costs bytes only where you use it.", "/docs/blocks-and-updates/"),
        ("", "One resolver, two paths", "Write a data source once in Rust. Server rendering calls it directly; client loading gets a generated endpoint that runs the same code with the same session.", "/docs/data-sources/"),
        ("", "Secure by default", "Opaque session cookies, rotation at sign-in, CSRF tokens injected into every POST form, origin checks, escaping everywhere, private caching.", "/docs/security/"),
        (" b", "A boundary you can trust", "Template expressions mean the same thing in Rust and JavaScript. Anything that can't is a build error that tells you what to do instead.", "/docs/expressions/"),
        (" b", "Measured, not claimed", "Every server-rendered page carries Server-Timing that separates upstream latency from render time, and builds report JavaScript bytes per route.", "/docs/performance/"),
    ];
    s.push_str(r#"<section class="band reveal" id="features"><div class="band-head"><p class="eyebrow">Why Rust-kit</p><h2>Small surface. Strong defaults.</h2></div><div class="features">"#);
    for (i, (class, title, body, href)) in features.iter().enumerate() {
        s.push_str(&format!(
            r#"<div class="feature{class}"><div class="ico">{}</div><h3>{title}</h3><p>{body}</p><a href="{href}">Learn more →</a></div>"#,
            ICONS[i]
        ));
    }
    s.push_str("</div></section>");

    // Diagnostics
    s.push_str(&format!(
        r#"<section class="band reveal" id="diagnostics"><div class="split"><div><p class="eyebrow">Compiler-checked</p><h2>The build tells you when Rust and the browser would disagree.</h2><p>An imported JavaScript formatter can't produce exactly the same text in Rust. Instead of shipping a page that flickers, the compiler stops and points you at the fix.</p><ul><li>Errors point at the exact line and column</li><li>Every error comes with a suggested fix</li><li>Every error is listed in the <a href="/docs/diagnostics/">diagnostics reference</a></li></ul></div>
<div class="terminal"><div class="bar"><i></i><i></i><i></i></div><pre>{}</pre></div></div></section>"#,
        terminal(&diag)
    ));

    // Tutorials
    s.push_str(r#"<section class="band reveal" id="learn"><div class="band-head"><p class="eyebrow">Learn by building</p><h2>From an empty folder to a deployed app.</h2><p>Every tutorial is a complete exercise, and every page sample in them is compiled when this site builds.</p></div><div class="cards tutorials">"#);
    for (i, p) in ctx.tutorials.iter().take(6).enumerate() {
        let meta = match (&p.level, &p.duration) {
            (Some(l), Some(d)) => format!("<span class=\"card-meta\">{} · {}</span>", esc(l), esc(d)),
            _ => String::new(),
        };
        s.push_str(&format!(
            "<a class=\"card\" href=\"{}\"><span class=\"num\">{:02}</span><h3>{}</h3><p>{}</p>{meta}</a>",
            p.url(),
            i + 1,
            esc(&p.title),
            esc(&p.description)
        ));
    }
    s.push_str("</div></section>");

    s.push_str(r#"<section class="cta-band reveal"><div class="inner"><div><h2>Ship pages people can read immediately.</h2><p>Start with the guide, or read the design proposal it all came from.</p></div><div class="cta"><a class="btn primary" href="/docs/getting-started/">Get started →</a><a class="btn" href="/docs/design/">Read the proposal</a></div></div></section>"#);
    s.push_str("</main>");
    s.push_str(&footer());
    s.push_str("</body>\n</html>\n");
    s
}

/// Colours a rendered diagnostic like a terminal.
fn terminal(diag: &str) -> String {
    let mut out = String::new();
    for line in diag.lines() {
        let e = esc(line);
        let styled = if let Some(rest) = e.strip_prefix("error:") {
            format!("<span class=\"er\">error:</span><b>{rest}</b>")
        } else if e.trim_start().starts_with("--&gt;") {
            format!("<span class=\"ar\">{e}</span>")
        } else if e.trim_start().starts_with("= help:") {
            let (a, b) = e.split_once("help:").unwrap();
            format!("<span class=\"ar\">{a}</span><span class=\"hp\">help:{b}</span>")
        } else if e.trim_end().ends_with('^') {
            format!("<span class=\"hl\">{e}</span>")
        } else {
            e
        };
        out.push_str(&styled);
        out.push('\n');
    }
    out
}

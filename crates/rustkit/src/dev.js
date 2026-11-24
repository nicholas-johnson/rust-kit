// Rust-kit dev client. Served only under `rustkit dev`.
//
// Reloads the page when the server comes back from a rebuild, keeping the
// scroll position and the page's $state. Shows build failures over the page.
(() => {
  const build = document.currentScript?.dataset.build;
  const KEY = "rk:dev:" + location.pathname + location.search;
  let saved = null;
  try {
    saved = JSON.parse(sessionStorage.getItem(KEY) || "null");
  } catch {}
  sessionStorage.removeItem(KEY);

  let pageState = null;
  globalThis.__rkDev = {
    // Called by the runtime once the page has started. Assigning through the
    // proxy re-renders the blocks that read the restored values.
    adopt(s) {
      pageState = s;
      for (const [k, v] of Object.entries(saved?.state ?? {})) {
        if (k in s && typeof v === typeof s[k]) s[k] = v;
      }
    },
  };
  if (saved) {
    const restore = () => scrollTo(saved.x, saved.y);
    addEventListener("DOMContentLoaded", () => requestAnimationFrame(restore));
  }

  function reload() {
    const state = {};
    for (const [k, v] of Object.entries(pageState ?? {})) {
      if (v === null || typeof v !== "object") state[k] = v;
    }
    sessionStorage.setItem(KEY, JSON.stringify({ x: scrollX, y: scrollY, state }));
    location.reload();
  }

  const events = new EventSource("/_rk/dev/events");
  events.addEventListener("build", (e) => {
    if (e.data !== build) reload();
  });
  events.addEventListener("status", (e) => show(JSON.parse(e.data)));

  let host = null;
  function show(msg) {
    host?.remove();
    host = null;
    if (msg.status === "idle") return;
    host = document.createElement("rk-dev");
    const root = host.attachShadow({ mode: "open" });
    if (msg.status === "building") {
      root.innerHTML = `<style>div{position:fixed;right:12px;bottom:12px;z-index:2147483647;font:12px/1.4 system-ui,sans-serif;background:#222;color:#fff;padding:6px 10px;border-radius:6px;opacity:.85}</style><div role="status">Rebuilding…</div>`;
    } else if (msg.status === "failed") {
      root.innerHTML = `<style>
        .bg{position:fixed;inset:0;z-index:2147483647;background:rgba(0,0,0,.6);overflow:auto;padding:4vh 4vw}
        .box{background:#1b1b1f;color:#eee;border-radius:8px;padding:16px 20px;max-width:960px;margin:auto;font:13px/1.5 ui-monospace,Menlo,monospace}
        h2{font:600 15px system-ui,sans-serif;margin:0 0 12px;color:#ff8080}
        pre{white-space:pre-wrap;margin:0}
        p{font:12px system-ui,sans-serif;color:#aaa;margin:12px 0 0}
        button{float:right;font:inherit;background:none;border:1px solid #555;color:#ccc;border-radius:4px;cursor:pointer}
      </style><div class="bg"><div class="box" role="alertdialog" aria-label="Build failed"><button>Close</button><h2>Build failed</h2><pre></pre><p>Still serving the last successful build. Fix the error and save; this page reloads when the build succeeds.</p></div></div>`;
      root.querySelector("pre").textContent = msg.output || "";
      root.querySelector("button").onclick = () => {
        host.remove();
        host = null;
      };
    }
    document.documentElement.append(host);
  }
})();

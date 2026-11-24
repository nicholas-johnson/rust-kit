// Rust-kit dev client. Served only under `rustkit dev`.
//
// When the server comes back from a rebuild, this updates the open page in
// place instead of reloading it:
//
// - If only stylesheets changed, it swaps the <link> elements. Nothing else
//   on the page is touched.
// - Otherwise it fetches the page again, stops the running page module,
//   updates the body node by node (so unchanged elements keep focus and
//   typed text), and starts the new module with the old $state and the data
//   the page had loaded in the browser. That data shows until fresh data
//   arrives.
//
// If an update can't be done in place, it reloads, keeping the scroll
// position and $state. Build failures show over the page.
(() => {
  let build = document.currentScript?.dataset.build;
  const KEY = "rk:dev:" + location.pathname + location.search;
  let saved = null;
  try {
    saved = JSON.parse(sessionStorage.getItem(KEY) || "null");
  } catch {}
  sessionStorage.removeItem(KEY);

  // The body as the server sent it, serialized the way DOMParser output is,
  // so a fetched copy can be compared with it.
  let served = null;
  document.addEventListener("readystatechange", () => {
    if (document.readyState === "interactive" && served === null) served = document.body.innerHTML;
  });

  let page = null; // { state, sources, stop } from the runtime
  let carry = null; // what the next page module starts with
  const primitives = (o) =>
    Object.fromEntries(Object.entries(o ?? {}).filter(([, v]) => v === null || typeof v !== "object"));

  globalThis.__rkDev = {
    carried() {
      return carry?.sources ?? [];
    },
    attach(p) {
      page = p;
      // Assigning through the proxy re-renders the blocks that read the values.
      const restore = carry?.state ?? saved?.state ?? {};
      carry = null;
      saved = null;
      for (const [k, v] of Object.entries(restore)) {
        if (k in p.state && typeof v === typeof p.state[k]) p.state[k] = v;
      }
    },
  };
  if (saved) {
    const { x, y } = saved;
    addEventListener("DOMContentLoaded", () => requestAnimationFrame(() => scrollTo(x, y)));
  }

  function reload() {
    sessionStorage.setItem(KEY, JSON.stringify({ x: scrollX, y: scrollY, state: primitives(page?.state) }));
    location.reload();
  }

  async function swapStylesheets(next) {
    const hrefs = (d) => [...d.head.querySelectorAll('link[rel="stylesheet"]')].map((l) => l.getAttribute("href"));
    const want = hrefs(next);
    const have = [...document.head.querySelectorAll('link[rel="stylesheet"]')];
    const haveHrefs = have.map((l) => l.getAttribute("href"));
    if (want.join("\n") === haveHrefs.join("\n")) return false;
    // Load the new sheets before dropping the old ones, so nothing flashes unstyled.
    await Promise.all(
      want
        .filter((h) => !haveHrefs.includes(h))
        .map((href) => {
          const link = Object.assign(document.createElement("link"), { rel: "stylesheet", href });
          document.head.append(link);
          return new Promise((done) => (link.onload = link.onerror = done));
        }),
    );
    for (const l of have) if (!want.includes(l.getAttribute("href"))) l.remove();
    return true;
  }

  // Makes `from`'s children match `to`'s, reusing nodes position by position
  // where the tag and id are the same. Scripts from DOMParser never execute when
  // inserted, so the new page module is started by `import()` below.
  function morphChildren(from, to) {
    const a = [...from.childNodes];
    const b = [...to.childNodes];
    const same = (x, y) => x.nodeType === y.nodeType && x.nodeName === y.nodeName && (x.id ?? "") === (y.id ?? "");
    let i = 0;
    for (const y of b) {
      // An element with an id that moved up: drop the old nodes before it.
      if (y.id) {
        const j = a.findIndex((x, k) => k > i && same(x, y));
        if (j > i) {
          for (const x of a.slice(i, j)) x.remove();
          i = j;
        }
      }
      const x = a[i];
      if (x && same(x, y)) {
        morph(x, y);
        i++;
      } else {
        from.insertBefore(document.importNode(y, true), x ?? null);
      }
    }
    for (const x of a.slice(i)) x.remove();
  }

  function morph(from, to) {
    if (from.nodeType !== Node.ELEMENT_NODE) {
      if (from.nodeValue !== to.nodeValue) from.nodeValue = to.nodeValue;
      return;
    }
    for (const { name } of [...from.attributes]) if (!to.hasAttribute(name)) from.removeAttribute(name);
    for (const { name, value } of to.attributes) if (from.getAttribute(name) !== value) from.setAttribute(name, value);
    if (from.nodeName === "SCRIPT") {
      if (from.textContent !== to.textContent) from.textContent = to.textContent;
      return;
    }
    morphChildren(from, to);
  }

  async function update(id) {
    const res = await fetch(location.href, { cache: "no-store", credentials: "same-origin" });
    const type = res.headers.get("content-type") ?? "";
    if (!res.ok || res.redirected || !type.startsWith("text/html")) throw new Error("not a page");
    const next = new DOMParser().parseFromString(await res.text(), "text/html");

    const styles = await swapStylesheets(next);
    document.title = next.title;
    const body = next.body.innerHTML;
    if (body === served) return styles ? "stylesheets" : "nothing";

    carry = page ? { state: primitives(page.state), sources: [...page.sources].map(([n, s]) => [n, s.data]).filter(([, d]) => d !== undefined) } : null;
    page?.stop();
    page = null;
    morphChildren(document.body, next.body);
    served = body;
    const src = next.querySelector('script[type="module"][src]')?.getAttribute("src");
    // A fresh URL runs the module again even if its contents didn't change.
    if (src) await import(src + "?hot=" + encodeURIComponent(id));
    carry = null;
    return "page";
  }

  let running = false;
  let latest = build;
  async function onBuild(id) {
    latest = id;
    if (running || id === build) return;
    running = true;
    try {
      while (latest !== build) {
        const id = latest;
        const what = await update(id);
        build = id;
        if (what !== "nothing") console.info(`[rustkit] build ${id}: updated ${what} in place`);
      }
      show({ status: "idle" });
    } catch (e) {
      console.info("[rustkit] reloading:", e.message);
      reload();
    } finally {
      running = false;
    }
  }

  // The browser reconnects every 200 ms, which covers a restart. If the
  // server stays away (`rustkit dev` was stopped), back off to every 2 s.
  function connect() {
    let failures = 0;
    const events = new EventSource("/_rk/dev/events");
    events.addEventListener("build", (e) => {
      failures = 0;
      onBuild(e.data);
    });
    events.addEventListener("status", (e) => show(JSON.parse(e.data)));
    events.onerror = () => {
      if (++failures < 25) return;
      events.close();
      setTimeout(connect, 2000);
    };
  }
  connect();

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
      </style><div class="bg"><div class="box" role="alertdialog" aria-label="Build failed"><button>Close</button><h2>Build failed</h2><pre></pre><p>Still serving the last successful build. Fix the error and save; this page updates when the build succeeds.</p></div></div>`;
      root.querySelector("pre").textContent = msg.output || "";
      root.querySelector("button").onclick = () => {
        host.remove();
        host = null;
      };
    }
    document.documentElement.append(host);
  }
})();

// Rust-kit browser runtime. Shared by every interactive route.
//
// The server renders the whole page. This module adopts the blocks the
// compiler marked with <!--rk:key--> comments, delegates events to the
// handlers named in data-rk-on, and re-renders a block when state it reads
// changes or when the client-loaded source it shows arrives.

const ESC = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const esc = (v) => (v == null ? "" : String(v)).replace(/[&<>"']/g, (c) => ESC[c]);
const attr = (name, v) => (v === false || v == null ? "" : v === true ? " " + name : ` ${name}="${esc(v)}"`);
const LOADING = '<p class="rk-loading" aria-busy="true">Loading…</p>';

let notify = () => {};

/** Reactive page state. Assigning a property schedules an update. */
export function state(init) {
  return new Proxy(init, {
    set(target, key, value) {
      if (!Object.is(target[key], value)) {
        target[key] = value;
        notify(key);
      }
      return true;
    },
  });
}

export function start($s, page) {
  const payload = document.getElementById("rk-payload");
  const P = payload ? JSON.parse(payload.textContent) : { s: {} };
  const instances = new Map(); // id -> { id, key, scope, start, end, kids, handlers }
  const handlers = new Map(); // handler id -> function
  const sources = new Map(); // source name -> { status, data }
  const changed = new Set();
  let seq = 0;
  let owner = null;
  let queued = false;

  const r = {
    esc,
    attr,
    csrf: () => P.csrf ?? "",
    on(type, key, c) {
      const id = "#" + seq++;
      handlers.set(id, page.handlers[key](c));
      owner?.handlers.add(id);
      return `${type}:${id}`;
    },
    block(key, c) {
      const inst = { id: "i" + seq++, key, scope: c, kids: new Set(), handlers: new Set() };
      instances.set(inst.id, inst);
      owner?.kids.add(inst.id);
      return `<!--rk:${inst.id}-->${render(inst)}<!--/rk:${inst.id}-->`;
    },
  };

  function render(inst) {
    const def = page.blocks[inst.key];
    const prev = owner;
    owner = inst;
    try {
      if (!def.source) return def.render(inst.scope, r);
      const st = load(def.source, false);
      let h = "";
      if (st.data !== undefined) h += def.render({ ...inst.scope, [def.bind]: st.data }, r);
      else if (st.status === "loading") h += LOADING;
      if (st.status === "error") {
        h += `<p class="rk-error" role="alert">Couldn't load this. <button type="button" data-rk-retry="${esc(def.source)}">Retry</button></p>`;
      }
      return h;
    } finally {
      owner = prev;
    }
  }

  function load(name, force) {
    let st = sources.get(name);
    if (st && !force) return st;
    st = { status: "loading", data: st?.data };
    sources.set(name, st);
    fetch("/_rk/source/" + encodeURIComponent(name), {
      credentials: "same-origin",
      headers: { accept: "application/json" },
    })
      .then((res) => (res.ok ? res.json() : Promise.reject(new Error("HTTP " + res.status))))
      .then(
        (data) => {
          st.status = "ready";
          st.data = data;
        },
        () => {
          st.status = "error";
        },
      )
      .then(() => refresh((i) => page.blocks[i.key].source === name));
    return st;
  }

  function dispose(inst) {
    for (const id of inst.kids) {
      const kid = instances.get(id);
      if (kid) dispose(kid);
      instances.delete(id);
    }
    inst.kids.clear();
    for (const h of inst.handlers) handlers.delete(h);
    inst.handlers.clear();
  }

  // Finds block markers under `root`. Runtime blocks already exist; server
  // rendered ones are adopted with the scope the compiler described.
  function bind(root) {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_COMMENT);
    const stack = [];
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const m = /^(\/?)rk:(\w+)$/.exec(n.data);
      if (!m) continue;
      if (m[1]) {
        const inst = stack.pop();
        if (inst) inst.end = n;
        continue;
      }
      let inst = instances.get(m[2]);
      if (!inst) {
        inst = { id: "s" + seq++, key: m[2], scope: page.ssr[m[2]]?.(P) ?? {}, kids: new Set(), handlers: new Set() };
        instances.set(inst.id, inst);
        stack.at(-1)?.kids.add(inst.id);
      }
      inst.start = n;
      stack.push(inst);
    }
  }

  function rerender(inst) {
    dispose(inst);
    const tpl = document.createElement("template");
    tpl.innerHTML = render(inst);
    bind(tpl.content);
    for (let n = inst.start.nextSibling; n && n !== inst.end; ) {
      const next = n.nextSibling;
      n.remove();
      n = next;
    }
    inst.end.parentNode.insertBefore(tpl.content, inst.end);
  }

  function refresh(match) {
    const targets = [...instances.values()].filter((i) => i.start?.isConnected && match(i));
    targets.sort((a, b) => (a.start.compareDocumentPosition(b.start) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
    // An outer block re-renders its children, which drops them from `instances`.
    for (const t of targets) if (instances.has(t.id)) rerender(t);
  }

  notify = (key) => {
    changed.add(key);
    if (queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      const keys = new Set(changed);
      changed.clear();
      refresh((i) => page.blocks[i.key].deps.some((d) => keys.has(d)));
    });
  };

  function handler(id) {
    if (handlers.has(id)) return handlers.get(id);
    // A server-rendered handler: build it once from the payload scope.
    const fn = page.handlers[id]?.(page.ssr[id]?.(P) ?? {});
    handlers.set(id, fn);
    return fn;
  }

  for (const type of page.events) {
    document.addEventListener(
      type,
      (ev) => {
        for (let el = ev.target; el && el !== document; el = el.parentNode) {
          const spec = el.getAttribute?.("data-rk-on");
          if (!spec) continue;
          for (const part of spec.split(" ")) {
            const [t, id] = part.split(":");
            if (t !== type) continue;
            const fn = handler(id);
            if (typeof fn === "function") fn.call(el, ev);
          }
        }
      },
      true,
    );
  }

  document.addEventListener("click", (ev) => {
    const name = ev.target.closest?.("[data-rk-retry]")?.getAttribute("data-rk-retry");
    if (name) load(name, true) && refresh((i) => page.blocks[i.key].source === name);
  });

  bind(document.body);
  // Server-rendered client sources show a loading region; fetch them now.
  for (const inst of instances.values()) {
    const src = page.blocks[inst.key].source;
    if (src) load(src, false);
  }
  // Under `rustkit dev`, restores state saved before a reload.
  globalThis.__rkDev?.adopt($s);
}

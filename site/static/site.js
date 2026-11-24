// rust-kit.com: theme, search, copy buttons, contents highlighting, reveals.
// Everything here is progressive: the site reads fine without it.
(() => {
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => [...r.querySelectorAll(s)];
  const root = document.documentElement;

  // Theme
  $$("[data-theme-toggle]").forEach((b) =>
    b.addEventListener("click", () => {
      const next = root.dataset.theme === "dark" ? "light" : "dark";
      root.dataset.theme = next;
      localStorage.setItem("rk-theme", next);
    }),
  );

  // Copy buttons
  $$("figure.code .copy").forEach((btn) =>
    btn.addEventListener("click", async () => {
      const code = btn.closest("figure").querySelector("code").innerText;
      try {
        await navigator.clipboard.writeText(code);
        btn.textContent = "Copied";
        btn.classList.add("done");
        setTimeout(() => {
          btn.textContent = "Copy";
          btn.classList.remove("done");
        }, 1600);
      } catch {
        btn.textContent = "Select & copy";
      }
    }),
  );

  // Close the mobile sidebar after navigating within the page.
  const navOpen = $("#nav-open");
  $$(".sidebar a").forEach((a) => a.addEventListener("click", () => navOpen && (navOpen.checked = false)));

  // Contents highlighting
  const tocLinks = $$(".toc a");
  if (tocLinks.length && "IntersectionObserver" in window) {
    const byId = new Map(tocLinks.map((a) => [decodeURIComponent(a.hash.slice(1)), a]));
    const visible = new Set();
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) e.isIntersecting ? visible.add(e.target.id) : visible.delete(e.target.id);
        const heads = $$(".prose h2[id], .prose h3[id]");
        const current = heads.find((h) => visible.has(h.id)) ||
          [...heads].reverse().find((h) => h.getBoundingClientRect().top < 120);
        tocLinks.forEach((a) => a.classList.remove("active"));
        if (current) byId.get(current.id)?.classList.add("active");
      },
      { rootMargin: "-64px 0px -65% 0px" },
    );
    $$(".prose h2[id], .prose h3[id]").forEach((h) => io.observe(h));
  }

  // Scroll reveals on the landing page
  const reveals = $$(".reveal");
  if (reveals.length && "IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => entries.forEach((e) => e.isIntersecting && (e.target.classList.add("in"), io.unobserve(e.target))),
      { threshold: 0.12 },
    );
    reveals.forEach((el) => io.observe(el));
  } else {
    reveals.forEach((el) => el.classList.add("in"));
  }

  // Search
  const dialog = $("#search");
  const input = $("[data-search-input]");
  const list = $("[data-search-results]");
  const empty = $("[data-search-empty]");
  let index = null;
  let sel = 0;

  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const mark = (s, terms) => {
    let out = esc(s);
    for (const t of terms) out = out.replace(new RegExp(`(${t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "ig"), "<mark>$1</mark>");
    return out;
  };

  async function open() {
    if (!dialog) return;
    dialog.showModal();
    input.value = "";
    input.focus();
    render([]);
    if (!index) {
      try {
        index = await (await fetch("/search.json")).json();
      } catch {
        index = [];
      }
      // The visitor may have typed while the index was loading.
      if (input.value) input.dispatchEvent(new Event("input"));
    }
  }

  function search(q) {
    const terms = q.toLowerCase().split(/\s+/).filter((t) => t.length > 1);
    if (!terms.length || !index) return [[], terms];
    const scored = [];
    for (const e of index) {
      const t = e.t.toLowerCase(), h = e.h.toLowerCase(), x = e.x.toLowerCase();
      let score = 0;
      for (const term of terms) {
        const s = (t.includes(term) ? 8 : 0) + (h.includes(term) ? 5 : 0) + (x.includes(term) ? 1 : 0);
        if (!s) {
          score = 0;
          break;
        }
        score += s + (t.startsWith(term) || h.startsWith(term) ? 3 : 0);
      }
      if (score) scored.push([score, e]);
    }
    scored.sort((a, b) => b[0] - a[0]);
    return [scored.slice(0, 12).map((s) => s[1]), terms];
  }

  function render(results, terms = []) {
    sel = 0;
    list.innerHTML = results
      .map((e, i) => {
        const x = e.x.length > 140 ? e.x.slice(0, 140) + "…" : e.x;
        return `<li><a href="${esc(e.u)}"${i === 0 ? ' class="sel"' : ""}><span class="r-top"><span class="r-kind">${esc(e.k)}</span>${mark(e.t, terms)}${
          e.h && e.h !== e.t ? `<span class="r-h">› ${mark(e.h, terms)}</span>` : ""
        }</span><span class="r-x">${mark(x, terms)}</span></a></li>`;
      })
      .join("");
    empty.hidden = !(input.value.trim().length > 1 && results.length === 0);
  }

  input?.addEventListener("input", () => {
    const [r, t] = search(input.value);
    render(r, t);
  });
  input?.addEventListener("keydown", (ev) => {
    const items = $$("a", list);
    if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
      ev.preventDefault();
      items[sel]?.classList.remove("sel");
      sel = (sel + (ev.key === "ArrowDown" ? 1 : -1) + items.length) % Math.max(items.length, 1);
      items[sel]?.classList.add("sel");
      items[sel]?.scrollIntoView({ block: "nearest" });
    } else if (ev.key === "Enter" && items[sel]) {
      ev.preventDefault();
      location.href = items[sel].href;
      dialog.close();
    }
  });
  list?.addEventListener("click", (ev) => ev.target.closest("a") && dialog.close());
  dialog?.addEventListener("click", (ev) => ev.target === dialog && dialog.close());
  $$("[data-search-open]").forEach((b) => b.addEventListener("click", open));
  document.addEventListener("keydown", (ev) => {
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName);
    if ((ev.key === "k" && (ev.metaKey || ev.ctrlKey)) || (ev.key === "/" && !typing)) {
      ev.preventDefault();
      dialog?.open ? dialog.close() : open();
    }
  });
})();

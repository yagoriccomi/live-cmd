// F4: temas (as duas janelas), paleta de comandos, favoritos e "Nova sessão aqui" (só no terminal).
// Usa os globais de terminal.js (invoke, tabs, active, conns, openSession, refresh, banner, msg, el).
(() => {
  const THEMES = { dark: "Aurora", ametista: "Ametista", magenta: "Magenta", cosmo: "Cosmo" };
  const getTheme = () => (THEMES[localStorage.theme] ? localStorage.theme : "dark");
  const setTheme = (t) => { localStorage.theme = t; applyTheme(); };
  function applyTheme() {
    document.documentElement.dataset.theme = getTheme();
    const sel = document.getElementById("theme");
    if (sel) sel.value = getTheme();
  }
  applyTheme();
  addEventListener("storage", (e) => { if (e.key === "theme") applyTheme(); }); // outra janela mudou
  const sel = document.getElementById("theme");
  if (sel) sel.onchange = () => setTheme(sel.value);

  const screen = document.getElementById("screen");
  if (!screen) return; // janela de configurações: só tema

  // ---------- Pasta atual por sessão (OSC 7, via evento term-cwd) ----------
  const cwds = {};
  window.__TAURI__.event.listen("term-cwd", (e) => { if (e.payload.cwd) cwds[e.payload.key] = e.payload.cwd; else delete cwds[e.payload.key]; });

  async function newHere() {
    const t = tabs.find((t) => t.key === active);
    const cwd = t && cwds[t.key];
    if (!cwd) return banner("A pasta atual desta sessão ainda não é conhecida.", "warn");
    const p = conns[t.conn]?.profiles?.[0];
    if (!p) return banner(msg("NOT_FOUND"), "err");
    const { cols, rows } = size();
    try {
      const sid = await invoke("term_create", { conn: t.conn, profileId: p.profile_id, label: p.label, cols, rows, cwd });
      await openSession(t.conn, { session_id: sid, label: p.label, state: "running" });
      refresh(t.conn);
    } catch (e) { banner(msg(e), "err"); }
  }

  // ---------- Favoritos ----------
  const favs = () => { try { const v = JSON.parse(localStorage.favs || "[]"); return Array.isArray(v) ? v.filter((s) => typeof s === "string") : []; } catch { return []; } };
  const saveFavs = (v) => { localStorage.favs = JSON.stringify(v); };
  function runFav(cmd) {
    const t = tabs.find((t) => t.key === active);
    if (!t || t.exited != null) return banner("Abra uma sessão para executar o comando.", "warn");
    invoke("term_input", { conn: t.conn, sessionId: t.sid, data: [...new TextEncoder().encode(cmd + "\r")] }).catch((e) => banner(msg(e), "err"));
  }

  // ---------- Paleta ----------
  const act = (detail) => () => document.dispatchEvent(new CustomEvent("term-action", { detail }));
  function items(q) {
    const list = [
      { label: "Nova sessão", run: () => newSession(tabs.find((t) => t.key === active)?.conn || LOCAL) },
      { label: "Nova sessão aqui (pasta atual)", run: newHere },
      { label: "Renomear sessão", run: act("rename") },
      ...Object.entries(THEMES).map(([k, v]) => ({ label: `Tema: ${v}`, run: () => setTheme(k) })),
      ...favs().map((c) => ({ label: c, fav: true, run: () => runFav(c) })),
    ];
    const ql = q.trim().toLowerCase();
    // Busca aproximada: as letras da consulta aparecem em ordem.
    const fuzzy = (s) => { let i = 0; for (const ch of s.toLowerCase()) if (ch === ql[i]) i++; return i === ql.length; };
    const out = list.filter((it) => fuzzy(it.label));
    if (ql && !favs().includes(q.trim())) out.push({ label: `Salvar "${q.trim()}" como favorito`, run: () => saveFavs([...favs(), q.trim()].slice(-100)), keep: true });
    return out;
  }

  const scrim = el("div", "dialog-scrim"); scrim.hidden = true;
  scrim.innerHTML = `<dialog open aria-modal="true" aria-labelledby="pal-t" class="palette">
    <h3 id="pal-t" class="sr-only">Paleta de comandos</h3>
    <input type="text" id="pal-q" role="combobox" aria-expanded="true" aria-controls="pal-list" aria-autocomplete="list" aria-describedby="pal-h" placeholder="Digite um comando ou um texto para salvar" autocomplete="off" spellcheck="false">
    <ul id="pal-list" role="listbox" aria-label="Comandos"></ul>
    <p class="muted small" id="pal-h">Enter executa · Delete remove o favorito selecionado · Esc fecha</p></dialog>`;
  document.body.append(scrim);
  const q = scrim.querySelector("#pal-q"), ul = scrim.querySelector("#pal-list");
  let cur = [], idx = 0, opener = null;

  function draw() {
    cur = items(q.value);
    idx = Math.min(idx, Math.max(0, cur.length - 1));
    ul.replaceChildren(...cur.map((it, i) => {
      const li = el("li", it.fav ? "mono" : null, it.fav ? "★ " + it.label : it.label);
      li.id = "pal-" + i; li.setAttribute("role", "option"); li.setAttribute("aria-selected", String(i === idx));
      li.onmousedown = (e) => { e.preventDefault(); idx = i; run(); };
      return li;
    }));
    if (cur.length) { q.setAttribute("aria-activedescendant", "pal-" + idx); ul.children[idx].scrollIntoView({ block: "nearest" }); }
    else q.removeAttribute("aria-activedescendant");
  }
  function open() { opener = document.activeElement; scrim.hidden = false; q.value = ""; idx = 0; draw(); q.focus(); }
  function close() { scrim.hidden = true; (opener || screen).focus(); }
  function run() {
    const it = cur[idx]; if (!it) return;
    if (it.keep) { it.run(); q.value = ""; idx = 0; return draw(); }
    close(); it.run();
  }
  q.oninput = () => { idx = 0; draw(); };
  q.onkeydown = (e) => {
    if (e.key === "ArrowDown") idx = Math.min(idx + 1, cur.length - 1);
    else if (e.key === "ArrowUp") idx = Math.max(idx - 1, 0);
    else if (e.key === "Enter") return run();
    else if (e.key === "Escape") return close();
    else if (e.key === "Delete" && cur[idx]?.fav) saveFavs(favs().filter((c) => c !== cur[idx].label));
    else return;
    e.preventDefault(); draw();
  };
  scrim.onmousedown = (e) => { if (e.target === scrim) close(); };

  // Captura: vem antes do teclado do terminal.
  addEventListener("keydown", (e) => {
    if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyP") { e.preventDefault(); e.stopPropagation(); scrim.hidden ? open() : close(); }
  }, true);
  document.addEventListener("term-action", (e) => {
    if (e.detail === "palette") open();
    if (e.detail === "theme") { open(); q.value = "Tema"; draw(); }
    if (e.detail === "new-here") newHere();
  });
})();

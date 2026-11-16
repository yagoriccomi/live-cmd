// Terminal Desktop (design §6). A tela vem pronta do Rust (vt100); aqui só desenho, teclado e lista.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);
const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

const LOCAL = "local";
// C2 §2.2 e design §6; nunca mostrar link, OTK nem caminhos.
const MSG = {
  EXPIRED: "Tempo esgotado. Gere um novo link no outro PC e tente de novo.",
  UNAUTHORIZED: "Link inválido ou já usado. Gere um novo no outro PC.",
  REVOKED: "Este PC foi removido pelo outro computador. Pareie novamente.",
  VERSION_UNSUPPORTED: "Versão incompatível. Atualize o live-cmd nos dois computadores.",
  SERVER_NOT_ALLOWED: "Servidor não reconhecido. Atualize o app ou use um link válido.",
  INVALID_MESSAGE: "Mensagem inválida. Tente conectar de novo.",
  LIMIT_EXCEEDED: "Limite atingido. Aguarde e tente novamente.",
  NOT_FOUND: "Não encontrado.",
  NOT_CONNECTED: "Sem conexão com este PC.",
  IO_ERROR: "Não foi possível gravar os dados do app.",
  CLIPBOARD: "Não foi possível usar a área de transferência.",
};
const msg = (code) => MSG[code] || "Algo deu errado. Tente de novo.";

const conns = { [LOCAL]: { name: "Este PC", status: { state: "ready" }, sessions: [], profiles: [] } };
let remoteOrder = [];
const tabs = [];       // { key, conn, sid, title, exited }
let active = null;     // key
let appCursor = false;
const screens = {};    // key → última tela recebida
let adding = null;     // id do PC sendo adicionado

// ---------- Medida da célula e tamanho ----------
function size() {
  const m = $("measure").getBoundingClientRect(), s = $("screen"), cs = getComputedStyle(s);
  const w = s.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const h = s.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const lh = parseFloat(cs.lineHeight) || m.height;
  return { cols: Math.max(20, Math.min(1000, Math.floor(w / (m.width / 10)))), rows: Math.max(5, Math.min(1000, Math.floor(h / lh))) };
}

// ---------- Desenho ----------
function draw(key) {
  const s = $("screen"), scr = screens[key];
  for (const n of [...s.children]) if (n.id !== "measure") n.remove();
  if (!scr) return;
  appCursor = scr.app_cursor;
  for (const row of scr.rows) {
    const line = el("div");
    for (const [text, fg, bg, fl] of row) {
      const sp = el("span", null, text);
      let [f, b] = fl & 8 ? [bg || "var(--term-bg)", fg || "var(--term-fg)"] : [fg, bg];
      if (f) sp.style.color = f;
      if (b) sp.style.background = b;
      if (fl & 1) sp.classList.add("b");
      if (fl & 2) sp.classList.add("i");
      if (fl & 4) sp.classList.add("u");
      if (fl & 16) sp.classList.add("cur");
      line.append(sp);
    }
    s.append(line);
  }
  afterDraw();
}

// ---------- Banner ----------
function banner(text, kind, actions = []) {
  $("banner").hidden = !text;
  $("banner").className = "status-banner" + (kind ? " " + kind : "");
  $("banner-t").textContent = text || "";
  $("banner-a").replaceChildren(...actions.map(([label, fn]) => { const b = el("button", null, label); b.type = "button"; b.onclick = fn; return b; }));
}

function updateBanner() {
  const t = tabs.find((t) => t.key === active);
  if (!t) return banner("");
  if (t.exited != null) return banner(`A sessão terminou (código ${t.exited}).`, "warn", [["Nova sessão", () => newSession(t.conn)], ["Fechar aba", () => closeTab(t.key)]]);
  const st = conns[t.conn]?.status || { state: "off" };
  if (st.state === "connecting") {
    if (!st.attempt) return banner("Conectando…");
    return banner(`Conexão perdida. Reconectando… (tentativa ${st.attempt + 1})`, "warn", [["Cancelar", () => invoke("remote_disconnect", { id: t.conn })]]);
  }
  if (st.state === "closed") return banner(st.code ? msg(st.code) : "Conexão encerrada.", "err", st.code === "REVOKED" ? [] : [["Reconectar", () => connectRemote(t.conn)]]);
  if (st.state === "off") return banner("Desconectado.", "warn", [["Conectar", () => connectRemote(t.conn)]]);
  banner("");
}

// ---------- Abas ----------
function renderTabs() {
  const bar = $("tabs");
  bar.replaceChildren(...tabs.map((t) => {
    const b = el("button");
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(t.key === active));
    b.tabIndex = t.key === active ? 0 : -1;
    b.append(el("span", null, t.title));
    const x = el("span", "x", "×");
    x.setAttribute("aria-hidden", "true");
    x.title = "Fechar aba (Ctrl+Shift+W)";
    x.onclick = (e) => { e.stopPropagation(); closeTab(t.key); };
    b.append(x);
    b.onclick = () => activate(t.key);
    b.onauxclick = (e) => { if (e.button === 1) closeTab(t.key); };
    return b;
  }));
  $("empty").hidden = tabs.length > 0;
  const t = tabs.find((t) => t.key === active);
  $("screen").setAttribute("aria-label", t ? "Terminal " + t.title : "Terminal");
  renderSidebar();
}

$("tabs").onkeydown = (e) => {
  if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
  const i = tabs.findIndex((t) => t.key === active);
  if (i < 0) return;
  activate(tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length].key, false);
  $("tabs").querySelector("[aria-selected=true]")?.focus();
};

function activate(key, focus = true) {
  active = key;
  renderTabs();
  draw(key);
  updateBanner();
  const t = tabs.find((t) => t.key === key);
  if (t && t.exited == null) { const { cols, rows } = size(); invoke("term_resize", { conn: t.conn, sessionId: t.sid, cols, rows }).catch(() => {}); }
  if (focus) $("screen").focus();
}

function titleOf(conn, s) {
  const label = s.label || "Terminal";
  return conn === LOCAL ? label : `${conns[conn].name} · ${label}`;
}

async function openSession(conn, s) {
  const key = conn + "/" + s.session_id;
  if (!tabs.some((t) => t.key === key)) {
    const { cols, rows } = size();
    let ok;
    try { ok = await invoke("term_attach", { conn, sessionId: s.session_id, cols, rows }); }
    catch (e) { return banner(msg(e), "err"); }
    tabs.push({ key, conn, sid: s.session_id, title: titleOf(conn, s), exited: ok.state === "exited" ? ok.exit_code ?? 0 : null });
  }
  activate(key);
}

async function closeTab(key) {
  const i = tabs.findIndex((t) => t.key === key);
  if (i < 0) return;
  const [t] = tabs.splice(i, 1);
  delete screens[key];
  invoke("term_detach", { conn: t.conn, sessionId: t.sid }).catch(() => {});
  if (active === key) {
    active = tabs[Math.min(i, tabs.length - 1)]?.key ?? null;
    if (active) return activate(active);
    draw(null);
    updateBanner();
  }
  renderTabs();
  if (!active) $("local-new").focus();
}

async function newSession(conn, profile) {
  const c = conns[conn];
  const p = profile || c.profiles[0];
  if (!p) return banner(msg("NOT_FOUND"), "err");
  const { cols, rows } = size();
  try {
    const sid = await invoke("term_create", { conn, profileId: p.profile_id, label: p.label, cols, rows });
    await openSession(conn, { session_id: sid, label: p.label, state: "running" });
    refresh(conn);
  } catch (e) { banner(msg(e), "err"); }
}

// ---------- Lista lateral ----------
async function refresh(conn) {
  try {
    const v = await invoke("term_list", { conn });
    conns[conn].sessions = v.sessions;
    conns[conn].profiles = v.profiles;
  } catch { conns[conn].sessions = []; }
  renderSidebar();
}

async function refreshRemotes() {
  const v = await invoke("term_connections").catch(() => ({ remotes: [] }));
  remoteOrder = v.remotes.map((r) => r.id);
  for (const r of v.remotes) conns[r.id] = { sessions: [], profiles: [], ...conns[r.id], name: r.name, status: r.status };
  renderSidebar();
}

function sessionItem(conn, s) {
  const li = el("li"), b = el("button");
  b.type = "button";
  const key = conn + "/" + s.session_id;
  if (key === active) b.setAttribute("aria-current", "true");
  b.append(el("span", s.state === "exited" ? "muted" : "dot-ok", s.state === "exited" ? "○" : "●"), el("span", null, s.label || "Terminal"));
  b.onclick = () => openSession(conn, s);
  b.oncontextmenu = (e) => { e.preventDefault(); menu(b, [["Encerrar sessão", () => endSession(conn, s)]]); };
  b.onkeydown = (e) => { if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); b.oncontextmenu(e); } };
  li.append(b);
  return li;
}

function newButton(conn) {
  const b = el("button", "add", "+ Nova sessão ▾");
  b.type = "button";
  b.setAttribute("aria-haspopup", "menu");
  b.onclick = () => profileMenu(b, conn);
  return b;
}

function renderSidebar() {
  $("local-ul").replaceChildren(...conns[LOCAL].sessions.map((s) => sessionItem(LOCAL, s)));
  $("remote-ul").replaceChildren(...remoteOrder.map((id) => {
    const c = conns[id], st = c.status.state, li = el("li"), b = el("button");
    b.type = "button";
    const [cls, dot, label] = st === "ready" ? ["dot-ok", "●", "conectado"] : st === "closed" && c.status.code ? ["dot-err", "⚠", "erro"] : st === "connecting" ? ["dot-warn", "◌", "conectando"] : ["muted", "○", "desconectado"];
    const d = el("span", cls, dot);
    d.setAttribute("aria-label", label);
    b.append(d, el("span", null, c.name));
    b.setAttribute("aria-expanded", String(st === "ready"));
    b.onclick = () => st === "ready" ? refresh(id) : connectRemote(id);
    b.oncontextmenu = (e) => {
      e.preventDefault();
      menu(b, [st === "ready" || st === "connecting" ? ["Desconectar", () => invoke("remote_disconnect", { id })] : ["Conectar", () => connectRemote(id)],
        ["Remover este PC", () => removeRemote(id)]]);
    };
    b.onkeydown = (e) => { if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); b.oncontextmenu(e); } };
    li.append(b);
    if (st === "ready") {
      const ul = el("ul");
      ul.append(...c.sessions.map((s) => sessionItem(id, s)));
      if (!c.sessions.length) ul.append(el("li", "muted small", `${c.name} não tem terminais abertos.`));
      const nl = el("li");
      nl.append(newButton(id));
      ul.append(nl);
      li.append(ul);
    }
    return li;
  }));
}

async function connectRemote(id) {
  try { await invoke("remote_connect", { id }); } catch (e) { banner(msg(e), "err"); }
}

// ---------- Menu suspenso ----------
let menuOpener = null;
function menu(anchor, items) {
  const m = $("menu");
  m.replaceChildren(...items.map(([label, fn]) => {
    if (!fn) return el("p", "muted small", label);
    const b = el("button", null, label);
    b.type = "button";
    b.setAttribute("role", "menuitem");
    b.onclick = () => { closeMenu(false); fn(); };
    return b;
  }));
  const r = anchor.getBoundingClientRect();
  m.style.left = Math.min(r.left, innerWidth - 200) + "px";
  m.style.top = Math.min(r.bottom, innerHeight - 40 * items.length - 8) + "px";
  m.hidden = false;
  menuOpener = anchor;
  (m.querySelector("button") || m).focus();
}
function closeMenu(refocus = true) { $("menu").hidden = true; if (refocus) menuOpener?.focus(); }
$("menu").onkeydown = (e) => {
  const items = [...$("menu").querySelectorAll("button")], i = items.indexOf(document.activeElement);
  if (e.key === "Escape" || e.key === "Tab") { e.preventDefault(); closeMenu(); }
  else if (e.key === "ArrowDown") { e.preventDefault(); items[(i + 1) % items.length]?.focus(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); items[(i + items.length - 1) % items.length]?.focus(); }
};
document.addEventListener("mousedown", (e) => { if (!$("menu").hidden && !$("menu").contains(e.target)) closeMenu(false); });

function profileMenu(anchor, conn) {
  const p = conns[conn].profiles;
  menu(anchor, p.length ? p.map((x) => [x.label, () => newSession(conn, x)]) : [["Nenhum perfil disponível."]]);
}

// ---------- Diálogos ----------
function trap(dialog, onEscape) {
  dialog.onkeydown = (e) => {
    if (e.key === "Escape") { e.preventDefault(); onEscape(); }
    if (e.key !== "Tab") return;
    const f = [...dialog.querySelectorAll("button:not([disabled]), input:not([disabled])")];
    const [a, b] = [f[0], f[f.length - 1]];
    if (e.shiftKey && document.activeElement === a) { e.preventDefault(); b.focus(); }
    else if (!e.shiftKey && document.activeElement === b) { e.preventDefault(); a.focus(); }
  };
}

function ask(title, text, ok) {
  const opener = document.activeElement;
  $("ask-t").textContent = title;
  $("ask-d").textContent = text;
  $("ask-ok").textContent = ok;
  $("ask-scrim").hidden = false;
  $("ask-cancel").focus();
  return new Promise((done) => {
    const end = (v) => { $("ask-scrim").hidden = true; opener?.focus(); done(v); };
    $("ask-ok").onclick = () => end(true);
    $("ask-cancel").onclick = () => end(false);
    trap($("ask"), () => end(false));
  });
}

async function endSession(conn, s) {
  const label = s.label || "Terminal";
  if (!await ask(`Encerrar ${label}?`, "Programas rodando nele serão fechados.", "Encerrar sessão")) return;
  try { await invoke("term_close", { conn, sessionId: s.session_id }); } catch (e) { banner(msg(e), "err"); }
  refresh(conn);
}

async function removeRemote(id) {
  const name = conns[id].name;
  if (!await ask(`Remover ${name}?`, `Este app esquece ${name}. Para usar de novo, será preciso um novo link. Para tirar o acesso lá também, remova este PC em Aparelhos no outro computador.`, "Remover este PC")) return;
  for (const t of tabs.filter((t) => t.conn === id)) await closeTab(t.key);
  try { await invoke("remote_remove", { id }); } catch (e) { banner(msg(e), "err"); }
  delete conns[id];
  refreshRemotes();
}

// Adicionar PC
let addOpener = null;
function openAdd() {
  addOpener = document.activeElement;
  $("add-link").value = "";
  $("add-name").value = "";
  $("add-link").type = "password";
  $("add-show").setAttribute("aria-pressed", "false");
  addError(null);
  $("add-state").textContent = "";
  setAddBusy(false);
  $("add-scrim").hidden = false;
  $("add-link").focus();
}
function closeAdd() {
  if (adding) invoke("remote_disconnect", { id: adding });
  adding = null;
  $("add-link").value = "";
  $("add-scrim").hidden = true;
  addOpener?.focus();
}
function addError(code) {
  $("add-err").hidden = !code;
  $("add-err").querySelector("p").textContent = code ? msg(code) : "";
  $("add-link").setAttribute("aria-invalid", String(!!code));
}
function setAddBusy(b) { for (const id of ["add-link", "add-name", "add-ok", "add-show"]) $(id).disabled = b; }
$("add-show").onclick = () => {
  const show = $("add-link").type === "password";
  $("add-link").type = show ? "text" : "password";
  $("add-show").setAttribute("aria-pressed", String(show));
  $("add-show").setAttribute("aria-label", show ? "Ocultar link" : "Mostrar link");
};
$("add-cancel").onclick = closeAdd;
trap($("add"), closeAdd);
$("add-link").onkeydown = $("add-name").onkeydown = (e) => { if (e.key === "Enter") $("add-ok").click(); };
$("add-ok").onclick = async () => {
  if (!$("add-link").value.trim()) return addError("UNAUTHORIZED");
  addError(null);
  setAddBusy(true);
  try {
    const r = await invoke("remote_add", { link: $("add-link").value, name: $("add-name").value });
    $("add-link").value = "";
    adding = r.id;
    $("add-state").textContent = `Conectando a ${r.name}…`;
  } catch (e) {
    setAddBusy(false);
    addError(e);
  }
};
$("remote-add").onclick = openAdd;

// ---------- Eventos do Rust ----------
// Seleção some se o DOM for refeito (a cada ~33 ms): não redesenha enquanto há seleção ou arrasto na tela.
listen("term-screen", ({ payload }) => { screens[payload.key] = payload; if (payload.key === active && !holdDraw()) draw(active); });
listen("term-exited", ({ payload }) => {
  const t = tabs.find((t) => t.key === payload.key);
  if (t) { t.exited = payload.code; if (t.key === active) updateBanner(); }
  refresh(payload.key.split("/")[0]);
});
listen("term-conn", async ({ payload }) => {
  const id = payload.conn, prev = conns[id]?.status;
  if (!conns[id] || !remoteOrder.includes(id)) await refreshRemotes();
  if (!conns[id]) return;
  conns[id].status = payload;
  if (id === adding) {
    if (payload.state === "ready") {
      adding = null;
      closeAdd();
    } else if (payload.state === "closed") {
      adding = null;
      $("add-state").textContent = "";
      setAddBusy(false);
      addError(payload.code || "NOT_CONNECTED");
    }
  }
  if (payload.state === "ready") {
    await refresh(id);
    // Reconectou: anexa de novo as abas abertas (tela nova a partir do começo do buffer).
    if (prev?.state === "connecting" && prev.attempt > 0) {
      const { cols, rows } = size();
      for (const t of tabs.filter((t) => t.conn === id && t.exited == null)) invoke("term_attach", { conn: id, sessionId: t.sid, cols, rows }).catch(() => {});
    }
  }
  renderSidebar();
  updateBanner();
});

// ---------- Teclado ----------
const SS3 = { ArrowUp: "A", ArrowDown: "B", ArrowRight: "C", ArrowLeft: "D", Home: "H", End: "F" };
const TILDE = { Insert: 2, Delete: 3, PageUp: 5, PageDown: 6, F5: 15, F6: 17, F7: 18, F8: 19, F9: 20, F10: 21, F11: 23, F12: 24 };
const FN = { F1: "P", F2: "Q", F3: "R", F4: "S" };

/** Bytes que a tecla manda ao PTY (xterm); `null` = não é do terminal. */
function keyBytes(e) {
  const mod = 1 + (e.shiftKey ? 1 : 0) + (e.altKey ? 2 : 0) + (e.ctrlKey ? 4 : 0);
  if (SS3[e.key]) return mod > 1 ? `\x1b[1;${mod}${SS3[e.key]}` : (appCursor ? "\x1bO" : "\x1b[") + SS3[e.key];
  if (TILDE[e.key]) return `\x1b[${TILDE[e.key]}${mod > 1 ? ";" + mod : ""}~`;
  if (FN[e.key]) return mod > 1 ? `\x1b[1;${mod}${FN[e.key]}` : "\x1bO" + FN[e.key];
  const esc = e.altKey ? "\x1b" : "";
  switch (e.key) {
    case "Enter": return esc + "\r";
    case "Backspace": return esc + (e.ctrlKey ? "\x08" : "\x7f");
    case "Tab": return e.shiftKey ? "\x1b[Z" : "\t";
    case "Escape": return "\x1b";
  }
  if (e.key.length !== 1 && [...e.key].length !== 1) return null;
  if (e.ctrlKey && !e.altKey) {
    const c = e.key.toUpperCase().charCodeAt(0);
    if (c >= 64 && c <= 95) return String.fromCharCode(c - 64);
    if (e.key === " " || e.key === "2") return "\x00";
    if (e.key === "/") return "\x1f";
    return null;
  }
  // AltGr chega como Ctrl+Alt: o caractere já vem pronto.
  if (e.ctrlKey && e.altKey) return e.key;
  return esc + e.key;
}

function send(data) {
  const t = tabs.find((t) => t.key === active);
  if (t && t.exited == null) invoke("term_input", { conn: t.conn, sessionId: t.sid, data }).catch(() => {});
}

$("screen").addEventListener("keydown", (e) => {
  if (e.isComposing || e.key === "Process" || e.key === "Dead") return;
  if (screenKey(e)) return;
  if (e.ctrlKey && e.shiftKey && ["C", "V", "T", "W", "F6"].includes(e.key.length === 1 ? e.key.toUpperCase() : e.key)) return;
  if (e.ctrlKey && (e.key === "Tab" || (!e.shiftKey && !e.altKey && e.key.toLowerCase() === "b"))) return;
  const b = keyBytes(e);
  if (b == null) return;
  e.preventDefault();
  e.stopPropagation();
  send(b);
});

document.addEventListener("keydown", (e) => {
  const k = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const t = tabs.find((t) => t.key === active);
  if (e.ctrlKey && e.shiftKey && k === "T") { e.preventDefault(); newSession(t ? t.conn : LOCAL); }
  else if (e.ctrlKey && e.shiftKey && k === "W") { e.preventDefault(); if (active) closeTab(active); }
  else if (e.ctrlKey && k === "Tab") {
    e.preventDefault();
    const i = tabs.findIndex((t) => t.key === active);
    if (tabs.length) activate(tabs[(i + (e.shiftKey ? tabs.length - 1 : 1)) % tabs.length].key);
  }
  else if (e.ctrlKey && !e.shiftKey && !e.altKey && k === "B") {
    e.preventDefault();
    const c = $("layout").classList.toggle("collapsed");
    if (c && $("sidebar").contains(document.activeElement)) $("screen").focus();
  }
  else if (e.ctrlKey && e.shiftKey && k === "C") {
    e.preventDefault();
    copySel();
  }
  else if (e.ctrlKey && e.shiftKey && k === "V") { e.preventDefault(); paste(); }
  else if (e.ctrlKey && e.shiftKey && k === "F6") {
    e.preventDefault();
    $("layout").classList.remove("collapsed");
    ($("sidebar").querySelector("[aria-current=true]") || $("local-new")).focus();
  }
});

$("screen").addEventListener("wheel", (e) => {
  const t = tabs.find((t) => t.key === active);
  if (e.ctrlKey && e.deltaY) { e.preventDefault(); return zoom(e.deltaY < 0 ? 1 : -1); }
  if (!t || !e.deltaY) return;
  e.preventDefault();
  invoke("term_scroll", { conn: t.conn, sessionId: t.sid, lines: e.deltaY < 0 ? 3 : -3 });
}, { passive: false });

// ---------- F2: tela (copiar/colar, menu, busca, links, fonte, arrastar, ajuda) ----------
const curTab = () => tabs.find((t) => t.key === active);
const rowsEl = () => [...$("screen").children].filter((n) => n.id !== "measure");
let selecting = false, pendingDraw = false;
const hasSel = () => { const s = getSelection(); return !s.isCollapsed && $("screen").contains(s.anchorNode); };
function holdDraw() { const h = selecting || hasSel(); pendingDraw ||= h; return h; }
function flushDraw() { if (pendingDraw && !holdDraw()) { pendingDraw = false; draw(active); } }
$("screen").addEventListener("mousedown", (e) => { if (e.button === 0) selecting = true; });
document.addEventListener("mouseup", () => { selecting = false; setTimeout(flushDraw); });
document.addEventListener("selectionchange", () => { if (!selecting) flushDraw(); });

function copySel() {
  const s = getSelection().toString();
  if (!s) return;
  invoke("clip_copy", { text: s }).catch((err) => banner(msg(err), "err"));
  getSelection().removeAllRanges();
  $("screen").focus();
}
function paste() {
  const t = curTab();
  if (t && t.exited == null) invoke("term_paste", { conn: t.conn, sessionId: t.sid }).catch((err) => banner(msg(err), "err"));
  $("screen").focus();
}
function selectAll() {
  const r = rowsEl();
  if (!r.length) return;
  const g = new Range();
  g.setStartBefore(r[0]);
  g.setEndAfter(r[r.length - 1]);
  getSelection().removeAllRanges();
  getSelection().addRange(g);
}
const action = (name) => () => document.dispatchEvent(new CustomEvent("term-action", { detail: name }));

/** Teclas da tela tratadas aqui (não vão ao PTY). true = consumida. */
function screenKey(e) {
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key, plain = e.ctrlKey && !e.shiftKey && !e.altKey;
  let done = true;
  if (plain && k === "c" && hasSel()) copySel();
  else if (plain && k === "v") paste();
  else if (plain && k === "f") openFind();
  else if (plain && (k === "=" || k === "+")) zoom(1);
  else if (plain && k === "-") zoom(-1);
  else if (plain && k === "0") zoom(0);
  else if (!e.ctrlKey && !e.altKey && !e.shiftKey && k === "F1") help();
  else if (k === "ContextMenu" || (e.shiftKey && k === "F10")) { const r = $("screen").getBoundingClientRect(); ctxMenu(r.left + 20, r.top + 20); }
  else done = false;
  if (done) { e.preventDefault(); e.stopPropagation(); }
  return done;
}

// Menu de contexto da tela (substitui o nativo do WebView: Recarregar, Inspecionar…).
function ctxMenu(x, y) {
  const at = { getBoundingClientRect: () => ({ left: x, bottom: y }), focus: () => $("screen").focus() };
  menu(at, [
    ["Copiar   Ctrl+C", hasSel() ? copySel : null], ["Colar   Ctrl+V", paste], ["Selecionar tudo", selectAll], ["Buscar   Ctrl+F", openFind],
    ["Nova sessão   Ctrl+Shift+T", action("new")], ["Renomear aba", action("rename")], ["Fechar aba   Ctrl+Shift+W", action("close")],
    ["Exportar saída", action("export")], ["Dividir painel", action("split")], ["Paleta de comandos", action("palette")], ["Tema", action("theme")],
    ["Aumentar fonte   Ctrl+=", () => zoom(1)], ["Diminuir fonte   Ctrl+-", () => zoom(-1)], ["Redefinir fonte   Ctrl+0", () => zoom(0)],
    // ponytail: Ctrl+L limpa em pwsh/bash; cmd ignora. Trocar por comando no Rust (limpar buffer vt100) se precisar.
    ["Limpar", () => { send("\x0c"); $("screen").focus(); }],
  ].filter(([, fn]) => fn));
}
$("screen").oncontextmenu = (e) => { e.preventDefault(); ctxMenu(e.clientX, e.clientY); };

// Fonte: Ctrl+roda, Ctrl+=/-/0; salva no localStorage e recalcula cols/rows.
const FONT0 = 13.6;
let font = +localStorage.getItem("term-font") || FONT0;
function zoom(d) {
  font = d ? Math.max(8, Math.min(32, font + d)) : FONT0;
  localStorage.setItem("term-font", font);
  $("screen").style.fontSize = font + "px";
  fit();
}
$("screen").style.fontSize = font + "px";

// Busca (Ctrl+F) nas linhas desenhadas, com CSS Highlight; no fim da tela rola o histórico (term_scroll).
let findQ = "", findI = 0, findDir = 0, findHits = [];
function openFind() { $("find").hidden = false; $("find-q").select(); $("find-q").focus(); }
function closeFind() { $("find").hidden = true; findQ = ""; afterDraw(); $("screen").focus(); }
function afterDraw() {
  CSS.highlights?.delete("find"); CSS.highlights?.delete("find-cur");
  findHits = [];
  if (!findQ) { $("find-n").textContent = ""; return; }
  const q = findQ.toLowerCase();
  for (const row of rowsEl()) {
    const nodes = [], w = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
    let text = "";
    while (w.nextNode()) { nodes.push([w.currentNode, text.length]); text += w.currentNode.data; }
    text = text.toLowerCase();
    const at = (o) => { const [n, s] = nodes.findLast(([, s]) => s <= o) || nodes[0]; return [n, Math.min(o - s, n.data.length)]; };
    for (let i = text.indexOf(q); i >= 0; i = text.indexOf(q, i + 1)) {
      const r = new Range();
      r.setStart(...at(i)); r.setEnd(...at(i + q.length));
      findHits.push(r);
    }
  }
  if (findDir) findI = findDir < 0 ? findHits.length - 1 : 0;
  findDir = 0;
  findI = Math.max(0, Math.min(findI, findHits.length - 1));
  if (findHits.length && window.Highlight) {
    CSS.highlights.set("find", new Highlight(...findHits));
    CSS.highlights.set("find-cur", new Highlight(findHits[findI]));
  }
  $("find-n").textContent = findHits.length ? `${findI + 1} de ${findHits.length}` : "Nada nesta tela";
}
function findStep(d) {
  if (findHits.length && findI + d >= 0 && findI + d < findHits.length) { findI += d; return afterDraw(); }
  // Acabou na tela: rola uma tela no histórico (para cima = anteriores) e procura de novo no próximo desenho.
  const t = curTab();
  if (!t) return;
  findDir = d;
  const n = size().rows;
  invoke("term_scroll", { conn: t.conn, sessionId: t.sid, lines: d < 0 ? n : -n }).catch(() => {});
}
$("find-q").oninput = () => { findQ = $("find-q").value; findI = 0; afterDraw(); };
$("find-q").onkeydown = (e) => {
  if (e.key === "Enter") { e.preventDefault(); findStep(e.shiftKey ? -1 : 1); }
  else if (e.key === "Escape") { e.preventDefault(); closeFind(); }
};
$("find-prev").onclick = () => findStep(-1);
$("find-next").onclick = () => findStep(1);
$("find-x").onclick = closeFind;

// Links: Ctrl+clique abre URL no navegador; caminho Windows abre no Explorer (mostra o item, não executa).
$("screen").addEventListener("click", (e) => {
  if (!e.ctrlKey) return;
  const c = document.caretRangeFromPoint(e.clientX, e.clientY), row = c && rowsEl().find((r) => r.contains(c.startContainer));
  if (!row) return;
  const pre = new Range();
  pre.setStart(row, 0); pre.setEnd(c.startContainer, c.startOffset);
  const link = linkAt(row.textContent, pre.toString().length);
  if (!link) return;
  e.preventDefault();
  const op = window.__TAURI__.opener;
  (/^https?:/.test(link) ? op.openUrl(link) : op.revealItemInDir(link)).catch(() => banner(msg("NOT_FOUND"), "err"));
});
$("screen").addEventListener("mousemove", (e) => $("screen").classList.toggle("ctrl", e.ctrlKey));

// Arrastar arquivo(s) para a janela: cola o(s) caminho(s) entre aspas.
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent(({ payload }) => {
  if (payload.type !== "drop" || !payload.paths?.length) return;
  send(payload.paths.map((p) => `"${p}"`).join(" "));
  $("screen").focus();
});

// Ajuda (F1).
const KEYS = [["Ctrl+C", "Copiar seleção (sem seleção: interrompe, ^C)"], ["Ctrl+V / Ctrl+Shift+V", "Colar"], ["Ctrl+Shift+C", "Copiar seleção"],
  ["Ctrl+F", "Buscar"], ["Enter / Shift+Enter", "Próximo / anterior na busca"], ["Ctrl+Shift+T", "Nova sessão"], ["Ctrl+Shift+W", "Fechar aba"],
  ["Ctrl+Tab / Ctrl+Shift+Tab", "Próxima / aba anterior"], ["Ctrl+B", "Mostrar/ocultar lista lateral"], ["Ctrl+Shift+F6", "Ir para a lista lateral"],
  ["Ctrl+= / Ctrl+- / Ctrl+0", "Aumentar / diminuir / redefinir fonte"], ["Ctrl+roda do mouse", "Zoom da fonte"], ["Roda do mouse", "Rolar histórico"],
  ["Ctrl+clique", "Abrir link ou caminho"], ["Botão direito / Shift+F10", "Menu do terminal"], ["Arrastar arquivo", "Cola o caminho"], ["F1", "Esta ajuda"]];
function help() {
  const opener = document.activeElement;
  $("help-list").replaceChildren(...KEYS.flatMap(([k, d]) => [el("dt", "mono", k), el("dd", null, d)]));
  $("help-scrim").hidden = false;
  const end = () => { $("help-scrim").hidden = true; opener?.focus(); };
  $("help-ok").onclick = end;
  trap($("help"), end);
  $("help-ok").focus();
}

// ---------- Tamanho ----------
let resizeTimer = 0;
function fit() {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    const t = tabs.find((t) => t.key === active);
    if (!t || t.exited != null) return;
    const { cols, rows } = size();
    invoke("term_resize", { conn: t.conn, sessionId: t.sid, cols, rows }).catch(() => {});
  }, 100);
}
new ResizeObserver(fit).observe($("screen"));

// ---------- Início ----------
$("local-new").onclick = () => profileMenu($("local-new"), LOCAL);
$("empty-new").onclick = () => newSession(LOCAL);
refresh(LOCAL);
refreshRemotes();
// Sessões criadas pelo celular aparecem sozinhas; não redesenha a lista sob o foco do teclado.
// ponytail: polling de 5 s; trocar por evento do host quando C3 tiver aviso de sessão nova.
setInterval(() => { if (!document.hidden && !$("sidebar").contains(document.activeElement)) refresh(LOCAL); }, 5000);
renderTabs();
updateBanner();

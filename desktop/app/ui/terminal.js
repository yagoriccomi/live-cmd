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
const tabs = [];       // { key, conn, sid, label, title, exited, split? }  split = { key, conn, sid } do painel direito
let active = null;     // key
let appCursor = false;
const screens = {};    // key → última tela recebida
let adding = null;     // id do PC sendo adicionado
const busy = new Set();   // chaves com saída nova fora de vista (item 8)
const closed = [];        // abas fechadas com sessão viva, para reabrir (item 7)
const lastEnter = {};     // chave → hora do último Enter (item 9)

// ---------- Medida da célula e tamanho ----------
function size() {
  const m = $("measure").getBoundingClientRect(), s = $("screen"), cs = getComputedStyle(s);
  const w = s.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const h = s.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const lh = parseFloat(cs.lineHeight) || m.height;
  return { cols: Math.max(20, Math.min(1000, Math.floor(w / (m.width / 10)))), rows: Math.max(5, Math.min(1000, Math.floor(h / lh))) };
}

// ---------- Desenho ----------
function draw(key, s = $("screen")) {
  const scr = screens[key];
  const keep = [...s.children].filter((n) => n.id === "measure"), lines = [];
  if (!scr) return s.replaceChildren(...keep);
  if (s === pane()) appCursor = scr.app_cursor;
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
    lines.push(line);
  }
  s.replaceChildren(...keep, ...lines);
  afterDraw();
}

// Junta várias telas recebidas no mesmo quadro num só desenho.
const soon = new Map();
function drawSoon(key, s = $("screen")) {
  if (!soon.size) requestAnimationFrame(() => { const q = [...soon]; soon.clear(); for (const [el, k] of q) draw(k, el); });
  soon.set(s, key);
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
let tabsLater = false;
function renderTabs() {
  const bar = $("tabs");
  if (bar.querySelector("input.rename")) { tabsLater = true; return; }
  bar.replaceChildren(...tabs.map((t) => {
    const b = el("button");
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(t.key === active));
    b.tabIndex = t.key === active ? 0 : -1;
    b.dataset.key = t.key;
    if (t.conn !== LOCAL) b.append(pcMark(t.conn));
    const name = el("span", "tab-name", t.title + (t.split ? " | dividido" : ""));
    name.ondblclick = (e) => { e.stopPropagation(); renameSession(t.conn, t.sid, name, t.label); };
    b.append(name);
    if (busy.has(t.key)) b.append(actDot());
    b.draggable = true;
    b.ondragstart = (e) => e.dataTransfer.setData("text/x-tab", t.key);
    b.ondragover = (e) => e.preventDefault();
    b.ondrop = (e) => { e.preventDefault(); moveTab(e.dataTransfer.getData("text/x-tab"), t.key); };
    b.oncontextmenu = (e) => { e.preventDefault(); menu(b, tabMenu(t, name)); };
    const x = el("span", "x", "×");
    x.setAttribute("aria-hidden", "true");
    x.title = "Fechar aba (Ctrl+Shift+W)";
    x.onclick = (e) => { e.stopPropagation(); closeTab(t.key); };
    b.append(x);
    b.onclick = () => { if (t.key !== active) activate(t.key); };
    b.onauxclick = (e) => { if (e.button === 1) closeTab(t.key); };
    return b;
  }));
  $("empty").hidden = tabs.length > 0;
  const t = tabs.find((t) => t.key === active);
  $("screen").setAttribute("aria-label", t ? "Terminal " + t.title : "Terminal");
  renderSidebar();
}

function moveTab(from, to) {
  const i = tabs.findIndex((t) => t.key === from), j = tabs.findIndex((t) => t.key === to);
  if (i < 0 || j < 0 || i === j) return;
  tabs.splice(j, 0, ...tabs.splice(i, 1));
  renderTabs();
}

function tabMenu(t, name) {
  return [["Renomear", () => renameSession(t.conn, t.sid, name, t.label)],
    t.split ? ["Desfazer divisão", () => unsplit(t)] : ["Dividir", () => splitMenu(name)],
    ["Reabrir aba fechada (Ctrl+Alt+T)", reopen],
    ["Fechar aba", () => closeTab(t.key)]];
}

const actDot = () => { const d = el("span", "act", "•"); d.setAttribute("aria-label", "saída nova"); return d; };

// ---------- Item 13: cor por PC (só neste app) ----------
const PALETTE = [["Azul", "#4f8cff"], ["Verde", "#2fbf71"], ["Laranja", "#f5a524"], ["Vermelho", "#e5484d"], ["Roxo", "#a970ff"], ["Ciano", "#12a5b0"]];
const pcColor = (id) => localStorage.getItem("pc-color:" + id) || PALETTE[0][1];
function pcMark(id) { const m = el("span", "pc-mark"); m.style.background = pcColor(id); m.setAttribute("aria-hidden", "true"); return m; }
function colorMenu(anchor, id) {
  menu(anchor, PALETTE.map(([n, c]) => [n, () => { localStorage.setItem("pc-color:" + id, c); renderTabs(); }]));
}

// ---------- Item 1/3: renomear inline (Enter salva, Esc cancela) ----------
function editInline(span, value, save) {
  const inp = el("input", "rename");
  inp.value = value;
  inp.maxLength = 64;
  inp.setAttribute("aria-label", "Novo nome");
  let done = false;
  const end = async (ok) => {
    if (done) return;
    done = true;
    document.removeEventListener("pointerdown", down, true);
    const v = inp.value.trim();
    inp.replaceWith(span);
    if (tabsLater) { tabsLater = false; renderTabs(); }
    if (ok && v && v !== value) { try { await save(v); } catch (e) { banner(msg(e), "err"); } }
  };
  inp.onkeydown = (e) => { e.stopPropagation(); if (e.key === "Enter") end(true); else if (e.key === "Escape") end(false); };
  inp.onkeyup = (e) => { e.stopPropagation(); e.preventDefault(); };
  inp.onclick = inp.ondblclick = inp.onmousedown = (e) => e.stopPropagation();
  // Foco roubado (redesenho, atualização) não cancela: volta ao campo. Clique fora salva.
  let out = false;
  const down = (e) => { if (e.target !== inp) out = true; };
  document.addEventListener("pointerdown", down, true);
  inp.onblur = () => { if (done) return; if (out || !document.hasFocus()) { document.removeEventListener("pointerdown", down, true); end(true); } else setTimeout(() => inp.isConnected && inp.focus()); };
  span.replaceWith(inp);
  inp.focus();
  inp.select();
}

function renameSession(conn, sid, span, label) {
  const cur = conns[conn]?.sessions.find((s) => s.session_id === sid)?.label || label || "Terminal";
  // Menu aberto antes de um redesenho guarda o span antigo (fora do DOM): usa o da aba atual.
  if (!span.isConnected) span = $("tabs").querySelector(`[data-key="${CSS.escape(conn + "/" + sid)}"] .tab-name`);
  if (!span) return;
  editInline(span, cur, async (v) => {
    await invoke("term_rename", { conn, sessionId: sid, label: v });
    for (const t of tabs) if (t.conn === conn && t.sid === sid) { t.label = v; t.title = titleOf(conn, { label: v }); }
    await refresh(conn);
    renderTabs();
  });
}

function renamePc(id, span) {
  editInline(span, conns[id].name, async (v) => {
    await invoke("remote_rename", { id, name: v });
    await refreshRemotes();
    for (const t of tabs) if (t.conn === id) t.title = titleOf(id, { label: t.label });
    renderTabs();
  });
}

// ---------- Item 5: divisão (uma por aba, dois painéis lado a lado) ----------
const pane = () => document.activeElement === $("screen2") ? $("screen2") : $("screen");
const activeTab = () => tabs.find((t) => t.key === active);
/** Sessão que recebe o teclado: painel direito se ele tem o foco. */
function target() {
  const t = activeTab();
  if (t?.split && pane() === $("screen2")) return { ...t.split, exited: null };
  return t;
}

function splitMenu(anchor) {
  const t = activeTab();
  if (!t) return;
  const opts = [];
  for (const [conn, c] of Object.entries(conns)) for (const s of c.sessions || [])
    if (s.state !== "exited" && conn + "/" + s.session_id !== t.key) opts.push([titleOf(conn, s), () => split(t, conn, s.session_id)]);
  menu(anchor || $("tabs"), opts.length ? opts : [["Nenhuma outra sessão aberta."]]);
}

async function split(t, conn, sid) {
  const key = conn + "/" + sid, { cols, rows } = size();
  if (!tabs.some((x) => x.key === key)) {
    try { await invoke("term_attach", { conn, sessionId: sid, cols, rows }); } catch (e) { return banner(msg(e), "err"); }
  }
  if (t.split) unsplit(t, false);
  t.split = { key, conn, sid };
  activate(t.key);
}

function unsplit(t, redraw = true) {
  const sp = t.split;
  t.split = null;
  if (sp && !tabs.some((x) => x.key === sp.key)) { delete screens[sp.key]; invoke("term_detach", { conn: sp.conn, sessionId: sp.sid }).catch(() => {}); }
  if (redraw) activate(t.key);
}

// ---------- Item 7: reabrir aba fechada ----------
async function reopen() {
  while (closed.length) {
    const { conn, sid } = closed.pop();
    const s = conns[conn]?.sessions.find((s) => s.session_id === sid && s.state !== "exited");
    if (s) return openSession(conn, s);
  }
  banner("Nenhuma aba fechada para reabrir.", "warn");
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
  const t = tabs.find((t) => t.key === key);
  busy.delete(key);
  if (t?.split) busy.delete(t.split.key);
  $("screen2").hidden = !t?.split;
  document.querySelector(".term-area").classList.toggle("split", !!t?.split);
  renderTabs();
  draw(key);
  if (t?.split) draw(t.split.key, $("screen2"));
  updateBanner();
  applyFont();
  resizeActive();
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
    tabs.push({ key, conn, sid: s.session_id, label: s.label, title: titleOf(conn, s), exited: ok.state === "exited" ? ok.exit_code ?? 0 : null });
  }
  activate(key);
}

async function closeTab(key) {
  const i = tabs.findIndex((t) => t.key === key);
  if (i < 0) return;
  const [t] = tabs.splice(i, 1);
  if (t.split) unsplit(t, false);
  if (t.exited == null) closed.push({ conn: t.conn, sid: t.sid });
  if (closed.length > 20) closed.shift();
  busy.delete(key);
  // Painel direito de outra aba ainda mostra esta sessão: não desanexa.
  if (!tabs.some((x) => x.split?.key === key)) { delete screens[key]; invoke("term_detach", { conn: t.conn, sessionId: t.sid }).catch(() => {}); }
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

const dropCwds = {}; // pasta atual por aba (OSC 7), para arrastar/colar em remoto

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

// Item 14: latência e tipo de rota de cada PC conectado, a cada 5 s.
setInterval(async () => {
  let changed = false;
  for (const id of remoteOrder) {
    const c = conns[id];
    if (c?.status.state !== "ready") continue;
    const s = await invoke("term_conn_stats", { conn: id }).catch(() => null);
    const q = s?.rtt_ms == null ? null : `${s.rtt_ms} ms` + (s.relay == null ? "" : s.relay ? " · relay" : " · direta");
    if (q !== c.q) { c.q = q; changed = true; }
  }
  if (changed) renderSidebar();
}, 5000);

function sessionItem(conn, s) {
  const li = el("li"), b = el("button");
  b.type = "button";
  const key = conn + "/" + s.session_id;
  if (key === active) b.setAttribute("aria-current", "true");
  const name = el("span", null, s.label || "Terminal");
  name.ondblclick = (e) => { e.stopPropagation(); renameSession(conn, s.session_id, name); };
  b.append(el("span", s.state === "exited" ? "muted" : "dot-ok", s.state === "exited" ? "○" : "●"), name);
  if (busy.has(key)) b.append(actDot());
  b.onclick = () => openSession(conn, s);
  b.oncontextmenu = (e) => { e.preventDefault(); menu(b, [["Renomear", () => renameSession(conn, s.session_id, name)], ["Encerrar sessão", () => endSession(conn, s)]]); };
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
  if ($("sidebar").querySelector("input.rename")) { tabsLater = true; return; }
  $("local-ul").replaceChildren(...conns[LOCAL].sessions.map((s) => sessionItem(LOCAL, s)));
  $("remote-ul").replaceChildren(...remoteOrder.map((id) => {
    const c = conns[id], st = c.status.state, li = el("li"), b = el("button");
    b.type = "button";
    const [cls, dot, label] = st === "ready" ? ["dot-ok", "●", "conectado"] : st === "closed" && c.status.code ? ["dot-err", "⚠", "erro"] : st === "connecting" ? ["dot-warn", "◌", "conectando"] : ["muted", "○", "desconectado"];
    const d = el("span", cls, dot);
    d.setAttribute("aria-label", label);
    const name = el("span", null, c.name);
    name.ondblclick = (e) => { e.stopPropagation(); renamePc(id, name); };
    // Item 14: estado por extenso; conectado mostra latência medida (pollStats).
    const q = el("span", "muted small conn-q", st === "connecting" ? (c.status.attempt ? `reconectando (tentativa ${c.status.attempt + 1})` : "conectando…") : st === "ready" && c.q ? c.q : label);
    b.append(pcMark(id), d, name, q);
    b.setAttribute("aria-expanded", String(st === "ready"));
    b.onclick = () => st === "ready" ? refresh(id) : connectRemote(id);
    b.oncontextmenu = (e) => {
      e.preventDefault();
      menu(b, [st === "ready" || st === "connecting" ? ["Desconectar", () => invoke("remote_disconnect", { id })] : ["Conectar", () => connectRemote(id)],
        ["Renomear", () => renamePc(id, name)], ["Cor…", () => colorMenu(b, id)], ["Remover este PC", () => removeRemote(id)]]);
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
  m.hidden = false;
  m.style.left = Math.max(0, Math.min(r.left, innerWidth - m.offsetWidth - 4)) + "px";
  m.style.top = Math.max(0, Math.min(r.bottom, innerHeight - m.offsetHeight - 4)) + "px";
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
  menu(anchor, p.length ? [...p.map((x) => [x.label, () => newSession(conn, x)]), ["Agente de IA…", () => newAi(conn)]] : [["Nenhum perfil disponível."]]);
}

const AI_SUG = {
  claude: ["--model opus", "--model sonnet", "--continue", "--permission-mode plan", "--permission-mode acceptEdits", "--dangerously-skip-permissions", "--verbose"],
  openclaude: ["--model opus", "--model sonnet", "--continue", "--permission-mode plan", "--permission-mode acceptEdits", "--dangerously-skip-permissions", "--verbose"],
  codex: ["--model gpt-5", "--full-auto", "--search", "--sandbox read-only", "--sandbox workspace-write", "--ask-for-approval on-request"],
};
// Agente de IA: pergunta agente, pasta e args; o host salva e retoma pelo id da sessão.
async function newAi(conn) {
  const d = JSON.parse(localStorage.getItem("ai-default") || "{}");
  $("ai-agent").value = d.agent || "claude"; $("ai-cwd").value = d.cwd || ""; $("ai-args").value = d.args || ""; $("ai-def").checked = false;
  const sug = () => $("ai-sug").replaceChildren(...(AI_SUG[$("ai-agent").value] || []).map((v) => { const o = el("option"); o.value = v; return o; }));
  $("ai-agent").onchange = sug; sug();
  // Navegação: subpastas no PC da conexão; clique entra, ".." sobe, raiz lista as unidades.
  const browse = async (path) => {
    let dirs = [];
    try { dirs = await invoke("term_dirs", { conn, path }); } catch (e) { banner(msg(e), "err"); }
    const base = path.replace(/[\/]+$/, ""), up = base.includes("\\") ? base.slice(0, base.lastIndexOf("\\") + 1) : "";
    const item = (name, to) => { const b = el("button", null, name); b.type = "button"; b.onclick = () => { $("ai-cwd").value = to; browse(to); }; return b; };
    $("ai-dirs").replaceChildren(...(path ? [item("..", up)] : []), ...dirs.map((d) => item(d, path ? base + "\\" + d : d)));
  };
  $("ai-cwd").onchange = () => browse($("ai-cwd").value.trim());
  browse($("ai-cwd").value.trim());
  $("ai-scrim").hidden = false; $("ai-cwd").focus();
  const ok = await new Promise((done) => {
    const end = (v) => { $("ai-scrim").hidden = true; done(v); };
    $("ai-ok").onclick = () => end(true); $("ai-cancel").onclick = () => end(false);
    trap($("ai"), () => end(false));
  });
  if (!ok) return;
  const agent = $("ai-agent").value, cwd = $("ai-cwd").value.trim() || null, args = $("ai-args").value.trim();
  if ($("ai-def").checked) localStorage.setItem("ai-default", JSON.stringify({ agent, cwd, args }));
  const p = conns[conn].profiles[0];
  if (!p) return banner(msg("NOT_FOUND"), "err");
  // Grupo = nome da pasta raiz.
  const label = `${(cwd || "").split(/[\/]/).filter(Boolean).pop() || "~"} · ${agent}`.slice(0, 64);
  const { cols, rows } = size();
  try {
    const sid = await invoke("term_create", { conn, profileId: p.profile_id, label, cols, rows, cwd, agent, args });
    await openSession(conn, { session_id: sid, label, state: "running" });
    refresh(conn);
  } catch (e) { banner(msg(e), "err"); }
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
listen("term-screen", ({ payload }) => {
  const k = payload.key, t = activeTab();
  screens[k] = payload;
  if (k === active) { if (!holdDraw()) drawSoon(k); }
  else if (t?.split?.key === k) drawSoon(k, $("screen2"));
  else if (!busy.has(k)) { busy.add(k); renderTabs(); }
});
// Item 9 (evento do F1): avisa no Windows se o comando levou mais de 10 s e não estava à vista.
listen("term-cmd-done", ({ payload }) => {
  const t = activeTab(), seen = document.hasFocus() && (payload.key === active || t?.split?.key === payload.key);
  const start = lastEnter[payload.key];
  delete lastEnter[payload.key];
  if (seen || !start || Date.now() - start < 10000) return;
  const tab = tabs.find((x) => x.key === payload.key);
  invoke("plugin:notification|notify", { options: { title: "Comando terminou", body: `${tab?.title || "Terminal"}: código ${payload.code ?? 0}` } }).catch(() => {});
});
// Ações do menu do terminal (F2).
document.addEventListener("term-action", (e) => {
  const a = e.detail?.action ?? e.detail, t = activeTab();
  if (a === "rename" && t) { const n = $("tabs").querySelector(`[data-key="${CSS.escape(t.key)}"] .tab-name`); if (n) renameSession(t.conn, t.sid, n, t.label); }
  else if (a === "split") splitMenu();
  else if (a === "new") newSession(t ? t.conn : LOCAL);
  else if (a === "close" && t) closeTab(t.key);
  else if (a === "export" && t) window.__TAURI__.dialog.save({ defaultPath: `${(t.label || "terminal").replace(/[\\/:*?"<>|]/g, "_")}.txt`, filters: [{ name: "Texto", extensions: ["txt"] }] })
    .then((path) => path && invoke("term_export_file", { conn: t.conn, sessionId: t.sid, path }).then(() => banner("Saída salva em " + path, "ok")))
    .catch((err) => banner(msg(err), "err"));
});
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
  const t = target();
  if (t && data.includes("\r")) lastEnter[t.key] = Date.now();
  if (t && t.exited == null) invoke("term_input", { conn: t.conn, sessionId: t.sid, data }).catch(() => {});
}

const termKey = (e) => {
  if (e.isComposing || e.key === "Process" || e.key === "Dead") return;
  if (screenKey(e)) return;
  if (e.ctrlKey && e.shiftKey && ["C", "V", "T", "W", "F6", "B"].includes(e.key.length === 1 ? e.key.toUpperCase() : e.key)) return;
  if (e.ctrlKey && e.key === "Tab") return;
  const b = keyBytes(e);
  if (b == null) return;
  e.preventDefault();
  e.stopPropagation();
  send(b);
};
$("screen").addEventListener("keydown", termKey);
$("screen2").addEventListener("keydown", termKey);
// Campo editável para o Windows achar caixa de texto (ditado Win+H, teclado de toque). DOM nunca muda: texto vira input do PTY.
for (const s of [$("screen"), $("screen2")]) {
  s.contentEditable = "true";
  s.spellcheck = false;
  // Ditado (Win+H) reenvia a frase inteira a cada parcial: manda só a diferença.
  let draft = "";
  s.addEventListener("beforeinput", (e) => {
    e.preventDefault();
    const d = e.data || e.dataTransfer?.getData("text/plain") || "";
    // Teclado físico já vai por termKey; aqui chega ditado/IME, que manda a frase acumulada.
    if (e.inputType === "insertText" || e.inputType === "insertCompositionText" || e.inputType === "insertReplacementText") {
      let i = 0;
      while (i < draft.length && i < d.length && draft[i] === d[i]) i++;
      if (i === 0) draft = ""; // frase nova, não apaga a anterior
      send("\x7f".repeat(draft.length - i) + d.slice(i));
      draft = d;
    } else {
      draft = "";
      if (e.inputType.startsWith("insert") && d) send(d);
      else if (e.inputType === "deleteContentBackward") send("\x7f");
    }
  });
  for (const ev of ["keydown", "blur", "mousedown"]) s.addEventListener(ev, () => { draft = ""; });
}

document.addEventListener("keydown", (e) => {
  const k = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const t = tabs.find((t) => t.key === active);
  if (e.ctrlKey && e.altKey && !e.shiftKey && k === "T") { e.preventDefault(); reopen(); }
  else if (e.ctrlKey && e.shiftKey && k === "T") { e.preventDefault(); newSession(t ? t.conn : LOCAL); }
  else if (e.ctrlKey && e.shiftKey && k === "W") { e.preventDefault(); if (active) closeTab(active); }
  else if (e.ctrlKey && k === "Tab") {
    e.preventDefault();
    const i = tabs.findIndex((t) => t.key === active);
    if (tabs.length) activate(tabs[(i + (e.shiftKey ? tabs.length - 1 : 1)) % tabs.length].key);
  }
  else if (e.ctrlKey && e.shiftKey && !e.altKey && k === "B") {
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

$("screen2").oncontextmenu = (e) => e.preventDefault();

const wheel = (e) => {
  if (e.ctrlKey && e.deltaY) { e.preventDefault(); return zoom(e.deltaY < 0 ? 1 : -1); }
  const t = e.currentTarget.id === "screen2" ? activeTab()?.split : activeTab();
  if (!t || !e.deltaY) return;
  e.preventDefault();
  invoke("term_scroll", { conn: t.conn, sessionId: t.sid, lines: e.deltaY < 0 ? 3 : -3 });
};
$("screen").addEventListener("wheel", wheel, { passive: false });
$("screen2").addEventListener("wheel", wheel, { passive: false });

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
  if (!t || t.exited != null) return $("screen").focus();
  const dir = dropCwds[t.key] ?? null;
  // Print: local vai para <pasta>\.live-cmd; remoto é enviado ao outro PC. Cola o caminho (claude/codex anexam pelo caminho).
  invoke("term_paste", { conn: t.conn, sessionId: t.sid, targetDir: dir }).then((name) => {
    if (!name) return;
    send(dir ? `"${dir.replace(/[\\/]+$/, "")}\\${name}"` : name);
    banner(`Print enviado: ${name} em ${dir ?? "pasta padrão do outro PC"}.`);
  }).catch((err) => banner(msg(err), "err"));
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
  const hit = Object.keys(SHORTCUTS).find((a) => keyName(e) === binds[a]);
  if (plain && k === "c" && hasSel()) copySel();
  else if (hit) SHORTCUTS[hit][1]();
  else if (k === "ContextMenu" || (e.shiftKey && k === "F10")) { const r = $("screen").getBoundingClientRect(); ctxMenu(r.left + 20, r.top + 20); }
  else done = false;
  if (done) { e.preventDefault(); e.stopPropagation(); }
  return done;
}

// Menu de contexto da tela (substitui o nativo do WebView: Recarregar, Inspecionar…).
function ctxMenu(x, y) {
  const at = { getBoundingClientRect: () => ({ left: x, bottom: y }), focus: () => $("screen").focus() };
  menu(at, [
    ["Copiar   Ctrl+C", hasSel() ? copySel : null], ["Colar   Ctrl+V", paste], ["Selecionar tudo", selectAll], ["Buscar   Ctrl+Shift+F", openFind],
    ["Nova sessão   Ctrl+Shift+T", action("new")], ["Nova sessão nesta pasta", action("new-here")], ["Reabrir aba fechada   Ctrl+Alt+T", reopen],["Renomear aba", action("rename")], ["Fechar aba   Ctrl+Shift+W", action("close")],
    ["Exportar saída", action("export")], ["Dividir painel", action("split")], ["Paleta de comandos", action("palette")], ["Tema", action("theme")],
    ["Aumentar fonte", () => zoom(1)], ["Diminuir fonte", () => zoom(-1)], ["Redefinir fonte", () => zoom(0)],
    ["Limpar", clearTab], ["Atalhos e ajuda   F1", () => help()],
  ].filter(([, fn]) => fn));
}
// Limpa o buffer vt100 no app (vale em cmd) e manda Ctrl+L para o shell redesenhar o prompt.
function clearTab() {
  const t = curTab();
  if (t) invoke("term_clear", { conn: t.conn, sessionId: t.sid }).then(() => { send("\x0c"); draw(t.key); }).catch((err) => banner(msg(err), "err"));
  $("screen").focus();
}
// Botão direito: copia se há seleção, senão cola (como no Windows Terminal). Shift+botão direito abre o menu.
$("screen").oncontextmenu = (e) => {
  e.preventDefault();
  if (e.shiftKey) ctxMenu(e.clientX, e.clientY);
  else if (hasSel()) copySel();
  else paste();
};
// ☰ Menu: só as ações da bandeja; o resto já tem lugar (barra, Shift+botão direito, atalhos).
$("tb-menu").onclick = () => menu($("tb-menu"), [["Novo pareamento (QR Code)", "pair"], ["Dispositivos conectados", "devices"], ["Configurações", "settings"], ["Sair do live-cmd", "quit"]]
  .map(([n, id]) => [n, () => invoke("tray_action", { id }).catch((err) => banner(msg(err), "err"))]));
$("tb-new").onclick = action("new");
$("tb-find").onclick = () => openFind();
$("tb-split").onclick = action("split");
$("tb-pal").onclick = action("palette");
$("tb-help").onclick = () => help();

// Fonte: Ctrl+roda, Ctrl+=/-/0; salva por PC no localStorage e recalcula cols/rows.
const FONT0 = 13.6;
const fontKey = () => "term-font:" + (curTab()?.conn ?? LOCAL);
let font = FONT0;
function applyFont() {
  const f = +localStorage.getItem(fontKey()) || +localStorage.getItem("term-font") || FONT0;
  if (f === font && $("screen").style.fontSize) return;
  font = f;
  $("screen").style.fontSize = $("screen2").style.fontSize = font + "px";
  fit();
}
function zoom(d) {
  localStorage.setItem(fontKey(), d ? Math.max(8, Math.min(32, font + d)) : FONT0);
  applyFont();
}
$("screen").style.fontSize = $("screen2").style.fontSize = font + "px";

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

// Arrastar arquivo(s): sessão local cola o(s) caminho(s) entre aspas; remota envia ao outro PC (pasta atual da sessão, senão a padrão do host).
window.__TAURI__.event.listen("term-cwd", (e) => { dropCwds[e.payload.key] = e.payload.cwd || undefined; });
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent(async ({ payload }) => {
  if (payload.type !== "drop" || !payload.paths?.length) return;
  const t = target();
  if (!t || t.conn === LOCAL) { send(payload.paths.map((p) => `"${p}"`).join(" ")); $("screen").focus(); return; }
  for (const [i, path] of payload.paths.entries()) {
    const name = path.split(/[\/]/).pop();
    banner(`Enviando ${name} (${i + 1}/${payload.paths.length})…`);
    try { await invoke("term_upload", { conn: t.conn, path, targetDir: dropCwds[t.key] ?? null }); }
    catch (e) { return banner(`Falha ao enviar ${name}: ${msg(e)}`, "err"); }
  }
  banner(`${payload.paths.length} arquivo(s) enviado(s) para ${dropCwds[t.key] ?? "a pasta padrão do outro PC"}.`);
});

// Ajuda (F1).
const KEYS = [["Ctrl+C", "Copiar seleção (sem seleção: interrompe, ^C)"], ["Ctrl+V / Ctrl+Shift+V", "Colar"], ["Ctrl+Shift+C", "Copiar seleção"],
  ["Ctrl+Shift+F", "Buscar"], ["Enter / Shift+Enter", "Próximo / anterior na busca"], ["Ctrl+Shift+T", "Nova sessão"], ["Ctrl+Shift+W", "Fechar aba"],
  ["Ctrl+Tab / Ctrl+Shift+Tab", "Próxima / aba anterior"], ["Ctrl+Shift+B", "Mostrar/ocultar lista lateral"], ["Ctrl+Shift+F6", "Ir para a lista lateral"],
  ["Ctrl+roda do mouse", "Zoom da fonte"], ["Roda do mouse", "Rolar histórico"],
  ["Ctrl+clique", "Abrir link ou caminho"], ["Botão direito", "Colar (com seleção: copiar)"], ["Shift+botão direito / Shift+F10 / ☰", "Menu do terminal"], ["Arrastar arquivo", "Local: cola o caminho · Remoto: envia ao outro PC"]];
// Atalhos remapeáveis: ação fixa, tecla editável no F1 (clique na tecla e pressione a nova). Salvos no localStorage.
const SHORTCUTS = {
  paste: ["Colar", () => paste(), "Ctrl+V"], find: ["Buscar", () => openFind(), "Ctrl+Shift+F"], clear: ["Limpar", () => clearTab(), "Ctrl+Shift+K"],
  zin: ["Aumentar fonte", () => zoom(1), ""], zout: ["Diminuir fonte", () => zoom(-1), ""], z0: ["Redefinir fonte", () => zoom(0), ""],
  help: ["Esta ajuda", () => help(), "F1"],
};
// Atalhos antigos (Ctrl+F, Ctrl+=…) iam contra programas do terminal: descartados se salvos.
const OLD_KEYS = ["Ctrl+F", "Ctrl+=", "Ctrl+-", "Ctrl+0"];
const binds = { ...Object.fromEntries(Object.entries(SHORTCUTS).map(([a, s]) => [a, s[2]])), ...(() => { try { return Object.fromEntries(Object.entries(JSON.parse(localStorage.getItem("term-keys") || "{}")).filter(([, k]) => !OLD_KEYS.includes(k))); } catch { return {}; } })() };
function keyName(e) {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.key.length === 1 ? e.key.toUpperCase() : e.key].filter(Boolean).join("+");
}
function help() {
  const opener = document.activeElement;
  const rows = Object.entries(SHORTCUTS).flatMap(([a, [d]]) => {
    const b = el("button", "mono", binds[a]);
    b.type = "button"; b.title = "Clique e pressione a nova tecla";
    b.onclick = () => { b.textContent = "…"; b.onkeydown = (e) => {
      const n = keyName(e);
      if (!n) return;
      e.preventDefault(); e.stopPropagation();
      if (n !== "Escape") { binds[a] = n; localStorage.setItem("term-keys", JSON.stringify(binds)); }
      b.textContent = binds[a]; b.onkeydown = null;
    }; };
    return [el("dt", null, b), el("dd", null, d)];
  });
  $("help-list").replaceChildren(...rows, ...KEYS.flatMap(([k, d]) => [el("dt", "mono", k), el("dd", null, d)]));
  $("help-scrim").hidden = false;
  const end = () => { $("help-scrim").hidden = true; opener?.focus(); };
  $("help-ok").onclick = end;
  trap($("help"), end);
  $("help-ok").focus();
}

// ---------- Tamanho ----------
// Os dois painéis têm a mesma largura: o tamanho medido em #screen vale para ambos.
function resizeActive() {
  const t = activeTab();
  if (!t) return;
  const { cols, rows } = size();
  if (t.exited == null) invoke("term_resize", { conn: t.conn, sessionId: t.sid, cols, rows }).catch(() => {});
  if (t.split) invoke("term_resize", { conn: t.split.conn, sessionId: t.split.sid, cols, rows }).catch(() => {});
}
let resizeTimer = 0;
function fit() {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(resizeActive, 100);
}
new ResizeObserver(fit).observe($("screen"));

// ---------- Início ----------
$("local-new").onclick = () => profileMenu($("local-new"), LOCAL);
$("empty-new").onclick = () => newSession(LOCAL);
// Retomar: abre em abas todas as sessões locais vivas (inclusive as restauradas do disco).
refresh(LOCAL).then(async () => {
  for (const s of conns[LOCAL].sessions || []) if (s.state !== "closed") await openSession(LOCAL, s).catch(() => {});
});
refreshRemotes();
// Sessões criadas pelo celular aparecem sozinhas; não redesenha a lista sob o foco do teclado.
// ponytail: polling de 5 s; trocar por evento do host quando C3 tiver aviso de sessão nova.
setInterval(() => { if (!document.hidden && !$("sidebar").contains(document.activeElement)) refresh(LOCAL); }, 5000);
renderTabs();
updateBanner();

// Item 20: aviso de atualização (mesmos comandos da janela principal).
invoke("check_update").then((v) => {
  if (!v) return;
  $("upd-t").textContent = `Versão ${v} disponível.`;
  $("upd-banner").hidden = false;
}).catch(() => {});
$("upd-x").onclick = () => { $("upd-banner").hidden = true; };
$("upd-go").onclick = async () => {
  $("upd-go").disabled = true;
  $("upd-t").textContent = "Baixando e instalando…";
  try { await invoke("install_update"); } catch { $("upd-go").disabled = false; $("upd-t").textContent = "Não foi possível atualizar. Tente de novo."; }
};

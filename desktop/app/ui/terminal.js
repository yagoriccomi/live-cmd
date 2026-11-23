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
  SCP_FAILED: "Não deu para enviar o print: o servidor precisa aceitar sua chave SSH (sem senha).",
  SCP_MISSING: "scp.exe não encontrado (instale o Cliente OpenSSH do Windows).",
  NOT_CONNECTED: "Sem conexão com este PC.",
  IO_ERROR: "Não foi possível gravar os dados do app.",
  CLIPBOARD: "Não foi possível usar a área de transferência.",
};
const msg = (code) => MSG[code] || "Algo deu errado. Tente de novo.";

// #44: badges manuais (salvas no host junto do layout, celular lê o mesmo `layout.json`).
const BADGES = { wait: ["⏳", "Em espera"], lock: ["🔒", "Travado"], me: ["❗", "Precisa de mim"], run: ["▶", "Em andamento"], done: ["✓", "Concluído"], err: ["✕", "Erro"], fav: ["★", "Favorito"] };
const conns = { [LOCAL]: { name: "Este PC", status: { state: "ready" }, sessions: [], profiles: [] } };
let remoteOrder = [];
const tabs = [];       // { key, conn, sid, label, title, exited, split?, max? }  split = registro igual ao da aba (painel direito/inferior); max = 0|1 painel maximizado
let active = null;     // key
let appCursor = false;
const screens = {};    // key → última tela recebida
let adding = null;     // id do PC sendo adicionado
const busy = new Set();   // chaves com saída nova fora de vista (item 8)
const closed = [];        // abas fechadas com sessão viva, para reabrir (item 7)
const lastEnter = {};     // chave → hora do último Enter (item 9)

// ---------- Medida da célula e tamanho ----------
// Mede a célula no #screen e guarda: com o painel esquerdo oculto (maximizado o direito) vale a última medida.
let cellW = 8;
function size(s = $("screen")) {
  const m = $("measure").getBoundingClientRect(), cs = getComputedStyle(s);
  if (m.width) cellW = m.width / 10;
  const w = s.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const h = s.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const lh = parseFloat(cs.lineHeight) || m.height || 16;
  return { cols: Math.max(20, Math.min(1000, Math.floor(w / cellW))), rows: Math.max(5, Math.min(1000, Math.floor(h / lh))) };
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
  if (s === pane()) afterDraw();
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
  const t = target();
  if (!t) return banner("");
  if (t.exited != null) return banner(`A sessão terminou (código ${t.exited}).`, "warn", [["Nova sessão", () => newSession(t.conn)], activeTab().split ? ["Fechar painel", () => closePane(focusPane)] : ["Fechar aba", () => closeTab(t.key)]]);
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
// Ordem das abas e aba ativa sobrevivem a fechar/atualizar o app; não grava durante a reabertura inicial.
let restoring = false;
function saveTabs() {
  if (restoring) return;
  try { localStorage.setItem("tabs-state", JSON.stringify({ order: tabs.map((t) => t.key), active, splits: Object.fromEntries(tabs.filter((t) => t.split).map((t) => [t.key, t.split.key])) })); } catch {}
}
function renderTabs() {
  saveTabs();
  paintSplit();
  const bar = $("tabs");
  if (bar.querySelector("input.rename")) { tabsLater = true; return; }
  bar.replaceChildren(...tabs.map((t) => {
    const b = el("button");
    b.type = "button";
    b.setAttribute("role", "tab");
    b.setAttribute("aria-selected", String(t.key === active));
    b.tabIndex = t.key === active ? 0 : -1;
    b.dataset.key = t.key;
    // Aba remota: só a cor do PC (nome do PC fica no tooltip e no leitor de tela).
    if (t.conn !== LOCAL) b.append(pcMark(t.conn));
    // Tooltip host · nome em todas as guias (#42); no dividido, os dois na ordem dos painéis.
    const tip = (r) => r.conn === LOCAL ? "Este PC · " + (r.label || "Terminal") : r.title;
    b.title = t.split ? tip(t) + " | " + tip(t.split) : tip(t);
    const bi = (r) => BADGES[layouts[r.conn]?.badge?.[r.sid]]?.[0], bt = [t, t.split].filter(Boolean).map(bi).filter(Boolean).join("");
  const name = el("span", "tab-name", (bt ? bt + " " : "") + (t.label || "Terminal") + (t.split ? " | " + (t.split.label || "Terminal") : ""));
    if (t.conn !== LOCAL) name.setAttribute("aria-label", t.title);
    name.ondblclick = (e) => { e.stopPropagation(); renameSession(t.conn, t.sid, name, t.label); };
    b.append(name);
    if (busy.has(t.key)) b.append(actDot());
    b.draggable = true;
    b.ondragstart = (e) => e.dataTransfer.setData("text/x-tab", t.key);
    b.ondragover = (e) => e.preventDefault();
    b.ondrop = (e) => { e.preventDefault(); e.stopPropagation(); moveTab(e.dataTransfer.getData("text/x-tab"), t.key, e.clientX); };
    b.oncontextmenu = (e) => { e.preventDefault(); menu(b, tabMenu(t, name)); };
    const x = el("span", "x", "×");
    x.setAttribute("aria-hidden", "true");
    x.title = "Fechar aba (Ctrl+Shift+W)";
    x.onclick = (e) => { e.stopPropagation(); closeTab(t.key, true); };
    b.append(x);
    b.onclick = () => { if (t.key !== active) activate(t.key); };
    // Botão do meio: sem preventDefault no mousedown o WebView2 entra em autoscroll e o auxclick some.
    b.onmousedown = (e) => { if (e.button === 1) e.preventDefault(); };
    b.onauxclick = (e) => { if (e.button === 1) { e.preventDefault(); closeTab(t.key, true); } };
    return b;
  }));
  $("empty").hidden = tabs.length > 0;
  const t = tabs.find((t) => t.key === active);
  $("screen").setAttribute("aria-label", t ? "Terminal " + t.title : "Terminal");
  renderSidebar();
}

/** Reordena (#27): solta antes ou depois da guia alvo conforme a metade; sem alvo, vai para o fim. Só a ordem das guias muda. */
function moveTab(from, to, x) {
  const i = tabs.findIndex((t) => t.key === from);
  if (i < 0 || from === to) return;
  const [m] = tabs.splice(i, 1);
  const b = to && $("tabs").querySelector(`[data-key="${CSS.escape(to)}"]`);
  let j = to ? tabs.findIndex((t) => t.key === to) : -1;
  if (j < 0) j = tabs.length;
  else if (b && x != null) { const r = b.getBoundingClientRect(); if (x > r.left + r.width / 2) j++; }
  tabs.splice(j, 0, m);
  renderTabs();
}
$("tabs").addEventListener("dragover", (e) => e.preventDefault());
$("tabs").addEventListener("drop", (e) => { const k = e.dataTransfer.getData("text/x-tab"); if (k) { e.preventDefault(); moveTab(k, null); } });
// Roda do mouse rola as guias na horizontal (#43).
$("tabs").addEventListener("wheel", (e) => { if (!e.deltaY) return; e.preventDefault(); $("tabs").scrollLeft += e.deltaY; }, { passive: false });

function tabMenu(t, name) {
  return [["Renomear", () => renameSession(t.conn, t.sid, name, t.label)],
    t.split ? ["Desfazer divisão", () => unsplit(t)] : ["Dividir", () => splitMenu(name)],
    ...(t.split ? [["Inverter lados", swapSides], ["Alternar orientação", toggleOrient], [t.max != null ? "Restaurar painéis" : "Maximizar painel", toggleMax]] : []),
    ["Reabrir aba fechada (Ctrl+Alt+T)", reopen],
    ["Fechar aba", () => closeTab(t.key, true)]];
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
  if (!span.isConnected) span = $("tabs").querySelector(`[data-key="${CSS.escape(conn + "/" + sid)}"] .tab-name`) || document.querySelector(`.pane-head[data-key="${CSS.escape(conn + "/" + sid)}"] .pane-title`);
  if (!span) return;
  editInline(span, cur, async (v) => {
    await invoke("term_rename", { conn, sessionId: sid, label: v });
    layoutOf(conn).named[sid] = true; saveSideOrder(conn); // nome manual não segue mais o `cd`
    for (const t of allRecs()) if (t.conn === conn && t.sid === sid) { t.label = v; t.title = titleOf(conn, { label: v }); window.histMeta?.(conn, { session_id: sid, label: v }); }
    await refresh(conn);
    renderTabs();
  });
}

function renamePc(id, span) {
  editInline(span, conns[id].name, async (v) => {
    await invoke("remote_rename", { id, name: v });
    await refreshRemotes();
    for (const t of allRecs()) if (t.conn === id) t.title = titleOf(id, { label: t.label });
    renderTabs();
  });
}

// ---------- Item 5: divisão (uma por aba, dois painéis: lado a lado ou um sobre o outro) ----------
const activeTab = () => tabs.find((t) => t.key === active);
/** Abas e painéis divididos, todos como registros { key, conn, sid, label, title, exited }. */
const allRecs = () => tabs.flatMap((t) => t.split ? [t, t.split] : [t]);
let focusPane = 0;         // 0 = esquerdo/superior, 1 = direito/inferior (só vale com divisão)
const paneAct = new Set(); // painel visível com saída nova enquanto o outro tem o foco
const pane = () => focusPane === 1 && activeTab()?.split ? $("screen2") : $("screen");
/** Sessão que recebe teclado, colar, arrastar, busca e exportar: a do painel focado. */
function target() {
  const t = activeTab();
  return t && focusPane === 1 && t.split ? t.split : t;
}

// Divisor: proporção e orientação ficam por aparelho (localStorage).
const layout = { stack: false, pct: 50, ...(() => { try { const v = JSON.parse(localStorage.getItem("split-layout") || "{}"); return { stack: !!v.stack, pct: Math.max(10, Math.min(90, +v.pct || 50)) }; } catch { return {}; } })() };
const saveLayout = () => { try { localStorage.setItem("split-layout", JSON.stringify(layout)); } catch {} };
const MIN_PANE = { w: 180, h: 140 }; // px mínimos de cada painel

function setPct(p, save = false) {
  const r = $("term-area").getBoundingClientRect(), len = layout.stack ? r.height : r.width;
  const min = len > 0 ? Math.min(45, (layout.stack ? MIN_PANE.h : MIN_PANE.w) / len * 100) : 10;
  layout.pct = Math.max(min, Math.min(100 - min, p));
  $("term-area").style.setProperty("--sp", layout.pct + "%");
  $("sash").setAttribute("aria-valuenow", String(Math.round(layout.pct)));
  if (save) saveLayout();
  fit();
}

/** Estado visual da divisão: classes, títulos, foco e botões. Chamado a cada renderTabs. */
function paintSplit() {
  const t = activeTab(), area = $("term-area"), sp = t?.split;
  if (!sp) { if (t) t.max = null; focusPane = 0; paneAct.clear(); }
  else if (t.max === 0) focusPane = 0;
  else if (t.max === 1) focusPane = 1;
  paneAct.delete(target()?.key);
  area.classList.toggle("split", !!sp);
  area.classList.toggle("stack", !!sp && layout.stack);
  area.classList.toggle("max1", !!sp && t.max === 0);
  area.classList.toggle("max2", !!sp && t.max === 1);
  area.style.setProperty("--sp", layout.pct + "%");
  $("screen2").hidden = !sp;
  $("screen").classList.toggle("focused", !!sp && focusPane === 0);
  $("screen2").classList.toggle("focused", !!sp && focusPane === 1);
  const sash = $("sash");
  sash.setAttribute("aria-orientation", layout.stack ? "horizontal" : "vertical");
  sash.setAttribute("aria-valuenow", String(Math.round(layout.pct)));
  $("screen2").setAttribute("aria-label", sp ? "Terminal dividido " + sp.title : "Terminal dividido");
  [t, sp].forEach((rec, i) => {
    const h = $("head" + (i + 1));
    h.classList.toggle("focused", !!sp && focusPane === i);
    if (!rec || !sp) return;
    h.dataset.key = rec.key;
    const title = h.querySelector(".pane-title");
    if (title) title.textContent = (rec.title || rec.label || "Terminal") + (rec.exited != null ? " (encerrada)" : "");
    h.querySelector(".pane-act").hidden = !paneAct.has(rec.key);
    const o = h.querySelector("[data-act=orient]");
    o.textContent = layout.stack ? "⇄" : "⇅";
    o.title = layout.stack ? "Lado a lado" : "Um sobre o outro";
    o.setAttribute("aria-label", o.title);
    const mx = h.querySelector("[data-act=max]");
    mx.textContent = t.max === i ? "⤡" : "⤢";
    mx.setAttribute("aria-pressed", String(t.max === i));
    mx.title = t.max === i ? "Restaurar painéis" : "Maximizar painel";
    mx.setAttribute("aria-label", mx.title);
  });
}

function setFocusPane(n, dom = true) {
  const t = activeTab();
  const next = n === 1 && t?.split && t.max !== 0 ? 1 : 0;
  const same = next === focusPane;
  focusPane = next;
  paintSplit();
  if (dom) pane().focus();
  if (same) return;
  appCursor = !!screens[target()?.key]?.app_cursor;
  updateBanner();
  afterDraw();
}

function toggleOrient() {
  layout.stack = !layout.stack;
  saveLayout();
  paintSplit();
  setPct(layout.pct);
}
function toggleMax() {
  const t = activeTab();
  if (!t?.split) return;
  t.max = t.max == null ? focusPane : null;
  paintSplit();
  fit();
  pane().focus();
}

/** Inverte os painéis (#28): mesmos PTYs, foco e proporção seguem o terminal; a guia passa a ter o outro como dono. */
function swapSides() {
  const t = activeTab(), sp = t?.split;
  if (!sp) return;
  tabs[tabs.indexOf(t)] = sp;
  sp.split = t;
  t.split = null;
  sp.max = t.max == null ? null : 1 - t.max;
  t.max = null;
  active = sp.key;
  focusPane = 1 - focusPane;
  layout.pct = 100 - layout.pct;
  saveLayout();
  renderTabs();
  draw(sp.key);
  draw(t.key, $("screen2"));
  setPct(layout.pct);
  pane().focus();
}

/** Menu para dividir: terminal novo ou outra sessão aberta (se já é aba, as duas viram uma só). */
function splitMenu(anchor) {
  const t = activeTab();
  if (!t) return;
  const opts = [["Terminal novo   Ctrl+Shift+D", splitNew]];
  for (const [conn, c] of Object.entries(conns)) for (const s of c.sessions || []) {
    const key = conn + "/" + s.session_id;
    if (s.state !== "exited" && key !== t.key && key !== t.split?.key) opts.push([titleOf(conn, s), () => split(t, conn, s.session_id)]);
  }
  menu(anchor || $("tabs"), opts);
}

async function splitNew() {
  const t = activeTab();
  if (!t) return newSession(LOCAL);
  const conn = target().conn, p = conns[conn]?.profiles[0];
  if (!p) return banner(msg("NOT_FOUND"), "err");
  const { cols, rows } = size(pane());
  try {
    const sid = await invoke("term_create", { conn, profileId: p.profile_id, label: p.label, cols, rows });
    await split(t, conn, sid, { session_id: sid, label: p.label, state: "running" });
    refresh(conn);
  } catch (e) { banner(msg(e), "err"); }
}

async function split(t, conn, sid, meta) {
  const key = conn + "/" + sid;
  if (key === t.key || key === t.split?.key) return;
  let rec = tabs.find((x) => x.key === key);
  if (rec) {
    // A sessão já era aba: vira o painel desta, sem aba duplicada.
    if (rec.split) unsplit(rec, false);
    tabs.splice(tabs.indexOf(rec), 1);
    rec.max = null;
  } else {
    const { cols, rows } = size(pane());
    let ok;
    try { ok = await invoke("term_attach", { conn, sessionId: sid, cols, rows }); } catch (e) { return banner(msg(e), "err"); }
    const s = conns[conn]?.sessions.find((s) => s.session_id === sid) || meta || { session_id: sid };
    window.histMeta?.(conn, s);
    rec = { key, conn, sid, label: s.label, title: titleOf(conn, s), exited: ok?.state === "exited" ? ok.exit_code ?? 0 : null };
  }
  if (t.split) unsplit(t, false);
  t.split = rec;
  t.max = null;
  active = t.key;
  focusPane = 1;
  activate(t.key);
}

/** Desfaz a divisão: a sessão do segundo painel passa a ser uma aba normal, logo depois desta. */
function unsplit(t, redraw = true) {
  const sp = t.split;
  if (!sp) return;
  t.split = null;
  t.max = null;
  paneAct.delete(sp.key);
  if (!tabs.some((x) => x.key === sp.key)) tabs.splice(tabs.indexOf(t) + 1, 0, sp);
  focusPane = 0;
  if (redraw) activate(t.key);
}

/** Solta a visão de uma sessão (desanexa; o PTY segue vivo no host) e guarda para reabrir. */
function release(r) {
  if (r.exited == null) closed.push({ conn: r.conn, sid: r.sid });
  if (closed.length > 20) closed.shift();
  busy.delete(r.key);
  paneAct.delete(r.key);
  delete screens[r.key];
  invoke("term_detach", { conn: r.conn, sessionId: r.sid }).catch(() => {});
}

/** Fecha só o painel n da aba ativa; a outra sessão continua como aba normal. */
function closePane(n) {
  const t = activeTab(), sp = t?.split;
  if (!sp) return;
  t.split = null;
  t.max = null;
  focusPane = 0;
  if (n === 1) {
    release(sp);
    return activate(t.key);
  }
  tabs[tabs.indexOf(t)] = sp;
  release(t);
  activate(sp.key);
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
  if (key !== active) focusPane = 0;
  active = key;
  const t = tabs.find((t) => t.key === key);
  busy.delete(key);
  if (t?.split) busy.delete(t.split.key);
  renderTabs();
  draw(key);
  if (t?.split) draw(t.split.key, $("screen2"));
  updateBanner();
  applyFont();
  resizeActive();
  if (focus) pane().focus();
}

function titleOf(conn, s) {
  const label = s.label || "Terminal";
  return conn === LOCAL ? label : `${conns[conn].name} · ${label}`;
}

async function openSession(conn, s) {
  const key = conn + "/" + s.session_id;
  // Sessão que já é painel dividido de uma aba: vai para essa aba, sem aba duplicada.
  const host = tabs.find((t) => t.split?.key === key);
  if (host) { activate(host.key); return setFocusPane(1); }
  if (!tabs.some((t) => t.key === key)) {
    const { cols, rows } = size();
    let ok;
    try { ok = await invoke("term_attach", { conn, sessionId: s.session_id, cols, rows }); }
    catch (e) { return banner(msg(e), "err"); }
    tabs.push({ key, conn, sid: s.session_id, label: s.label, title: titleOf(conn, s), exited: ok.state === "exited" ? ok.exit_code ?? 0 : null });
    window.histMeta?.(conn, s);
  }
  activate(key);
}

async function closeTab(key, ask = false) {
  const i = tabs.findIndex((t) => t.key === key);
  if (i < 0) return;
  // Fechar a guia não encerra o terminal (#31): só desanexa; a sessão segue na lista.
  if (ask && !confirm("Fechar a guia? O terminal continua rodando e segue na lista de sessões.")) return;
  const [t] = tabs.splice(i, 1);
  // Fechar a aba solta as duas sessões; para fechar só uma, use o painel (closePane).
  if (t.split) release(t.split);
  release(t);
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
// Destino SSH da aba local (`{user, host, port}` vindo do host), ou null.
const sshOf = (t) => t?.conn === LOCAL ? conns[LOCAL].sessions.find((s) => s.session_id === t.sid)?.ssh ?? null : null;
const sshGit = {}; // git da pasta remota por aba SSH (ssh_git)

// ---------- Lista lateral ----------
async function refresh(conn) {
  try {
    const v = await invoke("term_list", { conn });
    conns[conn].sessions = v.sessions;
    conns[conn].profiles = v.profiles;
  } catch { conns[conn].sessions = []; }
  renderSidebar();
  // Ordem/grupos chegam depois: host lento não segura a lista de sessões.
  if (!layouts[conn]) {
    layouts[conn] = {};
    layouts[conn] = await invoke("term_layout_get", { conn }).catch(() => ({})) || {};
    renderSidebar();
  }
}

async function refreshRemotes() {
  const v = await invoke("term_connections").catch(() => ({ remotes: [] }));
  remoteOrder = v.remotes.map((r) => r.id);
  for (const r of v.remotes) conns[r.id] = { sessions: [], profiles: [], ...conns[r.id], name: r.name, status: r.status };
  renderSidebar();
}

// Git da aba em foco (branch/pendências) a cada 15 s.
setInterval(() => { const c = active?.split("/")[0]; if (c && conns[c] && !document.hidden) refresh(c); }, 15000);
// Aba SSH em foco: git roda no servidor pelo próprio ssh (só com pasta conhecida pelo OSC 7).
setInterval(async () => {
  const t = activeTab(), ssh = sshOf(t), cwd = t && dropCwds[t.key];
  if (!ssh || !cwd || document.hidden) return;
  const g = await invoke("ssh_git", { target: ssh, cwd }).catch(() => null);
  if (JSON.stringify(g) !== JSON.stringify(sshGit[t.key] ?? null)) { sshGit[t.key] = g; renderSidebar(); }
}, 15000);

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

// Tipo da sessão só para o visual (ícone e filtro da lista): agente, SSH ou shell, pelo rótulo.
const KIND_ICON = { agent: "✦", ssh: "⇄", shell: "›_" };
// Ícone redondo do agente (#40, #41): logo empacotado, senão imagem enviada ao host, senão a inicial.
// ponytail: sem logos oficiais no repo; ponha o arquivo em ui/ e o nome aqui (ex.: claude: "claude.png").
const AGENT_LOGO = {};
const agentIcons = new Map(); // "conn|agente" -> Promise<data URL | null>
function agentIcon(conn, agent) {
  const i = el("span", "ag-ico", (agent || "?")[0].toUpperCase());
  i.setAttribute("aria-hidden", "true");
  const k = conn + "|" + agent;
  if (!agentIcons.has(k)) agentIcons.set(k, AGENT_LOGO[agent] ? Promise.resolve(AGENT_LOGO[agent]) : invoke("agent_icon_get", { conn, agent }).catch(() => null));
  agentIcons.get(k).then((u) => { if (u) { i.textContent = ""; i.style.backgroundImage = `url("${u}")`; } });
  return i;
}
const sessionKind = (label = "") => /\b(claude|openclaude|codex)\b/i.test(label) ? "agent" : /\bssh\b|@/i.test(label) ? "ssh" : "shell";
let sideFilter = "";
function sessionItem(conn, s) {
  const li = el("li"), b = el("button");
  b.type = "button";
  const key = conn + "/" + s.session_id;
  b.dataset.key = key; // F2/Delete fora do terminal
  if (key === active) b.setAttribute("aria-current", "true");
  // #11: nome = pasta atual (segue o `cd`), salvo se renomeado à mão.
  const cwd = dropCwds[key] || s.cwd;
  const auto = !layoutOf(conn).named[s.session_id] && baseName(cwd);
  const name = el("span", "card-t", auto || (s.label || "Terminal").replace(/\\{2,}/g, "\\")); // host remoto manda barras escapadas
  name.ondblclick = (e) => { e.stopPropagation(); renameSession(conn, s.session_id, name); };
  // Visual V1 Aurora: ícone pelo tipo, pasta atual (OSC 7) e estado por extenso.
  const kind = sessionKind(s.label);
  li.dataset.kind = kind;
  const txt = el("span", "card-txt");
  txt.append(name, el("span", "card-s mono", cwd || (s.state === "exited" ? "sessão encerrada" : "")));
  const git = s.ssh ? sshGit[key] : s.git;
  // Aba em foco: dados do projeto, clique copia.
  if (key === active) {
    const chips = el("span", "card-chips");
    const chip = (text, val, title) => {
      const c = el("span", "chip mono", text);
      c.title = title + " (clique copia)";
      c.onclick = (e) => { e.stopPropagation(); invoke("clip_copy", { text: String(val) }).then(() => banner("Copiado: " + val), (err) => banner(msg(err), "err")); };
      chips.append(c);
    };
    if (cwd) chip("📁 pasta", cwd, cwd);
    if (git) {
      chip("⎇ " + (git.branch || "HEAD solto"), git.branch, "Branch atual");
      chip(git.dirty ? `● ${git.dirty} pendente${git.dirty > 1 ? "s" : ""}` : "✓ limpo", git.dirty ? `${git.dirty} arquivo(s) sem commit` : "sem pendências", "Pendências de commit");
    }
    const ms = Number(s.created_at_ms);
    if (ms) { const d = new Date(ms).toLocaleString("pt-BR"); chip("🕒 " + d, d, "Criado em"); }
    txt.append(chips);
  }
  // #44: badge manual no lugar de "● ativo"; online/offline fica no cabeçalho do host (#46).
  const bd = BADGES[layoutOf(conn).badge[s.session_id]];
  const st = el("span", "card-st" + (s.state === "exited" ? " muted" : ""), s.state === "exited" ? "● parado" : bd ? bd[0] : "");
  if (bd) st.title = bd[1];
  const tile = el("span", "tile", kind === "agent" ? null : KIND_ICON[kind]);
  if (kind === "agent") tile.append(agentIcon(conn, (s.label.match(/\b(openclaude|claude|codex)\b/i) || [, "?"])[1].toLowerCase()));
  b.append(tile, txt, st);
  b.firstChild.setAttribute("aria-hidden", "true");
  if (busy.has(key)) b.append(actDot());
  b.onclick = () => openSession(conn, s);
  b.oncontextmenu = (e) => { e.preventDefault(); menu(b, cardMenu(conn, s, name, b, cwd, git)); };
  b.onkeydown = (e) => {
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); b.oncontextmenu(e); }
    else if (e.key === "F2") { e.preventDefault(); renameSession(conn, s.session_id, name); }
    else if (e.key === "Delete") { e.preventDefault(); endSession(conn, s); }
  };
  li.append(b);
  li.draggable = true;
  li.ondragstart = (e) => { e.dataTransfer.setData("text/plain", s.session_id); dragFrom = conn; li.classList.add("dragging"); };
  li.ondragend = () => li.classList.remove("dragging");
  dropTarget(li, conn, () => groupKey(conn, s), s.session_id);
  return li;
}

// Menu do card (B9, #35): barra Renomear/Dividir/Explorer/Fixar; Projeto, Organizar, Perigo.
function cardMenu(conn, s, name, b, cwd, git) {
  const L = layoutOf(conn), g = groupKey(conn, s), copy = (v) => invoke("clip_copy", { text: v }).then(() => banner("Copiado: " + v), (err) => banner(msg(err), "err"));
  // ponytail: Explorer só para sessões deste PC (caminho remoto não existe aqui).
  const explorer = () => conn === LOCAL && cwd ? window.__TAURI__.opener.revealItemInDir(cwd).catch(() => banner(msg("NOT_FOUND"), "err")) : banner("Abrir no Explorer só vale para sessões deste PC com pasta conhecida.", "warn");
  const groups = [...new Set(conns[conn].sessions.map((x) => groupKey(conn, x)))].filter((x) => x !== g);
  const move = (to) => { delete L.group[s.session_id]; if (groupKey(conn, s) !== to) L.group[s.session_id] = to; saveSideOrder(conn); renderSidebar(); };
  return [
    { row: [["✎", "Renomear", () => renameSession(conn, s.session_id, name), "F2"], ["◫", "Dividir", () => splitCard(conn, s)], ["📁", "Abrir no Explorer", explorer],
      ["📌", L.pinned === g ? "Desafixar grupo" : "Fixar no topo", () => { L.pinned = L.pinned === g ? null : g; saveSideOrder(conn); renderSidebar(); }]] },
    "#Projeto",
    ["Copiar caminho", cwd && (() => copy(cwd)), "", "⧉"],
    ["Copiar nome da branch", git?.branch && (() => copy(git.branch)), "", "⎇"],
    ["Iniciar agente aqui", () => newAi(conn, cwd), "", "🤖"],
    "#Badge",
    ...Object.entries({ "": ["", "Nenhuma"], ...BADGES }).map(([k, [ic, lb]]) => [(L.badge[s.session_id] || "") === k ? "✓ " + lb : lb, () => { if (k) L.badge[s.session_id] = k; else delete L.badge[s.session_id]; saveSideOrder(conn); renderSidebar(); renderTabs(); }, "", ic]),
    "#Organizar",
    ["Mover para grupo…", () => { let n = 1; while (Object.values(L.group).includes("Grupo " + n)) n++; menu(b, [...groups.map((x) => [baseName(x) || x, () => move(x)]), ["Novo grupo", () => move("Grupo " + n)]]); }, "", "▤"],
    "#Perigo",
    ["Encerrar sessão", () => endSession(conn, s), "Del", "✕", "danger"],
  ].filter((it) => typeof it === "string" || it.row || it[1]);
}
// #36: a sessão do card vai para o segundo painel da guia atual; a que estava lá vira guia separada (unsplit em split()).
function splitCard(conn, s) {
  const t = activeTab();
  if (!t || t.key === conn + "/" + s.session_id) return openSession(conn, s);
  split(t, conn, s.session_id, s);
}

// ---------- #9/#10/#37: grupos, arraste e ordem (salvos no host: `layout_get`/`layout_set`) ----------
// Layout por conexão: { order: [sid], group: {sid: grupo manual}, named: {sid: true}, badge: {sid: chave}, pinned: grupo }.
const layouts = {};
let dragFrom = null;
const baseName = (p) => (p || "").replace(/\\{2,}/g, "\\").split(/[\\/]/).filter(Boolean).pop() || "";
function layoutOf(conn) {
  const L = layouts[conn] ||= {};
  L.order ||= []; L.group ||= {}; L.named ||= {}; L.badge ||= {};
  return L;
}
function saveSideOrder(conn) {
  invoke("term_layout_set", { conn, layout: layoutOf(conn) }).catch((e) => banner(msg(e), "err"));
}
// Manual vence; senão raiz git; senão pasta atual.
function groupKey(conn, s) {
  const k = conn + "/" + s.session_id, git = s.ssh ? sshGit[k] : s.git;
  return layoutOf(conn).group[s.session_id] || git?.root || dropCwds[k] || s.cwd || "Sem pasta";
}
// Solta antes de `before` (sid) ou no fim; muda de grupo se o destino for outro.
function dropTarget(node, conn, key, before) {
  node.ondragover = (e) => { if (dragFrom === conn) { e.preventDefault(); e.stopPropagation(); node.classList.add("drop"); } };
  node.ondragleave = () => node.classList.remove("drop");
  node.ondrop = (e) => {
    e.preventDefault(); e.stopPropagation(); node.classList.remove("drop");
    const sid = e.dataTransfer.getData("text/plain"), L = layoutOf(conn);
    const s = conns[conn].sessions.find((x) => x.session_id === sid);
    if (!s || sid === before) return;
    const g = key();
    delete L.group[sid];
    if (groupKey(conn, s) !== g) L.group[sid] = g;
    const ids = sorted(conn, conns[conn].sessions).map((x) => x.session_id).filter((x) => x !== sid);
    const at = before ? ids.indexOf(before) : -1;
    ids.splice(at < 0 ? ids.length : at, 0, sid);
    L.order = ids;
    saveSideOrder(conn);
    renderSidebar();
  };
}
function sorted(conn, list) {
  const o = layoutOf(conn).order, i = (s) => { const n = o.indexOf(s.session_id); return n < 0 ? 1e9 : n; };
  return [...list].sort((a, b) => i(a) - i(b));
}
// Cards agrupados; grupo fixo no topo; zona final cria grupo manual.
function groupedItems(conn, list) {
  const L = layoutOf(conn), groups = new Map();
  for (const s of sorted(conn, list)) { const g = groupKey(conn, s); groups.set(g, [...(groups.get(g) || []), s]); }
  const keys = [...groups.keys()].sort((a, b) => (b === L.pinned) - (a === L.pinned));
  const out = keys.map((g) => {
    const li = el("li", "sess-group"), h = el("button", "group-h small", (g === L.pinned ? "📌 " : "") + (baseName(g) || g)), ul = el("ul", "cards");
    h.type = "button"; h.title = g;
    h.oncontextmenu = (e) => { e.preventDefault(); menu(h, [[L.pinned === g ? "Desafixar" : "Fixar no topo", () => { L.pinned = L.pinned === g ? null : g; saveSideOrder(conn); renderSidebar(); }]]); };
    h.onkeydown = (e) => { if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); h.oncontextmenu(e); } };
    dropTarget(h, conn, () => g, null);
    ul.append(...groups.get(g).map((s) => sessionItem(conn, s)));
    li.append(h, ul);
    return li;
  });
  if (list.length > 1) {
    let n = 1;
    while (Object.values(L.group).includes("Grupo " + n)) n++;
    const z = el("li", "group-new muted small", "Solte aqui para criar um grupo");
    dropTarget(z, conn, () => "Grupo " + n, null);
    out.push(z);
  }
  return out;
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
  const shown = conns[LOCAL].sessions.filter((s) => !sideFilter || (s.ssh ? "ssh" : sessionKind(s.label)) === sideFilter);
  $("local-ul").replaceChildren(...groupedItems(LOCAL, shown.filter((s) => !s.ssh)));
  // Categoria SSH: um card por servidor (user@host:porta), abas embaixo, como os outros PCs.
  const servers = new Map();
  for (const s of shown.filter((s) => s.ssh)) {
    const id = `${s.ssh.user}@${s.ssh.host}:${s.ssh.port}`;
    servers.set(id, [...(servers.get(id) || []), s]);
  }
  $("h-ssh").hidden = !servers.size;
  $("ssh-ul").replaceChildren(...[...servers].map(([id, list]) => {
    const li = el("li"), b = el("button"), ul = el("ul");
    b.type = "button";
    b.append(el("span", "dot-ok", "⇄"), el("span", null, list[0].label || id), el("span", "muted small conn-q", id));
    b.onclick = () => openSession(LOCAL, list[0]);
    ul.append(...list.map((s) => sessionItem(LOCAL, s)));
    li.append(b, ul);
    return li;
  }));
  window.auroraTerm?.();
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
      ul.append(...groupedItems(id, c.sessions));
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
  // Item: [texto, fn, atalho?, ícone?, classe?]; "#Rótulo" = seção; { row: [[ícone, texto, fn, atalho?]] } = barra rápida (#32).
  const btn = (label, fn, key, icon, cls) => {
    const b = el("button", cls || null);
    b.type = "button";
    b.setAttribute("role", "menuitem");
    if (icon != null) b.append(el("span", "m-ic", icon));
    if (label) b.append(el("span", "m-t", label));
    if (key) b.append(el("span", "m-k", key));
    b.onclick = () => { closeMenu(false); fn(); };
    return b;
  };
  m.replaceChildren(...items.map((it) => {
    if (typeof it === "string") return el("p", "m-lbl", it.slice(1));
    if (it.row) {
      const r = el("div", "m-row");
      r.append(...it.row.map(([icon, label, fn, key]) => { const b = btn("", fn, null, icon); b.title = label + (key ? "  " + key : ""); b.setAttribute("aria-label", label); return b; }));
      return r;
    }
    const [label, fn, key, icon, cls] = it;
    if (!fn) return el("p", "muted small", label);
    return icon == null && !key ? btn(label, fn) : btn(label, fn, key, icon, cls);
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

// Agente de IA: newAi(conn) mora em agents.js (formulário do ticket 019).

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
  for (const t of tabs.filter((t) => t.conn !== id && t.split?.conn === id)) { release(t.split); t.split = null; t.max = null; }
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
  if (k === active || t?.split?.key === k) {
    const s = k === active ? $("screen") : $("screen2");
    if (!holdDraw(s)) drawSoon(k, s);
    if (t.split && target().key !== k && !paneAct.has(k)) { paneAct.add(k); paintSplit(); }
  }
  else if (!busy.has(k)) { busy.add(k); renderTabs(); }
});
// Item 9 (evento do F1): avisa no Windows se o comando levou mais de 10 s e não estava à vista.
listen("term-cmd-done", ({ payload }) => {
  const seen = document.hasFocus() && payload.key === target()?.key;
  const start = lastEnter[payload.key];
  delete lastEnter[payload.key];
  if (seen || !start || Date.now() - start < 10000) return;
  const tab = allRecs().find((x) => x.key === payload.key);
  invoke("plugin:notification|notify", { options: { title: "Comando terminou", body: `${tab?.title || "Terminal"}: código ${payload.code ?? 0}` } }).catch(() => {});
});
// Ações do menu do terminal (F2).
document.addEventListener("term-action", (e) => {
  const a = e.detail?.action ?? e.detail, t = target();
  if (a === "rename" && t) { const n = $("tabs").querySelector(`[data-key="${CSS.escape(t.key)}"] .tab-name`) || document.querySelector(`.pane-head[data-key="${CSS.escape(t.key)}"] .pane-title`); if (n) renameSession(t.conn, t.sid, n, t.label); }
  else if (a === "split") splitMenu();
  else if (a === "split-new") splitNew();
  else if (a === "orient") toggleOrient();
  else if (a === "swap") swapSides();
  else if (a === "maximize") toggleMax();
  else if (a === "unsplit" && activeTab()) unsplit(activeTab());
  else if (a === "new") newSession(t ? t.conn : LOCAL);
  else if (a === "close" && t) activeTab().split ? closePane(focusPane) : closeTab(t.key, true);
  else if (a === "export" && t) window.__TAURI__.dialog.save({ defaultPath: `${(t.label || "terminal").replace(/[\\/:*?"<>|]/g, "_")}.txt`, filters: [{ name: "Texto", extensions: ["txt"] }] })
    .then((path) => path && invoke("term_export_file", { conn: t.conn, sessionId: t.sid, path }).then(() => banner("Saída salva em " + path, "ok")))
    .catch((err) => banner(msg(err), "err"));
});
listen("term-exited", ({ payload }) => {
  const t = allRecs().find((t) => t.key === payload.key);
  if (t) { t.exited = payload.code; if (t === target()) updateBanner(); paintSplit(); }
  // Encerrar o terminal fecha a guia dele (#31); no dividido, só o painel.
  const host = tabs.find((x) => x.key === payload.key || x.split?.key === payload.key);
  if (host) {
    const n = host.key === payload.key ? 0 : 1;
    if (!host.split) closeTab(host.key);
    else if (host.key === active) closePane(n);
    else if (n) { release(host.split); host.split = null; host.max = null; renderTabs(); }
    else { tabs[tabs.indexOf(host)] = host.split; release(host); host.split = null; renderTabs(); }
  }
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
      for (const t of allRecs().filter((t) => t.conn === id && t.exited == null)) invoke("term_attach", { conn: id, sessionId: t.sid, cols, rows }).catch(() => {});
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

function send(data, t = target()) {
  if (t && data.includes("\r")) lastEnter[t.key] = Date.now();
  if (t && t.exited == null) invoke("term_input", { conn: t.conn, sessionId: t.sid, data }).catch(() => {});
}

const termKey = (e) => {
  if (e.isComposing || e.key === "Process" || e.key === "Dead") return;
  if (paneKey(e) || screenKey(e)) return;
  // Exceções da regra #23: ficam com o app mesmo com o terminal em foco. F5 vai ao programa (#26).
  if (e.ctrlKey && e.shiftKey && ["C", "V", "T", "W", "F6", "B", "R"].includes(e.key.length === 1 ? e.key.toUpperCase() : e.key)) return;
  if (e.ctrlKey && e.key === "Tab") return;
  if (e.key === "F11") return;
  const z = zoomDir(e);
  if (z != null) { e.preventDefault(); e.stopPropagation(); return zoom(z); } // #52: com foco, só os terminais
  const pg = e.key === "PageUp" ? 1 : e.key === "PageDown" ? -1 : 0, t = target();
  // Shift+PgUp/PgDn rola o histórico sempre; sem Shift, só no shell (tela cheia como vim/less recebe a tecla).
  if (pg && t && !e.ctrlKey && !e.altKey && (e.shiftKey || !screens[t.key]?.alt)) {
    e.preventDefault(); e.stopPropagation();
    return invoke("term_scroll", { conn: t.conn, sessionId: t.sid, lines: pg * Math.max(1, size(pane()).rows - 1) }).catch(() => {});
  }
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
  else if (paneKey(e)) return;
  else if (e.ctrlKey && e.shiftKey && k === "W") { e.preventDefault(); if (t?.split) closePane(focusPane); else if (active) closeTab(active); }
  else if (e.ctrlKey && k === "Tab") {
    e.preventDefault();
    const i = tabs.findIndex((t) => t.key === active);
    if (tabs.length) activate(tabs[(i + (e.shiftKey ? tabs.length - 1 : 1)) % tabs.length].key);
  }
  else if (e.ctrlKey && e.shiftKey && !e.altKey && k === "B") {
    e.preventDefault();
    $("fold-side").click();
    const c = $("layout").classList.contains("collapsed");
    if (c && $("sidebar").contains(document.activeElement)) $("screen").focus();
  }
  else if (e.ctrlKey && e.shiftKey && k === "C") {
    e.preventDefault();
    copySel();
  }
  else if (e.ctrlKey && e.shiftKey && k === "V") { e.preventDefault(); paste(); }
  else if (e.ctrlKey && e.shiftKey && k === "F6") {
    e.preventDefault();
    // Fechada, o foco dentro da coluna a expande por cima (:focus-within).
    ($("sidebar").querySelector("[aria-current=true]") || $("local-new")).focus();
  }
  else if (e.key === "F11") { e.preventDefault(); fullscreen(); }
  else if ((e.ctrlKey && e.shiftKey && !e.altKey && k === "R") || (e.key === "F5" && !e.ctrlKey && !e.altKey)) { e.preventDefault(); redraw(); }
  else if (zoomDir(e) != null) { e.preventDefault(); appZoom(zoomDir(e)); } // #52: sem foco no terminal, só o app
  else if (binds.palette && keyName(e) === binds.palette) { e.preventDefault(); action("palette")(); }
  else if (!e.ctrlKey && !e.altKey && !e.shiftKey && !e.target.closest?.(".terminal, input, textarea, select")) itemKey(e);
});

// Fora do terminal (#24): F1 ajuda, F2 renomeia, Delete fecha, Enter abre; Esc sai da tela cheia (#26).
function itemKey(e) {
  const it = e.target.closest?.("[data-key]"), key = it?.dataset.key, tab = it?.getAttribute("role") === "tab";
  if (e.key === "F1") help();
  else if (e.key === "Escape" && isFull) fullscreen();
  else if (!key) return;
  else if (e.key === "F2") it.querySelector(".tab-name, .card-t")?.dispatchEvent(new MouseEvent("dblclick"));
  else if (e.key === "Delete" && tab) closeTab(key, true);
  else if (e.key === "Delete") {
    const i = key.lastIndexOf("/"), conn = key.slice(0, i), s = conns[conn]?.sessions.find((s) => s.session_id === key.slice(i + 1));
    if (s) endSession(conn, s);
  }
  else if (e.key === "Enter" && tab) activate(key);
  else return; // Enter no item da lista já abre pelo clique nativo do botão
  e.preventDefault();
}
let isFull = false;
function fullscreen() {
  const w = window.__TAURI__?.window?.getCurrentWindow?.();
  if (w) { isFull = !isFull; w.setFullscreen(isFull).catch((err) => banner(msg(err), "err")); }
}
// F5 (fora do terminal) / Ctrl+Shift+R: redesenha. No dividido, só o painel focado; sem foco num terminal, os dois.
function redraw() {
  const t = activeTab();
  if (!t) return;
  const inTerm = !!document.activeElement?.closest?.(".terminal");
  if (!inTerm || !focusPane || !t.split) draw(t.key, $("screen"));
  if (t.split && (!inTerm || focusPane)) draw(t.split.key, $("screen2"));
  resizeActive();
}
// Ctrl+= / Ctrl++ / Ctrl+- / Ctrl+0, normal e numpad: 1, -1, 0 ou null.
function zoomDir(e) {
  if (!e.ctrlKey || e.altKey) return null;
  return { Equal: 1, NumpadAdd: 1, Minus: -1, NumpadSubtract: -1, Digit0: 0, Numpad0: 0 }[e.code] ?? null;
}
// Zoom do app (texto e ícones); a fonte dos terminais compensa para eles não mudarem (#52).
let uiZoom = +localStorage.getItem("app-zoom") || 1;
function appZoom(d) {
  uiZoom = d ? Math.max(0.5, Math.min(2, Math.round((uiZoom + d / 10) * 10) / 10)) : 1;
  localStorage.setItem("app-zoom", uiZoom);
  applyUiZoom();
}
function applyUiZoom() {
  document.body.style.zoom = uiZoom === 1 ? "" : uiZoom;
  $("screen").style.fontSize = $("screen2").style.fontSize = font / uiZoom + "px";
  fit();
}

// Atalhos dos painéis: Ctrl+Shift+D divide com terminal novo (#33); com divisão, Alt+setas alterna o foco.
// Ctrl+W vai ao terminal; Ctrl+Shift+W fecha só o painel focado (#24). true = consumida.
function paneKey(e) {
  const t = activeTab(), inTerm = !!e.target.closest?.(".terminal"), k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  let done = true;
  if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyD") splitNew();
  else if (t?.split && inTerm && e.altKey && !e.ctrlKey && !e.shiftKey && ["ArrowLeft", "ArrowRight", ...(layout.stack ? ["ArrowUp", "ArrowDown"] : [])].includes(k)) setFocusPane(focusPane ? 0 : 1);
  else done = false;
  if (done) { e.preventDefault(); e.stopPropagation(); }
  return done;
}

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
const rowsEl = (s = pane()) => [...s.children].filter((n) => n.id !== "measure");
// Seleção em curso/ativa segura o redesenho do painel onde está (cada painel tem a sua).
const selecting = new Set(), pendingDraw = new Set();
const selIn = (s) => { const g = getSelection(); return !g.isCollapsed && s.contains(g.anchorNode); };
const hasSel = (s = pane()) => selIn(s);
function holdDraw(s = $("screen")) { const h = selecting.has(s) || selIn(s); if (h) pendingDraw.add(s); return h; }
function flushDraw() {
  for (const s of [...pendingDraw]) {
    if (holdDraw(s)) continue;
    pendingDraw.delete(s);
    const k = s === $("screen") ? active : activeTab()?.split?.key;
    if (k) draw(k, s);
  }
}
for (const s of [$("screen"), $("screen2")]) {
  // Arrastar/duplo/triplo clique: o outro painel fica sem seleção (user-select:none) até soltar, e a seleção não vaza.
  s.addEventListener("mousedown", (e) => {
    if (e.button !== 0) return;
    selecting.add(s);
    const other = s === $("screen") ? $("screen2") : $("screen");
    if (selIn(other)) getSelection().removeAllRanges();
    other.classList.add("sel-lock");
    // Arrastar além do topo/base rola o painel; mais longe da borda, mais rápido. A seleção acompanha via extend().
    let y = e.clientY, x = e.clientX, raf = 0;
    const move = (ev) => { y = ev.clientY; x = ev.clientX; };
    const tick = () => {
      if (!selecting.has(s)) { removeEventListener("mousemove", move); return; }
      const r = s.getBoundingClientRect(), d = y < r.top ? y - r.top : y > r.bottom ? y - r.bottom : 0;
      if (d) {
        s.scrollTop += Math.sign(d) * Math.min(4 + Math.abs(d) / 2, 80);
        const p = document.caretPositionFromPoint?.(Math.min(Math.max(x, r.left + 1), r.right - 1), d < 0 ? r.top + 1 : r.bottom - 1);
        if (p && s.contains(p.offsetNode)) getSelection().extend(p.offsetNode, p.offset);
      }
      raf = requestAnimationFrame(tick);
    };
    addEventListener("mousemove", move);
    raf = requestAnimationFrame(tick);
  });
  // Clique e foco escolhem o painel que recebe colar, arrastar, busca e exportar.
  s.addEventListener("focus", () => setFocusPane(s === $("screen2") ? 1 : 0, false));
}
document.addEventListener("mouseup", () => { selecting.clear(); $("screen").classList.remove("sel-lock"); $("screen2").classList.remove("sel-lock"); setTimeout(flushDraw); });
document.addEventListener("selectionchange", () => { if (!selecting.size) flushDraw(); });

function copySel() {
  const s = hasSel() ? getSelection().toString() : "";
  if (!s) return;
  invoke("clip_copy", { text: s }).catch((err) => banner(msg(err), "err"));
  getSelection().removeAllRanges();
  pane().focus();
}
function paste() {
  // Painel focado (esquerdo ou direito): a sessão e a pasta dele recebem o print.
  const t = target();
  if (!t || t.exited != null) return pane().focus();
  const dir = dropCwds[t.key] ?? null;
  // Print: local vai para <pasta>\.live-cmd; remoto é enviado ao outro PC. Cola o caminho (claude/codex anexam pelo caminho).
  const ssh = sshOf(t);
  invoke("term_paste", { conn: t.conn, sessionId: t.sid, targetDir: dir, ssh }).then((name) => {
    if (!name) return;
    const local = t.conn === LOCAL;
    // Local e host novo já devolvem o caminho absoluto; host antigo devolve só o nome e o caminho é deduzido.
    const abs = /^([A-Za-z]:)?[\\/]/.test(name);
    const path = local || abs ? name : dir ? `${dir.replace(/[\\/]+$/, "")}\\screenshots\\${name}` : `~\\Pictures\\Screenshots\\${name}`;
    send(`"${path}"`, t);
    banner(ssh ? `Print enviado ao servidor: ${path}.` : local ? `Print salvo: ${name}.` : `Print enviado: ${path}.`);
  }).catch((err) => banner(msg(err), "err"));
  pane().focus();
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
  const hit = Object.keys(SHORTCUTS).find((a) => a !== "help" && a !== "palette" && keyName(e) === binds[a]); // F1 só fora do terminal (#24)
  if (plain && k === "c" && hasSel()) copySel();
  else if (hit) SHORTCUTS[hit][1]();
  else if (k === "ContextMenu" || (e.shiftKey && k === "F10")) { const r = pane().getBoundingClientRect(); ctxMenu(r.left + 20, r.top + 20); }
  else done = false;
  if (done) { e.preventDefault(); e.stopPropagation(); }
  return done;
}

// Menu de contexto da tela (substitui o nativo do WebView: Recarregar, Inspecionar…).
function ctxMenu(x, y) {
  const at = { getBoundingClientRect: () => ({ left: x, bottom: y }), focus: () => pane().focus() };
  const sp = activeTab()?.split;
  // A9 (#32/#34): Copiar/Colar, Tema, fontes, Paleta, Nova sessão, Reabrir e Atalhos saíram do menu.
  menu(at, [
    { row: [["🔍", "Buscar", openFind, binds.find], ["⌫", "Limpar", clearTab, binds.clear],
      ["⤢", sp && activeTab().max != null ? "Restaurar painéis" : "Maximizar painel", sp ? action("maximize") : () => banner("Maximizar vale com a guia dividida.", "warn")], ["⇩", "Exportar saída", action("export")]] },
    "#Painel",
    sp ? ["Desfazer divisão", action("unsplit"), "", "◫"] : ["Dividir", action("split-new"), "Ctrl+Shift+D", "◫"],
    ["Inverter lados", sp && action("swap"), "", "⇄"],
    "#Guia",
    ["Renomear", action("rename"), "", "✎"],
    ["Fechar guia", action("close"), "Ctrl+Shift+W", "✕", "danger"],
  ].filter((it) => typeof it === "string" || it.row || it[1]));
}
// Limpa o buffer vt100 no app (vale em cmd) e manda Ctrl+L para o shell redesenhar o prompt.
function clearTab() {
  const t = target(), s = pane();
  if (t) invoke("term_clear", { conn: t.conn, sessionId: t.sid }).then(() => { send("\x0c", t); draw(t.key, s); }).catch((err) => banner(msg(err), "err"));
  s.focus();
}
// Botão direito: copia se há seleção, senão cola (como no Windows Terminal). Shift+botão direito abre o menu.
for (const s of [$("screen"), $("screen2")]) s.oncontextmenu = (e) => {
  e.preventDefault();
  s.focus();
  if (e.shiftKey) ctxMenu(e.clientX, e.clientY);
  else if (hasSel()) copySel();
  else paste();
};
// Recolher/expandir a lista de terminais e o topo (destaque + resumo) para ampliar o terminal; lembrado por aparelho.
function foldToggle(btn, apply, key) {
  const set = (on) => {
    apply(on);
    btn.setAttribute("aria-pressed", String(on));
    try { localStorage.setItem(key, on ? "1" : ""); } catch {}
    requestAnimationFrame(() => window.dispatchEvent(new Event("resize")));
  };
  let on = false;
  try { on = localStorage.getItem(key) === "1"; } catch {}
  set(on);
  btn.onclick = () => set(btn.getAttribute("aria-pressed") !== "true");
}
foldToggle($("fold-side"), (on) => $("layout").classList.toggle("collapsed", on), "fold-side");
foldToggle($("fold-top"), (on) => { $("hero").hidden = on; $("stats").hidden = on; }, "fold-top");

// Fonte: Ctrl+roda, Ctrl+=/-/0; salva por PC no localStorage e recalcula cols/rows.
let FONT0 = 13; // V1 Aurora: Fira Code 13 px; a aparência (aparencia.js) troca o padrão
window.setFontBase = (n) => { if (n === FONT0) return; FONT0 = n; font = 0; applyFont(); };
let font = FONT0;
function applyFont() {
  const f = +localStorage.getItem("term-font") || FONT0; // um tamanho para todos os terminais (#52)
  if (f === font && $("screen").style.fontSize) return;
  font = f;
  applyUiZoom();
}
function zoom(d) {
  localStorage.setItem("term-font", d ? Math.max(8, Math.min(32, font + d)) : FONT0);
  applyFont();
}
applyUiZoom();

// Busca (Ctrl+F) nas linhas desenhadas, com CSS Highlight; no fim da tela rola o histórico (term_scroll).
let findQ = "", findI = 0, findDir = 0, findHits = [];
function openFind() { $("find").hidden = false; $("find-q").select(); $("find-q").focus(); }
function closeFind() { $("find").hidden = true; findQ = ""; afterDraw(); pane().focus(); }
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
  const t = target();
  if (!t) return;
  findDir = d;
  const n = size(pane()).rows;
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
for (const scr of [$("screen"), $("screen2")]) scr.addEventListener("click", (e) => {
  if (!e.ctrlKey) return;
  const c = document.caretRangeFromPoint(e.clientX, e.clientY), row = c && rowsEl(scr).find((r) => r.contains(c.startContainer));
  if (!row) return;
  const pre = new Range();
  pre.setStart(row, 0); pre.setEnd(c.startContainer, c.startOffset);
  const link = linkAt(row.textContent, pre.toString().length);
  if (!link) return;
  e.preventDefault();
  const op = window.__TAURI__.opener;
  (/^https?:/.test(link) ? op.openUrl(link) : op.revealItemInDir(link)).catch(() => banner(msg("NOT_FOUND"), "err"));
});
for (const scr of [$("screen"), $("screen2")]) scr.addEventListener("mousemove", (e) => scr.classList.toggle("ctrl", e.ctrlKey));

// Arrastar arquivo(s): sessão local cola o(s) caminho(s) entre aspas; remota envia ao outro PC (pasta atual da sessão, senão a padrão do host).
window.__TAURI__.event.listen("term-cwd", (e) => { dropCwds[e.payload.key] = e.payload.cwd || undefined; renderSidebar(); }); // #11: nome do card segue o `cd`
window.__TAURI__.webview.getCurrentWebview().onDragDropEvent(async ({ payload }) => {
  if (payload.type !== "drop" || !payload.paths?.length) return;
  // Solto sobre um painel: ele vira o focado e recebe o(s) arquivo(s); fora dos painéis vale o focado.
  if (activeTab()?.split && payload.position) {
    const x = payload.position.x / devicePixelRatio, y = payload.position.y / devicePixelRatio;
    const over = [$("screen"), $("screen2")].findIndex((s) => { const r = s.getBoundingClientRect(); return r.width && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom; });
    if (over >= 0) setFocusPane(over, false);
  }
  const t = target();
  const ssh = sshOf(t);
  if (ssh) {
    // SSH: grava no servidor (<pasta>/archives ou ~/Downloads/archives) e cola os caminhos remotos.
    const out = [];
    for (const [i, path] of payload.paths.entries()) {
      const name = path.split(/[\\/]/).pop();
      banner(`Enviando ${name} ao servidor (${i + 1}/${payload.paths.length})…`);
      try { out.push(await invoke("term_upload", { conn: t.conn, path, sessionId: t.sid, ssh, cwd: dropCwds[t.key] ?? null })); }
      catch (e) { return banner(`Falha ao enviar ${name}: ${msg(e)}`, "err"); }
    }
    send(out.map((p) => `"${p}"`).join(" "), t);
    banner(`${out.length} arquivo(s) enviado(s) ao servidor.`);
    return pane().focus();
  }
  if (!t || t.conn === LOCAL) { send(payload.paths.map((p) => `"${p}"`).join(" "), t); pane().focus(); return; }
  for (const [i, path] of payload.paths.entries()) {
    const name = path.split(/[\\/]/).pop();
    banner(`Enviando ${name} (${i + 1}/${payload.paths.length})…`);
    try { await invoke("term_upload", { conn: t.conn, path, sessionId: t.sid }); }
    catch (e) { return banner(`Falha ao enviar ${name}: ${msg(e)}`, "err"); }
  }
  banner(`${payload.paths.length} arquivo(s) enviado(s) para ${dropCwds[t.key] ? dropCwds[t.key].replace(/[\\/]+$/, "") + "\\archives" : "Downloads\\archives no outro PC"}.`);
});

// Ajuda (F1).
const KEYS = [["Ctrl+C", "Copiar seleção (sem seleção: interrompe, ^C)"], ["Ctrl+V / Ctrl+Shift+V", "Colar"], ["Ctrl+Shift+C", "Copiar seleção"],
  ["Ctrl+Shift+F", "Buscar"], ["Enter / Shift+Enter", "Próximo / anterior na busca"], ["Ctrl+Shift+T", "Nova sessão"], ["Ctrl+Shift+W", "Fechar aba (com divisão: só o painel focado)"],
  ["Ctrl+Shift+D", "Dividir com terminal novo"], ["Alt+← / Alt+→", "Alternar o foco entre os painéis divididos"],
  ["F2 / Delete / Enter (fora do terminal)", "Renomear / fechar / abrir o item focado"], ["F11", "Tela cheia (Esc fora do terminal sai)"], ["F5 (fora do terminal) / Ctrl+Shift+R", "Redesenhar a tela"],
  ["Ctrl+= / Ctrl+- / Ctrl+0", "Zoom: no terminal, a fonte de todos; fora, o app"], ["Shift+PgUp / Shift+PgDn", "Rolar o histórico (sem Shift: no shell rola, em tela cheia vai ao programa)"],
  ["Arrastar o divisor", "Ajustar os painéis · duplo clique volta a 50/50 · setas ajustam"],
  ["Ctrl+Tab / Ctrl+Shift+Tab", "Próxima / aba anterior"], ["Ctrl+Shift+B", "Mostrar/ocultar lista lateral"], ["Ctrl+Shift+F6", "Ir para a lista lateral"],
  ["Ctrl+roda do mouse", "Zoom da fonte dos terminais"], ["Roda do mouse", "Rolar histórico"],
  ["Ctrl+clique", "Abrir link ou caminho"], ["Botão direito", "Colar (com seleção: copiar)"], ["Shift+botão direito / Shift+F10 / ☰", "Menu do terminal"], ["Arrastar arquivo", "Local: cola o caminho · Remoto/SSH: envia e cola o caminho"]];
// Atalhos remapeáveis: ação fixa, tecla editável em Configurações > Atalhos (#55; clique na tecla e pressione a nova). Salvos no localStorage.
const SHORTCUTS = {
  paste: ["Colar", () => paste(), "Ctrl+V"], find: ["Buscar", () => openFind(), "Ctrl+Shift+F"], clear: ["Limpar", () => clearTab(), "Ctrl+Shift+K"],
  palette: ["Paleta de comandos", () => action("palette")(), ""], // #33: sem atalho padrão
  help: ["Esta ajuda", () => help(), "F1"],
};
// Atalhos antigos (Ctrl+F, Ctrl+=…) iam contra programas do terminal: descartados se salvos.
const OLD_KEYS = ["Ctrl+F", "Ctrl+=", "Ctrl+-", "Ctrl+0"];
const binds = { ...Object.fromEntries(Object.entries(SHORTCUTS).map(([a, s]) => [a, s[2]])), ...(() => { try { return Object.fromEntries(Object.entries(JSON.parse(localStorage.getItem("term-keys") || "{}")).filter(([, k]) => !OLD_KEYS.includes(k))); } catch { return {}; } })() };
function keyName(e) {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.key.length === 1 ? e.key.toUpperCase() : e.key].filter(Boolean).join("+");
}
// Configurações > Atalhos: as teclas remapeáveis entram no topo da lista, editáveis (#55).
$("keys-list").prepend(...Object.entries(SHORTCUTS).map(([a, [d]]) => {
  const b = el("button", "mono", binds[a] || "—");
  b.type = "button"; b.title = "Clique e pressione a nova tecla";
  b.onclick = () => { b.textContent = "…"; b.onkeydown = (e) => {
    const n = keyName(e);
    if (!n) return;
    e.preventDefault(); e.stopPropagation();
    // Tecla sem Ctrl/Alt (Enter, letras…) é do terminal: só F1–F12 passam sozinhas.
    if (n !== "Escape" && (e.ctrlKey || e.altKey || /^F\d+$/.test(e.key))) { binds[a] = n; localStorage.setItem("term-keys", JSON.stringify(binds)); }
    b.textContent = binds[a] || "—"; b.onkeydown = null;
  }; };
  const li = el("li");
  li.append(el("span", null, d), b);
  return li;
}));
function help() {
  const opener = document.activeElement;
  // F1 só lista (#55): a edição fica em Configurações > Atalhos.
  const rows = Object.entries(SHORTCUTS).flatMap(([a, [d]]) => [el("dt", "mono", binds[a] || "—"), el("dd", null, d)]);
  $("help-list").replaceChildren(...rows, ...KEYS.flatMap(([k, d]) => [el("dt", "mono", k), el("dd", null, d)]));
  $("help-scrim").hidden = false;
  const end = () => { $("help-scrim").hidden = true; opener?.focus(); };
  $("help-ok").onclick = end;
  trap($("help"), end);
  $("help-ok").focus();
}

// ---------- Tamanho ----------
// Cada painel redimensiona o seu PTY com o próprio tamanho (o divisor e a orientação mudam as medidas).
function resizeActive() {
  const t = activeTab();
  if (!t) return;
  const one = (r, s) => {
    if (r.exited != null || s.hidden || !s.offsetWidth) return;
    const { cols, rows } = size(s);
    invoke("term_resize", { conn: r.conn, sessionId: r.sid, cols, rows }).catch(() => {});
  };
  one(t, $("screen"));
  if (t.split) one(t.split, $("screen2"));
}
// `var`: `applyUiZoom()` roda no carregamento (acima) e chama fit() antes desta linha; `let` dava TDZ e abortava o script.
var resizeTimer = 0;
function fit() {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(resizeActive, 100);
}
const ro = new ResizeObserver(fit);
ro.observe($("screen"));
ro.observe($("screen2"));

// ---------- Divisor (sash), cabeçalhos dos painéis e arrastar aba para dividir ----------
(() => {
  const sash = $("sash"), area = $("term-area");
  const at = (e) => { const r = area.getBoundingClientRect(); return layout.stack ? (e.clientY - r.top) / r.height * 100 : (e.clientX - r.left) / r.width * 100; };
  sash.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    sash.setPointerCapture(e.pointerId);
    sash.classList.add("drag");
  });
  sash.addEventListener("pointermove", (e) => { if (sash.hasPointerCapture(e.pointerId)) setPct(at(e)); });
  const end = (e) => {
    if (!sash.hasPointerCapture(e.pointerId)) return;
    sash.releasePointerCapture(e.pointerId);
    sash.classList.remove("drag");
    saveLayout();
  };
  sash.addEventListener("pointerup", end);
  sash.addEventListener("pointercancel", end);
  sash.addEventListener("dblclick", () => setPct(50, true));
  sash.addEventListener("keydown", (e) => {
    const back = layout.stack ? "ArrowUp" : "ArrowLeft", fwd = layout.stack ? "ArrowDown" : "ArrowRight";
    if (e.key === back || e.key === fwd) setPct(layout.pct + (e.key === fwd ? 2 : -2), true);
    else if (e.key === "Enter" || e.key === "Home") setPct(50, true);
    else return;
    e.preventDefault();
  });

  // Cabeçalho de cada painel: título (duplo clique renomeia), orientação, maximizar, desfazer divisão, fechar.
  for (const [i, h] of [[0, $("head1")], [1, $("head2")]]) {
    const title = el("span", "pane-title");
    title.ondblclick = () => { const r = i ? activeTab()?.split : activeTab(); if (r) renameSession(r.conn, r.sid, title, r.label); };
    const dot = actDot();
    dot.classList.add("pane-act");
    dot.hidden = true;
    h.append(title, dot);
    for (const [act, text, label, fn] of [["swap", "⇆", "Inverter lados", swapSides], ["orient", "⇅", "Alternar orientação", toggleOrient], ["max", "⤢", "Maximizar painel", toggleMax],
      ["unsplit", "⊟", "Desfazer divisão", () => unsplit(activeTab())], ["close", "×", "Fechar este painel (Ctrl+Shift+W)", () => closePane(i)]]) {
      const b = el("button", "pane-btn", text);
      b.type = "button";
      b.dataset.act = act;
      b.title = label;
      b.setAttribute("aria-label", label);
      b.onclick = (e) => { e.stopPropagation(); setFocusPane(i, false); fn(); };
      h.append(b);
    }
    h.onmousedown = (e) => { if (!e.target.closest("button, input")) { e.preventDefault(); setFocusPane(i); } };
  }

  // Arrastar uma aba para a área do terminal divide: a aba vira o painel da aba ativa.
  const hint = $("drop-hint");
  const dropTab = (key) => {
    const t = activeTab(), d = tabs.find((x) => x.key === key);
    hint.hidden = true;
    if (t && d && d !== t) split(t, d.conn, d.sid);
  };
  const over = (x, y) => { const r = area.getBoundingClientRect(); return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom; };
  const showHint = (on) => { hint.hidden = !on; hint.classList.toggle("stack", layout.stack); };
  // HTML5 (quando o WebView entrega drag/drop de DOM).
  area.addEventListener("dragover", (e) => { if ([...e.dataTransfer.types].includes("text/x-tab")) { e.preventDefault(); showHint(true); } });
  area.addEventListener("dragleave", (e) => { if (!area.contains(e.relatedTarget)) showHint(false); });
  area.addEventListener("drop", (e) => { const k = e.dataTransfer.getData("text/x-tab"); if (k) { e.preventDefault(); dropTab(k); } });
  // Ponteiro (alternativa: o handler de arquivos do Tauri pode engolir o drag de DOM no Windows).
  let press = null, moved = false;
  $("tabs").addEventListener("pointerdown", (e) => { const b = e.button === 0 && e.target.closest("[role=tab]"); press = b ? { key: b.dataset.key, x: e.clientX, y: e.clientY } : null; moved = false; });
  document.addEventListener("pointermove", (e) => {
    if (!press) return;
    if (!moved && Math.hypot(e.clientX - press.x, e.clientY - press.y) < 12) return;
    moved = true;
    showHint(over(e.clientX, e.clientY) && press.key !== active);
  });
  const stop = (e) => {
    const p = press;
    press = null;
    if (p && moved && e.type === "pointerup") {
      const bar = $("tabs").getBoundingClientRect(), on = document.elementFromPoint(e.clientX, e.clientY)?.closest("#tabs [role=tab]");
      if (on || (e.clientY >= bar.top && e.clientY <= bar.bottom)) moveTab(p.key, on?.dataset.key ?? null, e.clientX);
      else if (over(e.clientX, e.clientY)) dropTab(p.key);
    }
    showHint(false);
  };
  document.addEventListener("pointerup", stop);
  document.addEventListener("pointercancel", stop);
})();

// ---------- Início ----------
$("local-new").onclick = () => profileMenu($("local-new"), LOCAL);
$("empty-new").onclick = () => newSession(LOCAL);
// Retomar: abre em abas todas as sessões locais vivas (inclusive as restauradas do disco),
// na ordem das abas da última vez e voltando para a aba que estava ativa (salvas em saveTabs).
refresh(LOCAL).then(async () => {
  let saved = { order: [], active: null, splits: {} };
  try { saved = { ...saved, ...JSON.parse(localStorage.getItem("tabs-state") || "{}") }; } catch {}
  const pos = (s) => { const i = saved.order.indexOf(LOCAL + "/" + s.session_id); return i < 0 ? Infinity : i; };
  const list = (conns[LOCAL].sessions || []).filter((s) => s.state !== "closed").sort((a, b) => pos(a) - pos(b));
  restoring = true;
  for (const s of list) await openSession(LOCAL, s).catch(() => {});
  // Divisões salvas: se as duas sessões voltaram como abas separadas, junta numa só.
  for (const [main, other] of Object.entries(saved.splits || {})) {
    const m = tabs.find((t) => t.key === main), o = tabs.find((t) => t.key === other);
    if (m && o && m !== o && !m.split && !o.split) await split(m, o.conn, o.sid).catch(() => {});
  }
  restoring = false;
  const back = tabs.find((t) => t.key === saved.active || t.split?.key === saved.active) || tabs.find((t) => t.key === active) || tabs[0];
  if (back) activate(back.key);
});
refreshRemotes();
// Sessões criadas pelo celular aparecem sozinhas; não redesenha a lista sob o foco do teclado.
// ponytail: polling de 5 s; trocar por evento do host quando C3 tiver aviso de sessão nova.
setInterval(() => { if (!document.hidden && !$("sidebar").contains(document.activeElement)) refresh(LOCAL); }, 5000);
renderTabs();
updateBanner();

// Item 20: aviso de atualização (mesmos comandos da janela principal).
invoke("check_update").catch(() => null).then((v) => {
  if (!v) return staleDaemon();
  $("upd-t").textContent = `Versão ${v} disponível.`;
  $("upd-banner").hidden = false;
});
$("upd-x").onclick = () => { $("upd-banner").hidden = true; };
// Atualização não troca o serviço de terminais em uso: avisa e oferece reiniciar (fecha os terminais deste PC).
const staleDaemon = () => invoke("daemon_stale").then((stale) => {
  if (!stale) return;
  $("upd-t").textContent = "O serviço de terminais ainda é da versão anterior: conexões de outros PCs usam o código antigo.";
  $("upd-go").textContent = "Reiniciar serviço";
  $("upd-banner").hidden = false;
  $("upd-go").onclick = async () => {
    if (!await ask("Reiniciar o serviço de terminais?", "Os programas em execução nos terminais deste PC são encerrados e aparelhos conectados caem. Os terminais salvos reabrem em seguida.", "Reiniciar serviço")) return;
    $("upd-go").disabled = true;
    $("upd-t").textContent = "Reiniciando o serviço…";
    try { await invoke("daemon_restart"); location.reload(); }
    catch (e) { $("upd-go").disabled = false; $("upd-t").textContent = "Não foi possível reiniciar: " + msg(e); }
  };
}).catch(() => {});
$("upd-go").onclick = async () => {
  $("upd-go").disabled = true;
  $("upd-t").textContent = "Baixando e instalando…";
  try { await invoke("install_update"); } catch { $("upd-go").disabled = false; $("upd-t").textContent = "Não foi possível atualizar. Tente de novo."; }
};

// Barra superior própria (ticket 024): arraste/duplo clique/Snap via data-tauri-drag-region.
(() => {
  const w = window.__TAURI__?.window?.getCurrentWindow?.();
  if (!w) return;
  const max = document.getElementById("tb-max");
  const sync = async () => { const m = await w.isMaximized(); max.innerHTML = m ? "&#xE923;" : "&#xE922;"; max.title = max.ariaLabel = m ? "Restaurar" : "Maximizar"; };
  document.getElementById("tb-min").onclick = () => w.minimize();
  max.onclick = () => w.toggleMaximize();
  document.getElementById("tb-close").onclick = () => w.close();
  w.onResized(sync); sync();
})();

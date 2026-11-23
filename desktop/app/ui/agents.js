// Tela "Agentes de IA": sessões de agente por pasta raiz, com estado, Retomar (--resume) e nova sessão.
// Usa os globais de terminal.js (invoke, $, el, LOCAL, conns, remoteOrder, size, openSession, refresh, newAi, ask, banner, msg).
(() => {
  const panel = $("agents-panel"), nav = $("nav-agents");
  if (!panel || !nav) return;
  // Estado → [classe, marcador, texto]; o texto vai por extenso (o marcador é só visual).
  const STATUS = {
    rodando: ["dot-ok", "●", "Rodando"],
    esperando: ["dot-wait", "◆", "Esperando sua confirmação"],
    saiu: ["dot-warn", "◐", "Programa terminou"],
    encerrada: ["muted", "○", "Encerrada"],
    desconhecido: ["dot-err", "⚠", "PC sem resposta"],
  };
  const isLive = (x) => x.status === "rodando" || x.status === "saiu";
  let conn = LOCAL, opener = null;
  let filter = "all"; // all | live | resume | nome do agente
  let data = null;   // última resposta de agent_list
  // Agentes deste PC parados num pedido de confirmação (evento "agent-waiting" do host, o mesmo que vai ao celular).
  const waiting = new Set();
  window.__TAURI__.event.listen("agent-waiting", (e) => {
    const { session_id: id, label, waiting: on } = e.payload;
    if (on === waiting.has(id)) return;
    on ? waiting.add(id) : waiting.delete(id);
    if (on) banner(`${label || "Agente"} está esperando sua confirmação.`, "warn");
    if (!panel.hidden && conn === LOCAL) load();
  });

  const when = (ms) => {
    if (!ms) return "";
    const min = Math.round((Date.now() - ms) / 60000);
    if (min < 1) return "agora";
    if (min < 60) return `há ${min} min`;
    if (min < 1440) return `há ${Math.round(min / 60)} h`;
    return new Date(ms).toLocaleDateString("pt-BR");
  };
  const day = (ms) => {
    if (!ms) return "";
    const d = new Date(ms), t = new Date(), y = new Date();
    y.setDate(t.getDate() - 1);
    const hm = d.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
    if (d.toDateString() === t.toDateString()) return `hoje ${hm}`;
    if (d.toDateString() === y.toDateString()) return `ontem ${hm}`;
    return d.toLocaleDateString("pt-BR", { day: "2-digit", month: "short" }).replace(".", "").replace(" de ", " ");
  };
  const plural = (n, um, varios) => `${n} ${n === 1 ? um : varios}`;

  function connOptions() {
    const ids = [LOCAL, ...remoteOrder.filter((id) => conns[id]?.status.state === "ready")];
    if (!ids.includes(conn)) conn = LOCAL;
    $("ag-conn-row").hidden = ids.length < 2;
    $("ag-conn").replaceChildren(...ids.map((id) => {
      const o = el("option", null, conns[id].name);
      o.value = id; o.selected = id === conn;
      return o;
    }));
  }

  async function load() {
    connOptions();
    $("ag-state").textContent = "Carregando…";
    let v;
    try { v = await invoke("agent_list", { conn }); }
    catch (e) { $("ag-state").textContent = msg(e); $("ag-list").replaceChildren(); return; }
    data = v;
    render();
  }

  // Lista única (sem cabeçalho por pasta): sessões abertas primeiro, depois as mais recentes. A pasta vai na linha de detalhe.
  function render() {
    const v = data;
    if (!v) return;
    const all = v.groups.flatMap((g) => g.sessions.map((x) => ({ ...x, dir: g.path || "Pasta inicial do perfil", folder: g.name })));
    const live = (x) => isLive(x) || x.status === "desconhecido";
    const nLive = all.filter(isLive).length;
    $("ag-state").textContent = !v.online ? "O PC não respondeu; o estado das sessões é desconhecido."
      : all.length ? `${nLive} rodando · ${all.length - nLive} para retomar` : "";
    const agentsSeen = [...new Set(all.map((x) => x.agent))];
    $("ag-chips").replaceChildren(...[["all", "Todos"], ["live", "Rodando"], ["resume", "Retomar"], ...agentsSeen.map((a) => [a, a])].map(([f, t]) => {
      const c = el("button", "chip", t);
      c.type = "button"; c.dataset.f = f; c.setAttribute("aria-pressed", String(f === filter));
      return c;
    }));
    if (!all.length) {
      const e = el("div", "ag-empty");
      e.append(el("p", null, "Nenhuma sessão de agente ainda. Crie uma para conversar com claude, openclaude ou codex numa pasta; ela fica aqui para retomar depois."));
      $("ag-list").replaceChildren(e);
      return;
    }
    const keep = (x) => filter === "all" || (filter === "live" ? isLive(x) : filter === "resume" ? !live(x) : x.agent === filter);
    const rank = (x) => (waiting.has(x.session_id) ? 0 : isLive(x) ? 1 : 2);
    const shown = all.filter(keep).sort((p, q) => rank(p) - rank(q) || (q.used_ms || 0) - (p.used_ms || 0));
    const ul = el("ul", "ag-rows");
    ul.append(...shown.map(row));
    if (!shown.length) ul.append(el("li", "muted small", "Nenhuma sessão com esse filtro."));
    $("ag-list").replaceChildren(ul);
  }

  function row(x) {
    const li = el("li", "ag-row");
    li.dataset.ms = x.used_ms || "";
    li.dataset.agent = x.agent;
    li.dataset.dir = x.dir || "";
    li.dataset.live = isLive(x) ? "1" : "";
    const st = x.status === "rodando" && conn === LOCAL && waiting.has(x.session_id) ? "esperando" : x.status;
    const [cls, mark, label] = STATUS[st] || STATUS.desconhecido;
    const dot = el("span", `${cls} ag-dot`, mark);
    dot.setAttribute("aria-hidden", "true");
    const name = x.label || x.agent;
    const info = el("div", "ag-info");
    // Detalhe da linha: pasta para o que está aberto; agente e data para o que se retoma. O texto completo vai no title e para leitores de tela.
    const full = [x.agent + (x.args ? " " + x.args : ""), label, when(x.used_ms)].filter(Boolean).join(" · ");
    const short = isLive(x) ? x.dir : `${x.agent} · ${day(x.used_ms)}`;
    const detail = el("span", "muted small mono", short);
    detail.title = `${full} · ${x.dir}`;
    info.append(el("span", "ag-label", name), detail, el("span", "sr-only", `${full}. Pasta ${x.dir}`));
    const go = el("button", null, isLive(x) ? "Abrir" : "Retomar");
    go.type = "button";
    go.disabled = x.status === "desconhecido";
    go.setAttribute("aria-label", `${go.textContent} ${name}`);
    go.onclick = () => resume(x, go);
    const rm = el("button", null, "Remover");
    rm.type = "button";
    rm.setAttribute("aria-label", `Remover ${name}`);
    rm.onclick = () => remove(x, name);
    const act = el("span", "inline");
    act.append(go, rm);
    li.append(dot, info, act);
    return li;
  }

  async function resume(x, b) {
    b.disabled = true;
    const { cols, rows } = size();
    try {
      const s = await invoke("agent_resume", { conn, sessionId: x.session_id, cols, rows });
      hide(false);
      await openSession(conn, s);
      refresh(conn);
    } catch (e) { banner(msg(e), "err"); b.disabled = false; }
  }

  async function remove(x, name) {
    const live = isLive(x);
    if (!await ask(`Remover ${name}?`, live
      ? "A sessão está aberta: o terminal será encerrado e ela sai da lista."
      : "A sessão sai da lista e não poderá mais ser retomada por aqui.", "Remover")) return;
    try { await invoke("agent_remove", { conn, sessionId: x.session_id }); }
    catch (e) { banner(msg(e), "err"); }
    if (live) refresh(conn);
    load();
  }

  function show() {
    opener = document.activeElement;
    panel.hidden = false;
    nav.setAttribute("aria-expanded", "true");
    nav.setAttribute("aria-current", "true");
    $("ag-new").focus();
    load();
  }
  function hide(refocus = true) {
    panel.hidden = true;
    nav.setAttribute("aria-expanded", "false");
    nav.removeAttribute("aria-current");
    if (refocus) (opener && document.contains(opener) ? opener : nav).focus();
  }

  nav.onclick = () => (panel.hidden ? show() : hide());
  $("ag-x").onclick = () => hide();
  $("ag-reload").onclick = load;
  $("ag-chips").addEventListener("click", (e) => {
    const c = e.target.closest("button.chip[data-f]");
    if (!c) return;
    filter = c.dataset.f;
    render();
  });
  $("ag-conn").onchange = () => { conn = $("ag-conn").value; load(); };
  $("ag-new").onclick = () => { hide(false); newAi(conn); };
  // Terminal ganhou o foco (aba escolhida, sessão aberta por outro caminho): a tela sai da frente.
  $("screen").addEventListener("focus", () => { if (!panel.hidden) hide(false); });
  panel.onkeydown = (e) => { if (e.key === "Escape") { e.preventDefault(); hide(); } };
})();

// Formulário "Novo agente" (modelo B, #17). Padrão (agente + comandos extras) mora no host da conexão (#15);
// a pasta vem das recentes (sessões de agente registradas para essa conexão).
// Sugestões vêm do host (#18-#20); AI_SUG só cobre host antigo sem `agent_args`.
const AI_SUG = {
  claude: ["--model opus", "--model sonnet", "--continue", "--permission-mode plan", "--permission-mode acceptEdits", "--dangerously-skip-permissions", "--verbose"],
  openclaude: ["--model opus", "--model sonnet", "--continue", "--permission-mode plan", "--permission-mode acceptEdits", "--dangerously-skip-permissions", "--verbose"],
  codex: ["--model gpt-5", "--full-auto", "--search", "--sandbox read-only", "--sandbox workspace-write", "--ask-for-approval on-request"],
};
async function newAi(conn) {
  localStorage.removeItem("ai-default"); // resto de versão antiga; o padrão agora é do host
  const [d, list] = await Promise.all([
    invoke("agent_default_get", { conn }).catch(() => null),
    invoke("agent_list", { conn }).catch(() => null),
  ]);
  const recent = [...new Set((list?.groups || []).map((g) => g.path).filter(Boolean))];
  const opt = (v) => { const o = el("option"); o.value = v; return o; };
  const cwdIn = $("ai-cwd"), dirs = $("ai-dirs"), pick = $("ai-pick");
  $("ai-agent").value = d?.agent || "claude";
  if (!$("ai-agent").value) $("ai-agent").value = "claude";
  cwdIn.value = recent[0] || ""; $("ai-args").value = d?.args || ""; $("ai-def").checked = false;
  $("ai-cwd-sug").replaceChildren(...recent.map(opt));
  // Comandos extras já usados com o agente no host + opções conhecidas; texto livre continua valendo.
  const sug = async () => {
    const a = $("ai-agent").value, l = await invoke("agent_args", { conn, agent: a }).catch(() => null);
    if (a === $("ai-agent").value) $("ai-sug").replaceChildren(...(Array.isArray(l) ? l : AI_SUG[a] || []).map(opt));
  };
  $("ai-agent").onchange = sug; sug();
  // Caminho completo embaixo do campo: o que vai para o host, sem barra final.
  const path = () => { $("ai-path").textContent = cwdIn.value.trim().replace(/[\/]+$/, "") || "Pasta inicial do perfil (~)"; };
  // 📁 abre a navegação: subpastas no PC da conexão; clique entra, ".." sobe, raiz lista as unidades.
  const browse = async (p) => {
    let ds = [];
    try { ds = await invoke("term_dirs", { conn, path: p }); } catch (e) { banner(msg(e), "err"); }
    const base = p.replace(/[\/]+$/, ""), up = base.includes("\\") ? base.slice(0, base.lastIndexOf("\\") + 1) : "";
    const item = (name, to) => { const b = el("button", null, name); b.type = "button"; b.onclick = () => { cwdIn.value = to; path(); browse(to); }; return b; };
    dirs.replaceChildren(...(p ? [item("..", up)] : []), ...ds.map((x) => item(x, p ? base + "\\" + x : x)));
  };
  const show = (on) => { dirs.hidden = !on; pick.setAttribute("aria-expanded", on); if (on) browse(cwdIn.value.trim()); };
  pick.onclick = () => show(dirs.hidden);
  // Recentes primeiro; com "\\" no campo, as subpastas reais do pai que começam pelo trecho digitado.
  // Pasta inexistente ou sem permissão: o host devolve vazio/erro e ficam só as recentes.
  let seq = 0;
  cwdIn.oninput = async () => {
    path();
    const v = cwdIn.value, i = v.lastIndexOf("\\"), n = ++seq;
    const rec = recent.filter((r) => r.toLowerCase().startsWith(v.toLowerCase()));
    if (i < 0) return $("ai-cwd-sug").replaceChildren(...(v ? rec : recent).map(opt));
    const ds = await invoke("term_dirs", { conn, path: v.slice(0, i + 1) }).catch(() => []);
    if (n !== seq) return;
    const pre = v.slice(i + 1).toLowerCase(), sub = (Array.isArray(ds) ? ds : []).filter((d) => d.toLowerCase().startsWith(pre)).map((d) => v.slice(0, i + 1) + d);
    $("ai-cwd-sug").replaceChildren(...[...new Set([...rec, ...sub])].map(opt));
  };
  cwdIn.onchange = () => { path(); if (!dirs.hidden) browse(cwdIn.value.trim()); };
  show(false); path();
  $("ai-scrim").hidden = false; cwdIn.focus();
  const ok = await new Promise((done) => {
    const end = (v) => { $("ai-scrim").hidden = true; done(v); };
    $("ai-ok").onclick = () => end(true); $("ai-cancel").onclick = () => end(false);
    trap($("ai"), () => end(false));
  });
  if (!ok) return;
  const agent = $("ai-agent").value, cwd = cwdIn.value.trim().replace(/[\/]+$/, "") || null, args = $("ai-args").value.trim();
  if ($("ai-def").checked) await invoke("agent_default_set", { conn, agent, args }).catch((e) => banner(msg(e), "err"));
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

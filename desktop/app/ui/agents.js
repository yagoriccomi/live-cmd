// Tela "Agentes de IA": sessões de agente por pasta raiz, com estado, Retomar (--resume) e nova sessão.
// Usa os globais de terminal.js (invoke, $, el, LOCAL, conns, remoteOrder, size, openSession, refresh, newAi, ask, banner, msg).
(() => {
  const panel = $("agents-panel"), nav = $("nav-agents");
  if (!panel || !nav) return;
  // Estado → [classe, marcador, texto]; o texto vai por extenso (o marcador é só visual).
  const STATUS = {
    rodando: ["dot-ok", "●", "Rodando"],
    saiu: ["dot-warn", "◐", "Programa terminou"],
    encerrada: ["muted", "○", "Encerrada"],
    desconhecido: ["dot-err", "⚠", "PC sem resposta"],
  };
  const isLive = (x) => x.status === "rodando" || x.status === "saiu";
  let conn = LOCAL, opener = null;

  const when = (ms) => {
    if (!ms) return "";
    const min = Math.round((Date.now() - ms) / 60000);
    if (min < 1) return "agora";
    if (min < 60) return `há ${min} min`;
    if (min < 1440) return `há ${Math.round(min / 60)} h`;
    return new Date(ms).toLocaleDateString("pt-BR");
  };
  const plural = (n, um, varios) => `${n} ${n === 1 ? um : varios}`;

  function connOptions() {
    const ids = [LOCAL, ...remoteOrder.filter((id) => conns[id]?.status.state === "ready")];
    if (!ids.includes(conn)) conn = LOCAL;
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
    const n = v.groups.reduce((a, g) => a + g.sessions.length, 0);
    $("ag-state").textContent = !v.online ? "O PC não respondeu; o estado das sessões é desconhecido."
      : n ? `${plural(n, "sessão", "sessões")} em ${plural(v.groups.length, "pasta", "pastas")}.` : "";
    if (!n) {
      const e = el("div", "ag-empty");
      e.append(el("p", null, "Nenhuma sessão de agente ainda. Crie uma para conversar com claude, openclaude ou codex numa pasta; ela fica aqui para retomar depois."));
      $("ag-list").replaceChildren(e);
      return;
    }
    $("ag-list").replaceChildren(...v.groups.map(group));
  }

  function group(g, i) {
    const s = el("section", "ag-group"), h = el("h3", null, g.name);
    h.id = `ag-g${i}`;
    s.setAttribute("aria-labelledby", h.id);
    const ul = el("ul", "ag-rows");
    ul.append(...g.sessions.map(row));
    s.append(h, el("p", "muted small mono ag-path", g.path || "Pasta inicial do perfil"), ul);
    return s;
  }

  function row(x) {
    const li = el("li", "ag-row");
    const [cls, mark, label] = STATUS[x.status] || STATUS.desconhecido;
    const dot = el("span", `${cls} ag-dot`, mark);
    dot.setAttribute("aria-hidden", "true");
    const name = x.label || x.agent;
    const info = el("div", "ag-info");
    info.append(el("span", "ag-label", name),
      el("span", "muted small", [x.agent + (x.args ? " " + x.args : ""), label, when(x.used_ms)].filter(Boolean).join(" · ")));
    const go = el("button", isLive(x) ? null : "btn-primary", isLive(x) ? "Abrir" : "Retomar");
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
  $("ag-conn").onchange = () => { conn = $("ag-conn").value; load(); };
  $("ag-new").onclick = () => { hide(false); newAi(conn); };
  // Terminal ganhou o foco (aba escolhida, sessão aberta por outro caminho): a tela sai da frente.
  $("screen").addEventListener("focus", () => { if (!panel.hidden) hide(false); });
  panel.onkeydown = (e) => { if (e.key === "Escape") { e.preventDefault(); hide(); } };
})();

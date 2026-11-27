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

  const pickIcon = (agent) => pickAgentIcon(conn, agent, load);
  // Configurações > Agentes (025): um por agente conhecido, ícone + "Trocar imagem" (vale para este PC).
  const cfg = $("cfg-agents-list");
  const paintCfg = () => cfg?.replaceChildren(...Object.entries(AGENT_NAME).map(([a, n]) => {
    const li = el("li"), b = el("button", null, "Trocar imagem");
    b.type = "button"; b.setAttribute("aria-label", `Trocar imagem de ${n}`);
    b.onclick = () => pickAgentIcon(LOCAL, a, paintCfg);
    const name = el("span", "inline"); name.append(agentIcon(LOCAL, a), el("span", null, n));
    li.append(name, b);
    return li;
  }));
  setTimeout(paintCfg); // AGENT_NAME é declarado mais abaixo no arquivo

  // Só PNG, JPEG ou WebP até 48 KB (o host confere de novo); fora disso avisa e não muda nada.
  function pickAgentIcon(conn, agent, done) {
    const f = Object.assign(document.createElement("input"), { type: "file", accept: "image/png,image/jpeg,image/webp" });
    f.onchange = async () => {
      const file = f.files[0];
      if (!file) return;
      if (!/^image\/(png|jpeg|webp)$/.test(file.type)) return banner("Ícone precisa ser PNG, JPEG ou WebP.", "err");
      if (file.size > 48000) return banner("Ícone grande demais: máximo 48 KB.", "err");
      const b = new Uint8Array(await file.arrayBuffer());
      let s = ""; for (const c of b) s += String.fromCharCode(c);
      try {
        await invoke("agent_icon_set", { conn, agent, image: btoa(s) });
        agentIcons.delete(conn + "|" + agent);
        done();
      } catch (e) { banner(e === "INVALID_MESSAGE" ? "Imagem recusada pelo host: use PNG, JPEG ou WebP até 48 KB." : msg(e), "err"); }
    };
    f.click();
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
    // Ícone do agente: clique envia uma imagem própria, salva no host (vale para desktop e celular).
    const ico = el("button", "ag-ico-btn");
    ico.type = "button";
    ico.title = `Trocar ícone de ${x.agent}`;
    ico.setAttribute("aria-label", ico.title);
    ico.append(agentIcon(conn, x.agent));
    ico.onclick = () => pickIcon(x.agent);
    const act = el("span", "inline");
    act.append(go, rm);
    li.append(dot, ico, info, act);
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
// Caixa com sugestões (020): lista abaixo enquanto se digita, resto da 1ª sugestão em opacidade baixa dentro da caixa,
// Tab completa (+ `tail` para seguir), seta para baixo entra na lista, Enter escolhe, Esc fecha. `get(v)` → lista.
function combo(input, ac, get, tail = "") {
  let seq = 0, sel = -1, items = [];
  // newAi roda a cada abertura do diálogo: reaproveita a sugestão em sombra já criada.
  let ghost = input.nextElementSibling;
  if (!ghost?.classList.contains("ac-ghost")) { ghost = el("span", "ac-ghost mono"); ghost.setAttribute("aria-hidden", "true"); input.after(ghost); }
  input.parentElement.style.position ||= "relative";
  const paint = () => {
    ac.replaceChildren(...items.map((v, i) => {
      const o = el("div", "ac-item" + (i === sel ? " on" : ""), v); o.id = ac.id + "-" + i;
      o.setAttribute("role", "option"); o.setAttribute("aria-selected", i === sel);
      o.onmousedown = (e) => { e.preventDefault(); take(v); }; return o;
    }));
    ac.hidden = !items.length; input.setAttribute("aria-expanded", !ac.hidden);
    sel >= 0 ? input.setAttribute("aria-activedescendant", ac.id + "-" + sel) : input.removeAttribute("aria-activedescendant");
    const v = input.value, g = items[Math.max(sel, 0)] || "";
    ghost.replaceChildren();
    if (document.activeElement === input && g.toLowerCase().startsWith(v.toLowerCase()) && g.length > v.length)
      ghost.append(el("span", "ac-typed", v), g.slice(v.length));
    const cs = getComputedStyle(input);
    Object.assign(ghost.style, { left: input.offsetLeft + "px", top: input.offsetTop + "px", width: input.offsetWidth + "px", height: input.offsetHeight + "px",
      padding: cs.padding, border: cs.borderWidth + " solid transparent", font: cs.font });
  };
  const close = () => { seq++; items = []; sel = -1; paint(); };
  const take = (v, more) => { input.value = v + (more ? tail : ""); close(); refresh(); };
  const refresh = async () => { const n = ++seq, l = await get(input.value); if (n === seq) { items = [...new Set(l)]; sel = -1; paint(); } };
  input.oninput = refresh;
  input.onfocus = refresh;
  input.onblur = close;
  input.onkeydown = (e) => {
    if (e.key === "Tab" && !e.shiftKey && items.length) { e.preventDefault(); take(items[Math.max(sel, 0)], true); }
    else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && items.length) {
      e.preventDefault(); sel = e.key === "ArrowDown" ? (sel + 1) % items.length : (sel <= 0 ? items.length : sel) - 1;
      paint(); ac.children[sel]?.scrollIntoView({ block: "nearest" });
    }
    else if (e.key === "Enter" && sel >= 0) { e.preventDefault(); e.stopPropagation(); take(items[sel]); }
    else if (e.key === "Escape" && items.length) { e.preventDefault(); e.stopPropagation(); close(); }
  };
  return { close, refresh };
}
const AGENT_NAME = { claude: "Claude Code", openclaude: "OpenClaude", codex: "Codex", antigravity: "Antigravity" };
async function newAi(conn, dir, agent0) {
  localStorage.removeItem("ai-default"); // resto de versão antiga; o padrão agora é do host
  const [d, list] = await Promise.all([
    invoke("agent_default_get", { conn }).catch(() => null),
    invoke("agent_list", { conn }).catch(() => null),
  ]);
  const recent = [...new Set((list?.groups || []).map((g) => g.path).filter(Boolean))];
  const opt = (v, l) => { const o = el("option"); o.value = v; if (l) o.label = v + " — " + l; return o; };
  const cwdIn = $("ai-cwd"), dirs = $("ai-dirs"), pick = $("ai-pick");
  // Antigravity só aparece se o host achar `agy` no PATH (host antigo devolve array: fica oculto).
  $("ai-agent").querySelector('option[value="antigravity"]').hidden = !(await invoke("agent_args", { conn, agent: "antigravity" }).catch(() => null))?.installed;
  $("ai-agent").value = agent0 || d?.agent || "claude";
  if ($("ai-agent").selectedOptions[0]?.hidden) $("ai-agent").value = "claude";
  if (!$("ai-agent").value) $("ai-agent").value = "claude";
  cwdIn.value = dir || recent[0] || ""; $("ai-args").value = d?.args || ""; $("ai-def").checked = false;
  // Comandos extras já usados com o agente no host + opções conhecidas; texto livre continua valendo.
  const sug = async () => {
    const a = $("ai-agent").value, l = await invoke("agent_args", { conn, agent: a }).catch(() => null);
    if (a !== $("ai-agent").value) return;
    const o = Array.isArray(l?.options) ? l.options : (Array.isArray(l) ? l : AI_SUG[a] || []).map((value) => ({ value }));
    argSug = o.map((x) => x.value);
  };
  let argSug = [];
  // Trocar de agente não herda os comandos extras do anterior (flags não valem entre CLIs).
  $("ai-agent").onchange = () => { $("ai-args").value = $("ai-agent").value === d?.agent ? d?.args || "" : ""; sug(); }; sug();
  // Caminho completo embaixo do campo: o que vai para o host, sem barra final.
  const path = () => { $("ai-path").textContent = cwdIn.value.trim().replace(/[\\/]+$/, "") || "Pasta inicial do perfil (~)"; };
  // 📁 (#64): no PC local, o seletor nativo do Windows; no remoto, navegador próprio (voltar, endereço, pastas).
  const addr = $("ai-addr"), brw = $("ai-browser");
  const browse = async (p) => {
    let ds = [];
    try { ds = await invoke("term_dirs", { conn, path: p }); } catch (e) { banner(msg(e), "err"); }
    const base = p.replace(/[\\/]+$/, "");
    addr.value = base;
    const item = (name, to) => { const b = el("button", null, "📁 " + name); b.type = "button"; b.onclick = () => { cwdIn.value = to; path(); browse(to); }; return b; };
    dirs.replaceChildren(...(Array.isArray(ds) ? ds : []).map((x) => item(x, base ? base + "\\" + x : x)));
  };
  const up = (p) => { const b = p.replace(/[\\/]+$/, ""); return b.includes("\\") ? b.slice(0, b.lastIndexOf("\\")) : ""; };
  $("ai-back").onclick = () => { cwdIn.value = up(addr.value); path(); browse(cwdIn.value); };
  addr.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); cwdIn.value = addr.value.trim(); path(); browse(cwdIn.value); } };
  const show = (on) => { brw.hidden = !on; pick.setAttribute("aria-expanded", on); if (on) browse(cwdIn.value.trim()); };
  pick.onclick = async () => {
    if (conn !== LOCAL) return show(brw.hidden);
    const r = await window.__TAURI__.dialog.open({ directory: true, multiple: false, defaultPath: cwdIn.value.trim() || undefined, title: "Pasta do agente" }).catch(() => null);
    if (r) { cwdIn.value = r; path(); }
  };
  // Autocomplete (020): recentes + subpastas reais. "C:\proj" já lista o conteúdo de proj e as irmãs "proj*".
  // Tab numa pasta completa e acrescenta "\" (segue para as subpastas).
  const cwdAc = combo(cwdIn, $("ai-ac"), async (v) => {
    path();
    const lv = v.toLowerCase();
    const rec = recent.filter((r) => r.toLowerCase().startsWith(lv) && r.toLowerCase() !== lv);
    const i = v.lastIndexOf("\\");
    if (i < 0) return rec;
    const par = v.slice(0, i + 1), pre = v.slice(i + 1).toLowerCase(), base = v.replace(/[\\/]+$/, "");
    const arr = (x) => (Array.isArray(x) ? x : []);
    const [sib, kids] = await Promise.all([
      pre ? invoke("term_dirs", { conn, path: par }).catch(() => []) : [],
      invoke("term_dirs", { conn, path: v }).catch(() => []),
    ]);
    return [...rec, ...arr(sib).filter((d) => d.toLowerCase().startsWith(pre) && d.toLowerCase() !== pre).map((d) => par + d),
      ...arr(kids).map((d) => base + "\\" + d)];
  }, "\\");
  // Comandos extras: mesmo componente; completa só a última palavra, sem repetir opção já escrita.
  combo($("ai-args"), $("ai-ac2"), (v) => {
    const cut = v.lastIndexOf(" ") + 1, head = v.slice(0, cut), tok = v.slice(cut).toLowerCase();
    return argSug.filter((o) => o.toLowerCase().startsWith(tok) && o.toLowerCase() !== tok && !v.includes(o)).map((o) => head + o);
  }, " ");
  cwdIn.onchange = () => { path(); if (!brw.hidden) browse(cwdIn.value.trim()); };
  show(false); cwdAc.close(); path();
  $("ai-scrim").hidden = false; cwdIn.focus();
  const ok = await new Promise((done) => {
    const end = (v) => { $("ai-scrim").hidden = true; done(v); };
    $("ai-ok").onclick = () => end(true); $("ai-cancel").onclick = () => end(false);
    trap($("ai"), () => end(false));
  });
  if (!ok) return;
  const agent = $("ai-agent").value, cwd = cwdIn.value.trim().replace(/[\\/]+$/, "") || null, args = $("ai-args").value.trim();
  if ($("ai-def").checked) await invoke("agent_default_set", { conn, agent, args }).catch((e) => banner(msg(e), "err"));
  const p = conns[conn].profiles[0];
  if (!p) return banner(msg("NOT_FOUND"), "err");
  // Grupo = nome da pasta raiz.
  const label = `${(cwd || "").split(/[\\/]/).filter(Boolean).pop() || "~"} · ${agent}`.slice(0, 64);
  const { cols, rows } = size();
  try {
    const sid = await invoke("term_create", { conn, profileId: p.profile_id, label, cols, rows, cwd, agent, args });
    await openSession(conn, { session_id: sid, label, state: "running" });
    refresh(conn);
  } catch (e) { banner(msg(e), "err"); }
}

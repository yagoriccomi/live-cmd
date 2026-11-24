// Visual V1 Glass Aurora: trilho de telas, cartão de destaque e resumos (só desenho).
// Lê o estado de terminal.js (tabs, active, conns, remoteOrder, dropCwds, sessionKind) e o DOM que
// agents.js, ssh.js e historico.js já desenham. Não chama comando novo: os botões do destaque repassam
// o clique ao botão original da lista.
(() => {
  const PANELS = [["agents-panel", "nav-agents", "ag-x"], ["ssh-panel", "ssh-open", "ssh-x"], ["hist", "hist-open", "hist-x"],
    ["pair-pane", "nav-pair", "pp-x"], ["cfg-pane", "nav-settings", "cfg-x"]];
  const plural = (n, um, varios) => `${n} ${n === 1 ? um : varios}`;
  const txt = (root, sel) => root?.querySelector(sel)?.textContent.trim() || "";

  // ---------- Peças ----------
  function hero(box, { icon, title, sub, badge, tone, note, buttons = [] }) {
    if (!title) { box.replaceChildren(); box.hidden = true; return; }
    box.hidden = false;
    const top = el("div", "hero-top");
    const i = el("span", "tile tile-lg", icon);
    i.setAttribute("aria-hidden", "true");
    const t = el("div", "hero-txt");
    t.append(el("div", "hero-t", title), el("div", "hero-s mono", sub || ""));
    top.append(i, t);
    if (badge) top.append(el("span", "badge" + (tone ? " " + tone : ""), badge));
    const parts = [top];
    if (note) parts.push(el("p", "hero-n", note));
    if (buttons.length) {
      const row = el("div", "term-tools");
      row.append(...buttons.map((src) => {
        const b = el("button", null, src.textContent);
        b.type = "button";
        b.disabled = src.disabled;
        const label = src.getAttribute("aria-label");
        if (label) b.setAttribute("aria-label", label);
        b.onclick = () => src.click();
        return b;
      }));
      parts.push(row);
    }
    box.replaceChildren(...parts);
  }
  function stats(box, items) {
    box.replaceChildren(...items.map(([k, v, s]) => {
      const d = el("div", "stat");
      d.append(el("div", "stat-k", k), el("div", "stat-v", String(v)), el("div", "stat-s", s || ""));
      return d;
    }));
  }
  const setStat = (n, v, s) => { $(`st-${n}v`).textContent = v; $(`st-${n}s`).textContent = s; };

  // ---------- Terminal ----------
  window.auroraTerm = () => {
    const all = [...conns[LOCAL].sessions, ...remoteOrder.flatMap((id) => conns[id]?.sessions || [])];
    const live = all.filter((s) => s.state !== "exited");
    const agents = live.filter((s) => sessionKind(s.label) === "agent").length;
    const ssh = live.filter((s) => sessionKind(s.label) === "ssh").length;
    const pcs = remoteOrder.filter((id) => conns[id]?.status.state === "ready").length;
    $("side-sub").textContent = `${plural(live.length, "sessão aberta", "sessões abertas")} · ${plural(agents, "agente", "agentes")}`;
    setStat(3, live.length, `${plural(agents, "agente", "agentes")} · ${ssh} SSH · ${pcs}/${remoteOrder.length} PCs`);
    const t = tabs.find((x) => x.key === active);
    const i = tabs.indexOf(t);
    $("hero-i").textContent = KIND_ICON[sessionKind(t?.label || t?.title)];
    $("hero-t").textContent = t ? t.title : "Nenhuma sessão aberta";
    $("hero-s").textContent = t ? [dropCwds[t.key], `aba ${i + 1}`].filter(Boolean).join(" · ") : "Escolha uma sessão à esquerda ou crie uma nova.";
    const b = $("hero-b");
    b.hidden = !t;
    b.textContent = t?.exited != null ? "ENCERRADA" : "RODANDO";
    b.className = "badge" + (t?.exited != null ? " warn" : "");
  };

  // Filtro da lista lateral (Todas / Agentes / SSH / Shell).
  $("side-chips").onclick = (e) => {
    const c = e.target.closest("button.chip");
    if (!c) return;
    sideFilter = c.dataset.f;
    for (const x of $("side-chips").children) x.setAttribute("aria-pressed", String(x === c));
    renderSidebar();
  };

  // ---------- Trilho ----------
  function railCurrent() {
    const open = PANELS.find(([p]) => !$(p).hidden);
    for (const b of document.querySelectorAll(".rail-btn")) b.removeAttribute("aria-current");
    $(open ? open[1] : "nav-term").setAttribute("aria-current", "page");
  }
  $("nav-term").onclick = () => {
    for (const [p, , x] of PANELS) if (!$(p).hidden) $(x).click();
    $("screen").focus();
  };
  // #62: hover no item com painel abre o painel por cima do terminal; clique fixa; sair fecha com folga para o vão.
  // Itens sem painel (celular, dispositivos, ajustes) só navegam no clique.
  let peek = null, peekT = 0;
  const unpeek = () => { clearTimeout(peekT); peekT = setTimeout(() => { if (peek && !$(peek[0]).hidden) $(peek[2]).click(); peek = null; }, 400); };
  $("nav-term").closest(".rail").addEventListener("mouseenter", () => clearTimeout(peekT));
  $("nav-term").closest(".rail").addEventListener("mouseleave", () => peek && unpeek());
  for (const p of PANELS.slice(0, 3)) {
    const b = $(p[1]), pane = $(p[0]).querySelector(".pane-l") || $(p[0]);
    b.addEventListener("mouseenter", () => {
      clearTimeout(peekT);
      if (peek === p || PANELS.some(([q]) => !$(q).hidden && q !== peek?.[0])) return;
      if (peek && !$(peek[0]).hidden) $(peek[2]).click();
      if ($(p[0]).hidden) { b.click(); peek = p; }
    });
    pane.addEventListener("mouseenter", () => peek === p && clearTimeout(peekT));
    pane.addEventListener("mouseleave", () => peek === p && unpeek());
    // Clique real com o painel em prévia: fixa em vez de fechar.
    b.addEventListener("click", (e) => { if (e.isTrusted && peek === p) { peek = null; clearTimeout(peekT); e.stopImmediatePropagation(); } }, true);
  }
  // Celular, Dispositivos e Ajustes abrem painéis desta janela (app.js expõe window.liveTab).
  for (const [id, act] of [["nav-pair", "pair"], ["nav-devices", "devices"], ["nav-settings", "settings"]])
    $(id).onclick = () => window.liveTab?.(act);

  // CPU e memória do PC (comando sys_stats, Win32). A cada 3 s, só com a janela visível.
  const gb = (b) => (b / 2 ** 30).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + " GB";
  async function sysStats() {
    if (document.hidden) return;
    const conn = activeTab()?.conn || LOCAL;
    if (conns[conn]?.status?.state !== "ready") return;
    // Aba SSH: CPU/memória do servidor (Linux); outro SO mostra "—" em vez dos números deste PC.
    const ssh = sshOf(activeTab());
    const v = await invoke(ssh ? "ssh_stats" : "sys_stats", ssh ? { target: ssh } : { conn }).catch(() => null);
    if (!v) { if (ssh) { setStat(1, "—", "servidor"); setStat(2, "—", ""); } return; }
    if (v.cpu != null) setStat(1, Math.round(v.cpu) + "%", v.cores ? plural(v.cores, "núcleo", "núcleos") : "");
    if (v.mem_total) setStat(2, gb(v.mem_used), "de " + Math.round(v.mem_total / 2 ** 30) + " GB");
    if (v.host && $("pp-host")) $("pp-host").textContent = v.host;
  }
  sysStats();
  setInterval(sysStats, 3000);

  // ---------- Agentes ----------
  const AG_BADGE = { "dot-wait": ["ESPERANDO", "warn"], "dot-ok": ["RODANDO", ""], "dot-warn": ["TERMINOU", "warn"], "dot-err": ["SEM RESPOSTA", "err"], muted: ["ENCERRADA", "off"] };
  const dotOf = (row) => Object.keys(AG_BADGE).find((c) => row.querySelector(".ag-dot")?.classList.contains(c)) || "muted";
  function agents() {
    const rows = [...$("ag-list").querySelectorAll(".ag-row")];
    const pick = rows.find((r) => dotOf(r) === "dot-wait") || rows.find((r) => dotOf(r) === "dot-ok") || rows[0];
    const [badge, tone] = pick ? AG_BADGE[dotOf(pick)] : [];
    hero($("ag-hero"), pick ? {
      icon: "✦", title: txt(pick, ".ag-label"), sub: pick.dataset.dir, badge, tone,
      note: txt(pick, ".ag-info .sr-only"), buttons: [...pick.querySelectorAll("button")],
    } : {});
    const run = rows.filter((r) => ["dot-ok", "dot-wait"].includes(dotOf(r))).length;
    const dirs = new Set(rows.map((r) => r.dataset.dir));
    stats($("ag-stats"), [["Rodando", run, run ? "abertas agora" : "nenhuma aberta"], ["Sessões salvas", rows.length, "--resume"], ["Pastas", dirs.size, "raiz dos projetos"]]);
    // Linha do tempo do que se retoma: um dia por bloco, mais recentes primeiro.
    const saved = rows.filter((r) => !r.dataset.live).sort((p, q) => (+q.dataset.ms || 0) - (+p.dataset.ms || 0));
    const box = $("ag-resume"), kids = [];
    let cur = null, ul = null;
    for (const r of saved) {
      const d = new Date(+r.dataset.ms || 0), t = new Date(), y = new Date();
      y.setDate(t.getDate() - 1);
      const g = d.toDateString() === t.toDateString() ? "Hoje" : d.toDateString() === y.toDateString() ? "Ontem"
        : d.toLocaleDateString("pt-BR", { day: "2-digit", month: "short" }).replace(".", "").replace(" de ", " ");
      if (g !== cur) { cur = g; ul = el("ul", "tl"); kids.push(el("h3", "hist-day", g), ul); }
      const li = el("li", "tl-row");
      li.append(el("span", "hist-time", d.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" })), el("span", "tl-dot ok"),
        el("span", "tl-c mono", txt(r, ".ag-label")), el("span", "hist-side", r.dataset.agent));
      li.querySelector(".hist-side").style.marginLeft = "auto";
      ul.append(li);
    }
    box.replaceChildren(...kids);
  }

  // ---------- SSH ----------
  function ssh() {
    const rows = [...$("ssh-list").children], sess = [...$("ssh-sess").children];
    const pick = rows.find((r) => r.classList.contains("on")) || rows[0];
    const st = pick?.querySelector(".ssh-st");
    const isOk = !!pick?.querySelector(".dot-ok");
    const tone = !st ? "" : isOk ? "" : pick.querySelector(".dot-err") ? "err" : "off";
    const want = ["Conectar", "Editar", "Copiar comando"];
    hero($("ssh-hero"), pick ? {
      icon: "⇄", title: txt(pick, ".ssh-name"), sub: pick.dataset.sub, badge: isOk ? "ONLINE" : st?.textContent.toUpperCase(), tone,
      buttons: want.map((w) => [...pick.querySelectorAll(".ssh-acts button")].find((b) => b.textContent === w)).filter(Boolean),
    } : {});
    const mine = pick ? sess.filter((r) => txt(r, ".ssh-name") === txt(pick, ".ssh-name")) : [];
    const key = pick?.dataset.key || "";
    stats($("ssh-stats"), [["Latência", pick?.dataset.ms ? pick.dataset.ms + " ms" : "–", "último teste"],
      ["Sessões", mine.length, `${mine.filter((r) => r.querySelector(".dot-ok")).length} conectadas`],
      ["Chave", key ? key.split(/[\/]/).pop() : "padrão", key || "chaves do OpenSSH do Windows"]]);
    $("ssh-sess-none").hidden = sess.length > 0;
  }

  // ---------- Histórico ----------
  function hist() {
    const items = [...$("hist-list").querySelectorAll(".hist-item")];
    const err = items.filter((i) => i.querySelector(".hist-code.err")).length;
    const ok = items.filter((i) => i.querySelector(".hist-code.ok")).length;
    const by = new Map();
    for (const i of items) {
      const [time, , label = "Terminal"] = txt(i, ".hist-meta").split(" · ");
      const g = by.get(label) || { n: 0, err: 0, time };
      g.n++; if (i.querySelector(".hist-code.err")) g.err++;
      by.set(label, g);
    }
    const q = $("hist-q").value.trim(), first = items[0];
    hero($("hist-hero"), first ? {
      icon: "⌕", title: q || txt(first, ".hist-cmd"), sub: `${plural(items.length, "comando", "comandos")} em ${plural(by.size, "sessão", "sessões")}`,
      badge: "CTRL+SHIFT+F", tone: "key",
      buttons: [Object.assign(el("button", null, "Copiar"), { onclick: () => first.querySelector(".hist-run")?.click() })],
    } : {});
    stats($("hist-stats"), [["Sucesso", ok, items.length ? `${Math.round((ok * 100) / items.length)}%` : ""], ["Erros", err, "saída ≠ 0"], ["Sessões", by.size, "no histórico"]]);
    if (window.histSessions) return histSessions();
    $("hist-sess").replaceChildren(...[...by].map(([label, g]) => {
      const li = el("li", "card"), k = sessionKind(label);
      li.dataset.kind = k;
      const t = el("span", "card-txt");
      t.append(el("span", "card-t", label), el("span", "card-s mono", g.time || ""));
      const n = el("span", "card-st " + (g.err ? "dot-err" : "dot-ok"), `● ${g.n}`);
      n.setAttribute("aria-label", `${plural(g.n, "comando", "comandos")}${g.err ? `, ${g.err} com erro` : ""}`);
      const i = el("span", "tile", KIND_ICON[k]);
      i.setAttribute("aria-hidden", "true");
      li.append(i, t, n);
      return li;
    }));
  }

  // Uma linha por sessão e dia (hist_sessions), do dia mais recente para o mais antigo.
  const dayName = (d) => {
    const iso = (t) => new Date(t - new Date(t).getTimezoneOffset() * 60000).toISOString().slice(0, 10);
    const now = Date.now();
    if (d === iso(now)) return "Hoje";
    if (d === iso(now - 86400000)) return "Ontem";
    const [y, m, dd] = d.split("-").map(Number);
    return new Date(y, m - 1, dd).toLocaleDateString("pt-BR", { day: "2-digit", month: "short" });
  };
  function histSessions() {
    const g = window.histSessions || [];
    const s = $("hist-stats").querySelectorAll(".stat")[2];
    if (s) { s.querySelector(".stat-v").textContent = new Set(g.map((x) => x.label)).size; s.querySelector(".stat-s").textContent = `${plural(g.length, "dia de uso", "dias de uso")}`; }
    $("hist-sess").replaceChildren(...g.map((x) => {
      const li = el("li", "card"), k = x.kind === "ssh" ? "ssh" : sessionKind(x.label);
      li.dataset.kind = k;
      const t = el("span", "card-txt");
      t.append(el("span", "card-t", x.label), el("span", "card-s mono", `${dayName(x.day).toLowerCase()} ${new Date(+x.last_ms).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" })}`));
      const n = el("span", "card-st " + (x.errors ? "dot-err" : "dot-ok"), `● ${x.count}`);
      n.setAttribute("aria-label", `${plural(x.count, "comando", "comandos")}${x.errors ? `, ${x.errors} com erro` : ""} em ${dayName(x.day)}`);
      const i = el("span", "tile", KIND_ICON[k] || KIND_ICON.shell);
      i.setAttribute("aria-hidden", "true");
      li.append(i, t, n);
      return li;
    }));
  }
  document.addEventListener("hist-sessions", () => { if (window.histSessions) histSessions(); });

  const watch = (ids, fn) => { const o = new MutationObserver(fn); for (const id of ids) o.observe($(id), { childList: true, subtree: true, characterData: true }); };
  watch(["ag-list"], agents);
  watch(["ssh-list", "ssh-sess"], ssh);
  watch(["hist-list"], hist);
  new MutationObserver(railCurrent).observe($("layout"), { attributes: true, attributeFilter: ["hidden"], subtree: true });
  railCurrent();
  auroraTerm();
})();

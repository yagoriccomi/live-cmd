// Visual V1 Glass Aurora: trilho de telas, cartão de destaque e resumos (só desenho).
// Lê o estado de terminal.js (tabs, active, conns, remoteOrder, dropCwds, sessionKind) e o DOM que
// agents.js, ssh.js e historico.js já desenham. Não chama comando novo: os botões do destaque repassam
// o clique ao botão original da lista.
(() => {
  const PANELS = [["agents-panel", "nav-agents", "ag-x"], ["ssh-panel", "ssh-open", "ssh-x"], ["hist", "hist-open", "hist-x"]];
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
    setStat(1, live.length, `${plural(agents, "agente", "agentes")} · ${ssh} SSH`);
    setStat(2, agents, agents ? "rodando agora" : "nenhum rodando");
    setStat(3, pcs, `de ${remoteOrder.length} pareados`);
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
  for (const [id, act] of [["nav-pair", "pair"], ["nav-settings", "settings"]])
    $(id).onclick = () => invoke("tray_action", { id: act }).catch((err) => banner(msg(err), "err"));

  // ---------- Agentes ----------
  const AG_BADGE = { "dot-ok": ["RODANDO", ""], "dot-warn": ["TERMINOU", "warn"], "dot-err": ["SEM RESPOSTA", "err"], muted: ["ENCERRADA", "off"] };
  const dotOf = (row) => Object.keys(AG_BADGE).find((c) => row.querySelector(".ag-dot")?.classList.contains(c)) || "muted";
  function agents() {
    const rows = [...$("ag-list").querySelectorAll(".ag-row")], groups = [...$("ag-list").querySelectorAll(".ag-group")];
    const pick = rows.find((r) => dotOf(r) === "dot-ok") || rows[0];
    const [badge, tone] = pick ? AG_BADGE[dotOf(pick)] : [];
    hero($("ag-hero"), pick ? {
      icon: "✦", title: txt(pick, ".ag-label"), sub: txt(pick.closest(".ag-group"), ".ag-path"), badge, tone,
      note: txt(pick, ".ag-info .small"), buttons: [...pick.querySelectorAll("button")],
    } : {});
    const run = rows.filter((r) => dotOf(r) === "dot-ok").length;
    stats($("ag-stats"), [["Rodando", run, run ? "abertas agora" : "nenhuma aberta"], ["Sessões salvas", rows.length, "--resume"], ["Pastas", groups.length, "raiz dos projetos"]]);
    $("ag-dirs").replaceChildren(...groups.map((g) => {
      const li = el("li", "tl-row");
      li.append(el("span", "tl-dot ok"), el("span", "tl-c mono", txt(g, "h3")), el("span", "tl-m", txt(g, ".ag-path")), el("span", "tl-r", plural(g.querySelectorAll(".ag-row").length, "sessão", "sessões")));
      return li;
    }));
  }

  // ---------- SSH ----------
  function ssh() {
    const rows = [...$("ssh-list").children], sess = [...$("ssh-sess").children];
    const pick = rows[0];
    const st = pick?.querySelector(".ssh-st");
    const tone = !st ? "" : pick.querySelector(".dot-ok") ? "" : pick.querySelector(".dot-err") ? "err" : "off";
    hero($("ssh-hero"), pick ? {
      icon: "⇄", title: txt(pick, ".ssh-name"), sub: txt(pick, ".ssh-dest"), badge: st?.textContent.toUpperCase(), tone,
      buttons: [...pick.querySelectorAll(".ssh-acts button")].slice(0, 3),
    } : {});
    const on = rows.filter((r) => r.querySelector(".dot-ok")).length;
    const conn = sess.filter((r) => r.querySelector(".dot-ok")).length;
    stats($("ssh-stats"), [["Servidores", rows.length, "salvos neste PC"], ["Online", on, "responderam ao teste"], ["Sessões", sess.length, `${conn} conectadas`]]);
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
      badge: "CTRL+SHIFT+F", tone: "key", buttons: [first.querySelector(".hist-run")].filter(Boolean),
    } : {});
    stats($("hist-stats"), [["Sucesso", ok, items.length ? `${Math.round((ok * 100) / items.length)}%` : ""], ["Erros", err, "saída ≠ 0"], ["Sessões", by.size, "no histórico"]]);
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

  const watch = (ids, fn) => { const o = new MutationObserver(fn); for (const id of ids) o.observe($(id), { childList: true, subtree: true, characterData: true }); };
  watch(["ag-list"], agents);
  watch(["ssh-list", "ssh-sess"], ssh);
  watch(["hist-list"], hist);
  new MutationObserver(railCurrent).observe($("layout"), { attributes: true, attributeFilter: ["hidden"], subtree: true });
  railCurrent();
  auroraTerm();
})();

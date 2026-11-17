// Tela Histórico: comandos gravados pelo Rust (hist.rs) em history.jsonl, só neste PC.
// Usa os globais de terminal.js ($, el, invoke, banner, msg, ask, activeTab, conns, LOCAL).
// IIFE: funções daqui não vazam para o global (open/close sobrescreveriam as de window).
(() => {
  const KIND = { pwsh: "PowerShell", cmd: "Prompt de Comando", wsl: "WSL / bash", ssh: "SSH", outro: "Outra" };
  let timer = null, opener = null;

  // Rótulo e PC da aba, gravados junto de cada comando.
  window.histMeta = (conn, s) => {
    const pc = conn === LOCAL ? "Este PC" : conns[conn]?.name || "Outro PC";
    invoke("hist_meta", { conn, sessionId: s.session_id, label: s.label || "Terminal", pc }).catch(() => {});
  };

  const dayKey = (d) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  function groupName(ts) {
    const d = new Date(ts), today = new Date(), y = new Date(); y.setDate(today.getDate() - 1);
    if (dayKey(d) === dayKey(today)) return "Hoje";
    if (dayKey(d) === dayKey(y)) return "Ontem";
    return d.toLocaleDateString("pt-BR", { weekday: "long", day: "2-digit", month: "long", year: "numeric" });
  }

  /** Texto com as ocorrências de `q` em <mark> (sem innerHTML). */
  function highlight(text, q) {
    const f = document.createDocumentFragment();
    if (!q) { f.append(text); return f; }
    const low = text.toLowerCase(), n = q.toLowerCase();
    let i = 0;
    for (let j = low.indexOf(n); j >= 0; j = low.indexOf(n, i)) {
      f.append(text.slice(i, j), el("mark", null, text.slice(j, j + n.length)));
      i = j + n.length;
    }
    f.append(text.slice(i));
    return f;
  }

  function item(e, q) {
    const li = el("li", "hist-item");
    const cmd = el("code", "hist-cmd mono");
    cmd.append(highlight(e.cmd, q));
    cmd.title = e.cmd;
    const time = new Date(e.ts).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
    const meta = el("span", "hist-meta muted small");
    meta.append(highlight(`${time} · ${e.pc} · ${e.label}`, q));
    const badge = el("span", "hist-kind", KIND[e.kind] || e.kind);
    const code = e.code == null ? null : el("span", e.code === 0 ? "hist-code ok" : "hist-code err", e.code === 0 ? "ok" : `erro ${e.code}`);
    if (code) code.setAttribute("aria-label", e.code === 0 ? "terminou sem erro" : `terminou com código ${e.code}`);
    const run = el("button", "hist-run", "Reexecutar");
    run.type = "button";
    run.title = "Enviar à aba ativa e executar";
    run.setAttribute("aria-label", `Reexecutar ${e.cmd}`);
    run.onclick = () => rerun(e.cmd);
    const top = el("div", "hist-row");
    top.append(cmd, run);
    const bottom = el("div", "hist-row");
    bottom.append(meta, badge);
    if (code) bottom.append(code);
    li.append(top, bottom);
    return li;
  }

  async function load() {
    const q = $("hist-q").value.trim();
    const query = { q, days: $("hist-7d").checked ? 7 : null, kind: $("hist-kind").value || null, errors: $("hist-err").checked, limit: 1000 };
    let list;
    try { list = await invoke("hist_list", { query }); } catch (e) { $("hist-n").textContent = msg(e); return; }
    const box = $("hist-list");
    box.replaceChildren();
    const filtered = q || query.days || query.kind || query.errors;
    $("hist-n").textContent = list.length ? `${list.length} comando${list.length > 1 ? "s" : ""}${list.length >= 1000 ? " (os 1000 mais recentes)" : ""}` : "";
    $("hist-clear").disabled = !list.length && !filtered;
    if (!list.length) {
      box.append(el("p", "hist-empty muted", filtered ? "Nenhum comando encontrado com esses filtros." : "Nenhum comando gravado ainda. Os comandos digitados nos terminais aparecem aqui."));
      return;
    }
    let ul = null, cur = null;
    for (const e of list) {
      const g = groupName(e.ts);
      if (g !== cur) {
        cur = g;
        const h = el("h3", "hist-day", g);
        ul = el("ul", "hist-ul");
        box.append(h, ul);
      }
      ul.append(item(e, q));
    }
  }

  function rerun(cmd) {
    const t = activeTab();
    if (!t || t.exited != null) return banner("Abra uma aba de terminal para reexecutar o comando.", "warn");
    close(false);
    invoke("term_input", { conn: t.conn, sessionId: t.sid, data: cmd + "\r" }).catch((e) => banner(msg(e), "err"));
    $("screen").focus();
  }

  function open() {
    opener = document.activeElement;
    $("hist").hidden = false;
    $("hist-open").setAttribute("aria-expanded", "true");
    $("hist-q").focus();
    load();
  }
  function close(refocus = true) {
    if ($("hist").hidden) return;
    $("hist").hidden = true;
    $("hist-open").setAttribute("aria-expanded", "false");
    if (refocus) (opener && document.contains(opener) ? opener : $("screen")).focus();
  }

  $("hist-open").onclick = () => ($("hist").hidden ? open() : close());
  $("hist-x").onclick = () => close();
  $("hist").addEventListener("keydown", (e) => { if (e.key === "Escape") { e.preventDefault(); close(); } });
  $("hist-q").oninput = () => { clearTimeout(timer); timer = setTimeout(load, 150); };
  for (const id of ["hist-7d", "hist-kind", "hist-err"]) $(id).onchange = load;
  $("hist-clear").onclick = async () => {
    if (!await ask("Limpar histórico?", "Todos os comandos gravados neste PC serão apagados. Não dá para desfazer.", "Limpar histórico")) return;
    try { await invoke("hist_clear"); } catch (e) { return banner(msg(e), "err"); }
    load();
  };
  // Abrir uma sessão pela barra lateral volta ao terminal.
  $("sidebar").addEventListener("click", (e) => { if (e.target.closest("li > button")) close(false); });
})();

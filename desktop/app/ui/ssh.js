// Servidores SSH: lista salva (ssh.json), busca, adicionar/editar/remover, status por alcance TCP e
// "Conectar" (nova aba local com o ssh.exe do Windows). Nunca pede nem guarda senha.
// Usa os globais de terminal.js (invoke, $, el, banner, msg, size, openSession, refresh, LOCAL, trap, ask, menu).
(() => {
  const SSH_MSG = {
    INVALID_SSH_NAME: "Dê um nome de 1 a 40 caracteres.",
    INVALID_SSH_USER: "Usuário inválido: use só letras, números, ponto, hífen ou sublinhado, sem espaços.",
    INVALID_SSH_HOST: "Host inválido: use um nome ou IP, sem espaços nem caracteres especiais.",
    INVALID_SSH_PORT: "Porta inválida: use um número de 1 a 65535.",
    INVALID_SSH_KEEPALIVE: "Manter vivo: use 0 (desligado) ou de 5 a 3600 segundos.",
    INVALID_SSH_KEY: "Chave inválida: informe o caminho completo do arquivo (ex.: C:\\Users\\voce\\.ssh\\id_ed25519).",
    LIMIT_EXCEEDED: "Limite de 200 servidores atingido.",
  };
  const sshMsg = (code) => SSH_MSG[code] || msg(code);
  // Mesmas regras do host (desktop/src/ssh.rs); o Rust valida de novo ao salvar e ao conectar.
  const okHost = (h) => /^(?!-)[A-Za-z0-9._:-]{1,253}$/.test(h);
  const okUser = (u) => !u || /^(?!-)[A-Za-z0-9._-]{1,64}$/.test(u);
  const dest = (s) => (s.user ? `${s.user}@` : "") + s.host;
  const okKey = (k) => /^([A-Za-z]:[\\/]|\\\\.)/.test(k) && !/["\u0000-\u001f\u007f]/.test(k);

  let list = [];
  const status = {}; // id → { ok, ms?, code? } | "testing"
  let opener = null;
  let selected = null; // id do servidor em destaque (lista à esquerda, cartão e Configuração à direita)
  let filter = "all";  // all | on | off

  // ---------- Painel ----------
  function openPanel() {
    $("ssh-panel").hidden = false;
    document.querySelector(".term-area").hidden = true;
    $("ssh-open").setAttribute("aria-expanded", "true");
    load();
    $("ssh-q").focus();
  }
  function closePanel(focus = true) {
    if ($("ssh-panel").hidden) return;
    $("ssh-panel").hidden = true;
    document.querySelector(".term-area").hidden = false;
    $("ssh-open").setAttribute("aria-expanded", "false");
    if (focus) $("screen").focus();
  }
  $("ssh-open").onclick = () => ($("ssh-panel").hidden ? openPanel() : closePanel());
  $("ssh-x").onclick = () => closePanel();
  $("ssh-panel").addEventListener("keydown", (e) => { if (e.key === "Escape" && $("ssh-scrim").hidden) { e.preventDefault(); closePanel(); } });
  // Escolher uma aba ou sessão volta ao terminal.
  for (const id of ["tabs", "local-ul", "remote-ul"]) $(id).addEventListener("click", () => closePanel(false), true);

  async function load() {
    try { list = await invoke("ssh_list"); } catch (e) { list = []; banner(sshMsg(e), "err"); }
    render();
    probeAll();
    loadSessions();
  }

  // ---------- Sessões SSH (conectadas e desconectadas) ----------
  async function loadSessions() {
    let sess = [];
    try { sess = await invoke("ssh_sessions"); } catch { sess = []; }
    $("ssh-sess-box").hidden = !sess.length;
    $("ssh-sess").replaceChildren(...sess.map((x) => {
      const li = el("li");
      const on = x.state === "connected";
      const d = el("span", on ? "dot-ok" : "dot-warn", on ? "●" : "○");
      d.setAttribute("aria-hidden", "true");
      const info = el("div", "ssh-info");
      info.append(el("div", "ssh-name", x.label || "SSH"), el("div", "muted small", on ? "conectada" : "desconectada"));
      const b = el("button", on ? null : "btn-primary", on ? "Abrir" : "Reconectar");
      b.type = "button";
      b.setAttribute("aria-label", `${b.textContent} ${x.label || "SSH"}`);
      b.onclick = () => (on ? open(x) : reconnect(x, b));
      const acts = el("div", "ssh-acts");
      acts.append(b);
      li.append(d, info, acts);
      return li;
    }));
  }
  async function open(x) {
    closePanel(false);
    await openSession(LOCAL, { session_id: x.session_id, label: x.label, state: "running" });
  }
  async function reconnect(x, b) {
    b.disabled = true;
    const { cols, rows } = size();
    try {
      await invoke("ssh_reconnect", { sessionId: x.session_id, cols, rows });
      // A aba antiga mostrava a sessão encerrada: anexa de novo do começo.
      const key = LOCAL + "/" + x.session_id;
      if (tabs.some((t) => t.key === key)) await closeTab(key);
      closePanel(false);
      await openSession(LOCAL, { session_id: x.session_id, label: x.label, state: "running" });
      refresh(LOCAL);
    } catch (e) { banner(sshMsg(e), "err"); b.disabled = false; }
  }

  function statusText(id) {
    const s = status[id];
    if (!s) return ["muted", "○", "não testado"];
    if (s === "testing") return ["dot-warn", "◌", "testando…"];
    if (s.ok) return ["dot-ok", "●", `${s.ms} ms`];
    if (s.code === "OFFLINE") return ["muted", "○", "offline"];
    return ["dot-err", "⚠", s.code === "DNS" ? "host não encontrado" : s.code === "TIMEOUT" ? "sem resposta" : s.code === "OFFLINE" ? "offline" : "erro"];
  }

  function render() {
    const q = $("ssh-q").value.trim().toLowerCase();
    const isOn = (s) => status[s.id]?.ok === true;
    const shown = list.filter((s) => (!q || `${s.name} ${dest(s)}:${s.port}`.toLowerCase().includes(q))
      && (filter === "all" || (filter === "on" ? isOn(s) : !isOn(s))));
    if (!shown.some((s) => s.id === selected)) selected = shown[0]?.id ?? null;
    $("ssh-q").hidden = list.length < 8;
    $("ssh-sub").textContent = `${list.length === 1 ? "1 servidor" : `${list.length} servidores`} · ${list.filter(isOn).length} online`;
    $("ssh-list").replaceChildren(...shown.map(row));
    cfgRows(list.find((s) => s.id === selected));
    $("ssh-empty").hidden = shown.length > 0;
    $("ssh-empty").textContent = list.length ? "Nenhum servidor encontrado para essa busca." : "Nenhum servidor salvo. Clique em “+ Adicionar servidor”.";
  }
  $("ssh-q").oninput = render;
  $("ssh-chips").addEventListener("click", (e) => {
    const c = e.target.closest("button.chip[data-f]");
    if (!c) return;
    filter = c.dataset.f;
    for (const x of $("ssh-chips").querySelectorAll("[data-f]")) x.setAttribute("aria-pressed", String(x === c));
    render();
  });

  // Cartão "Configuração" do servidor em destaque: as mesmas opções do diálogo Editar, ao alcance de um toque.
  function cfgRows(s) {
    const box = $("ssh-cfg");
    $("ssh-cfg-t").hidden = box.hidden = !s;
    if (!s) return box.replaceChildren();
    const li = (...kids) => { const x = el("li", "cfg-row"); x.append(...kids); return x; };
    const retry = el("input"); retry.type = "checkbox"; retry.setAttribute("role", "switch"); retry.checked = !!s.auto_reconnect;
    const rl = el("label", "check toggle"); rl.append(retry, el("span", null, "Reconectar ao cair"));
    retry.onchange = async () => {
      try { await invoke("ssh_save", { server: { ...s, auto_reconnect: retry.checked } }); s.auto_reconnect = retry.checked; }
      catch (e) { retry.checked = !retry.checked; banner(sshMsg(e), "err"); }
    };
    const more = (label, value, fn) => { const b = el("button", "cfg-link", ""); b.type = "button"; b.append(el("span", null, label), el("span", "cfg-v", value + " ›")); b.onclick = fn; return li(b); };
    const del = el("button", "cfg-link danger", ""); del.type = "button"; del.append(el("span", null, "Excluir servidor"), el("span", "cfg-v", "›")); del.onclick = () => remove(s);
    box.replaceChildren(li(rl), more("Manter vivo", s.keepalive_s ? `${s.keepalive_s} s` : "desligado", () => edit(s)),
      more("Chave privada", s.key ? s.key.split(/[\/]/).pop() : "padrão", () => edit(s)), li(del));
  }
  // Comando equivalente para colar em outro terminal (sem senha: ela nunca é guardada).
  window.sshCommand = (s) => `ssh${s.key ? ` -i "${s.key}"` : ""}${s.port !== 22 ? ` -p ${s.port}` : ""} ${dest(s)}`;

  function row(s) {
    const li = el("li");
    li.dataset.id = s.id;
    if (s.id === selected) { li.classList.add("on"); li.setAttribute("aria-current", "true"); }
    li.addEventListener("click", (e) => { if (e.target.closest("button")) return; selected = s.id; render(); });
    const [cls, dot, label] = statusText(s.id);
    const d = el("span", cls, dot);
    d.setAttribute("aria-hidden", "true");
    const info = el("div", "ssh-info");
    info.append(el("div", "ssh-name", s.name), el("div", "muted small mono ssh-dest", dest(s) + (s.port !== 22 ? `:${s.port}` : "")));
    li.dataset.sub = `${dest(s)} : ${s.port}`;
    li.dataset.ms = status[s.id]?.ok ? String(status[s.id].ms) : "";
    li.dataset.key = s.key || "";
    const st = el("span", "muted small ssh-st", label);
    st.setAttribute("role", "status");
    const acts = el("div", "ssh-acts");
    const btn = (text, cls, fn, aria) => { const b = el("button", cls, text); b.type = "button"; b.onclick = fn; if (aria) b.setAttribute("aria-label", aria); return b; };
    acts.append(
      btn("Conectar", "btn-primary", () => connect(s), `Conectar a ${s.name}`),
      btn("Testar", null, () => probe(s), `Testar alcance de ${s.name}`),
      btn("Editar", null, () => edit(s), `Editar ${s.name}`),
      btn("Copiar comando", null, () => copyCmd(s), `Copiar comando ssh de ${s.name}`),
      btn("Remover", null, () => remove(s), `Remover ${s.name}`));
    li.append(d, info, st, acts);
    return li;
  }

  // ---------- Status (alcance TCP) ----------
  async function probe(s) {
    status[s.id] = "testing";
    render();
    status[s.id] = await invoke("ssh_probe", { host: s.host, port: s.port }).catch(() => ({ ok: false, code: "ERROR" }));
    if (!$("ssh-panel").hidden) render();
  }
  function probeAll() { for (const s of list) probe(s); }
  setInterval(() => { if (!$("ssh-panel").hidden && $("ssh-scrim").hidden) probeAll(); }, 30000);

  // ---------- Conectar ----------
  async function connect(s) {
    const { cols, rows } = size();
    try {
      const sid = await invoke("ssh_connect", { id: s.id, cols, rows });
      closePanel(false);
      await openSession(LOCAL, { session_id: sid, label: s.name, state: "running" });
      refresh(LOCAL);
    } catch (e) { banner(sshMsg(e), "err"); }
  }

  async function copyCmd(s) {
    try { await navigator.clipboard.writeText(window.sshCommand(s)); banner("Comando copiado. A senha não faz parte dele.", "ok"); }
    catch { banner("Não foi possível copiar o comando.", "err"); }
  }

  // ---------- Remover ----------
  async function remove(s) {
    if (!(await ask("Remover servidor?", `“${s.name}” (${dest(s)}) sai da lista. Abas já abertas continuam.`, "Remover"))) return;
    try { await invoke("ssh_remove", { id: s.id }); delete status[s.id]; } catch (e) { banner(sshMsg(e), "err"); }
    load();
  }

  // ---------- Adicionar / editar ----------
  const FIELDS = ["ssh-name", "ssh-user", "ssh-host", "ssh-port", "ssh-key", "ssh-alive"];
  function showErr(code, field) {
    $("ssh-err").hidden = !code;
    $("ssh-err").querySelector("p").textContent = code ? sshMsg(code) : "";
    for (const f of FIELDS) $(f).removeAttribute("aria-invalid");
    if (field) { $(field).setAttribute("aria-invalid", "true"); $(field).focus(); }
  }
  const FIELD_OF = { INVALID_SSH_NAME: "ssh-name", INVALID_SSH_USER: "ssh-user", INVALID_SSH_HOST: "ssh-host", INVALID_SSH_PORT: "ssh-port", INVALID_SSH_KEEPALIVE: "ssh-alive", INVALID_SSH_KEY: "ssh-key" };

  function edit(s) {
    opener = document.activeElement;
    const cur = s || { id: "", name: "", user: "", host: "", port: 22, key: null };
    $("ssh-t").textContent = s ? "Editar servidor" : "Adicionar servidor";
    $("ssh-name").value = cur.name; $("ssh-user").value = cur.user; $("ssh-host").value = cur.host;
    $("ssh-port").value = cur.port; $("ssh-key").value = cur.key || "";
    $("ssh-alive").value = cur.keepalive_s || 0; $("ssh-retry").checked = !!cur.auto_reconnect; $("ssh-tmux").checked = !!cur.tmux;
    showErr(null);
    $("ssh-scrim").hidden = false;
    $("ssh-name").focus();
    const end = () => { $("ssh-scrim").hidden = true; opener?.focus(); };
    trap($("ssh-dlg"), end);
    $("ssh-cancel").onclick = end;
    $("ssh-ok").onclick = async () => {
      const v = {
        id: cur.id, name: $("ssh-name").value.trim() || $("ssh-host").value.trim(), user: $("ssh-user").value.trim(), host: $("ssh-host").value.trim(),
        port: Number($("ssh-port").value || 22), key: $("ssh-key").value.trim() || null,
        keepalive_s: Number($("ssh-alive").value || 0), auto_reconnect: $("ssh-retry").checked, tmux: $("ssh-tmux").checked,
      };
      const bad = !v.name || v.name.length > 40 ? "INVALID_SSH_NAME" : !okUser(v.user) ? "INVALID_SSH_USER" : !okHost(v.host) ? "INVALID_SSH_HOST"
        : !Number.isInteger(v.port) || v.port < 1 || v.port > 65535 ? "INVALID_SSH_PORT" : v.key && !okKey(v.key) ? "INVALID_SSH_KEY"
        : !Number.isInteger(v.keepalive_s) || (v.keepalive_s !== 0 && (v.keepalive_s < 5 || v.keepalive_s > 3600)) ? "INVALID_SSH_KEEPALIVE" : null;
      if (bad) return showErr(bad, FIELD_OF[bad]);
      $("ssh-ok").disabled = true;
      try {
        const saved = await invoke("ssh_save", { server: v });
        delete status[saved.id];
        end();
        await load();
      } catch (e) { showErr(e, FIELD_OF[e]); }
      finally { $("ssh-ok").disabled = false; }
    };
  }
  $("ssh-add").onclick = () => edit(null);
  $("ssh-import").onclick = async () => {
    try {
      const n = await invoke("ssh_import");
      banner(n ? `${n} servidor(es) importado(s) de ~/.ssh/config.` : "Nada novo em ~/.ssh/config.", "ok");
      load();
    } catch (e) { banner(sshMsg(e), "err"); }
  };
  for (const f of FIELDS) $(f).addEventListener("keydown", (e) => { if (e.key === "Enter") { e.preventDefault(); $("ssh-ok").click(); } });

  $("ssh-key-pick").onclick = () => window.__TAURI__.dialog.open({ multiple: false, directory: false, title: "Chave privada SSH" })
    .then((p) => { if (typeof p === "string") $("ssh-key").value = p; })
    .catch((e) => showErr(String(e)));
})();

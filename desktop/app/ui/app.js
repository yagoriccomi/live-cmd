// Janela única com 3 abas (design §2–§5). Sem bundler: usa window.__TAURI__ (withGlobalTauri).
// Regra: nada de URI, OTK ou caminho em texto, log ou atributo. O QR chega pronto como imagem.
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
const show = (ids, on) => ids.forEach((id) => { $(id).hidden = id !== on; });
// Visual V1 Aurora: Parear/Aparelhos e Configurações são painéis da janela do terminal.
// Carregado como módulo: os nomes daqui não colidem com os de terminal.js.
const PANES = { pair: "pair-pane", devices: "pair-pane", settings: "cfg-pane" };
const OTHER = [["agents-panel", "ag-x"], ["ssh-panel", "ssh-x"], ["hist", "hist-x"]];

// ---------- Painéis ----------
let current = null; // "pair" | "settings" | null (painel fechado)
function tab(name) {
  const pane = PANES[name];
  if (!pane) return;
  for (const [p, x] of OTHER) if (!$(p).hidden) $(x).click();
  for (const p of new Set(Object.values(PANES))) $(p).hidden = p !== pane;
  const now = pane === "cfg-pane" ? "settings" : "pair";
  // Trocar de painel, fechar ou minimizar não invalida a OTK (P13): reabre a pendente; sem ela, gera outra.
  if (now === "pair" && (current !== "pair" || !expires)) pair(true);
  if (now === "pair") loadDevices();
  if (now === "settings") loadSettings();
  current = now;
  (name === "devices" ? $("dev-h") : $(now === "pair" ? "pp-x" : "cfg-x")).scrollIntoView({ block: "nearest" });
  $(now === "pair" ? "pp-x" : "cfg-x").focus();
}
function closePane() {
  if (!current) return;
  $(PANES[current]).hidden = true;
  current = null;
  $("screen")?.focus();
}
window.liveTab = tab;
$("pp-x").onclick = closePane;
$("cfg-x").onclick = closePane;
for (const p of ["pair-pane", "cfg-pane"]) $(p).addEventListener("keydown", (e) => {
  if (e.key === "Escape" && $("rm-scrim").hidden) { e.preventDefault(); closePane(); }
});
// Outro painel do trilho (Agentes, SSH, Histórico) abriu: este fecha.
new MutationObserver((recs) => {
  if (current && recs.some((r) => r.oldValue !== null && !r.target.hidden && OTHER.some(([p]) => r.target.id === p))) { $(PANES[current]).hidden = true; current = null; }
}).observe($("layout"), { attributes: true, attributeFilter: ["hidden"], attributeOldValue: true, subtree: true });
document.addEventListener("click", (e) => {
  const t = e.target.closest("[data-tab]");
  if (t) tab(t.dataset.tab);
  if (e.target.closest("[data-act=pair]")) pair();
});

// ---------- Parear ----------
let expires = null, skew = 0, known = null, ticker = null;
const now = () => Date.now() + skew;

async function pair(reuse = false) {
  show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-busy");
  $("copied").hidden = true;
  try {
    let r = reuse && await invoke("qr").catch(() => null);
    if (!r || !known) {
      // Sem OTK pendente: se um aparelho pareou enquanto a janela estava oculta, mostra "pareado" antes de gerar outra.
      const list = await invoke("devices");
      const fresh = reuse && !r && known && list.find((d) => !known.has(d.id));
      known = new Set(list.map((d) => d.id));
      if (fresh) return paired(fresh);
      r = r || await invoke("pair");
    }
    skew = r.now_ms - Date.now();
    expires = r.expires_ms;
    $("qr").src = r.qr;
    $("qr-box").classList.remove("expired");
    $("pair-live").hidden = false;
    $("pair-expired").hidden = true;
    $("pair-hint").hidden = false;
    $("copy").hidden = false;
    show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-show");
    paneState("aguardando leitura do QR Code…", "ESPERANDO", "");
    clearInterval(ticker);
    ticker = setInterval(tick, 1000);
    tick();
  } catch {
    expires = null;
    show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-error");
  }
}

function expire() {
  clearInterval(ticker);
  expires = null;
  $("qr").removeAttribute("src");
  $("qr-box").classList.add("expired");
  $("pair-live").hidden = true;
  $("pair-hint").hidden = true;
  $("copy").hidden = true;
  $("copied").hidden = true;
  $("pair-expired").hidden = false;
  $("pp-left").textContent = "–";
  paneState("o QR Code expirou", "EXPIRADO", "warn");
}

async function tick() {
  const left = Math.max(0, expires - now());
  const s = Math.ceil(left / 1000);
  $("left").textContent = $("pp-left").textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  $("bar").style.width = (left / 3000) + "%";
  $("bar").parentElement.classList.toggle("warn", s <= 60);
  const st = await invoke("status");
  if (st.pairing_expires_ms) return;
  // OTK sumiu antes do prazo: foi usada (aparelho novo) ou expirou.
  const fresh = (await invoke("devices")).find((d) => !known.has(d.id));
  if (fresh) paired(fresh); else expire();
}

function paneState(sub, badge, tone) {
  $("pp-state").textContent = sub;
  $("pp-badge").textContent = badge;
  $("pp-badge").className = "badge" + (tone ? " " + tone : "");
}

function paired(dev) {
  clearInterval(ticker);
  expires = null;
  $("done-t").textContent = `${dev.label} pareado`;
  $("pp-left").textContent = "–";
  paneState(`${dev.label} pareado`, "PAREADO", "");
  loadDevices();
  show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-done");
}

$("copy").onclick = async () => {
  try { await invoke("copy_link"); $("copied").hidden = false; } catch { expire(); }
};

// ---------- Aparelhos ----------
const fmtDate = (ms) => new Date(ms).toLocaleDateString("pt-BR", { day: "numeric", month: "short", year: "numeric" });

async function loadDevices() {
  show(["dev-busy", "dev-list", "dev-empty", "dev-error"], "dev-busy");
  try {
    const list = await invoke("devices");
    const ul = $("dev-ul");
    ul.replaceChildren(...list.map((d) => {
      const li = document.createElement("li");
      li.className = "device-row";
      const info = document.createElement("div");
      const name = document.createElement("strong");
      name.textContent = d.label;
      const status = document.createElement("div");
      status.className = "status muted";
      if (d.connected) {
        const dot = document.createElement("span");
        dot.className = "dot-ok"; dot.setAttribute("aria-hidden", "true"); dot.textContent = "● ";
        status.append(dot, `Conectado · pareado em ${fmtDate(d.paired_at_ms)}`);
      } else status.textContent = `Desconectado · pareado em ${fmtDate(d.paired_at_ms)}`;
      info.append(name, status);
      const btn = document.createElement("button");
      btn.type = "button";
      btn.textContent = "Remover";
      btn.setAttribute("aria-label", `Remover ${d.label}`);
      btn.onclick = () => askRemove(d, btn);
      li.append(info, btn);
      return li;
    }));
    show(["dev-busy", "dev-list", "dev-empty", "dev-error"], list.length ? "dev-list" : "dev-empty");
    $("pp-devs").textContent = list.length;
  } catch {
    show(["dev-busy", "dev-list", "dev-empty", "dev-error"], "dev-error");
  }
}
$("dev-retry").onclick = loadDevices;

let removing = null, opener = null;
function askRemove(d, btn) {
  removing = d; opener = btn;
  $("rm-t").textContent = `Remover ${d.label}?`;
  $("rm-err").hidden = true;
  $("rm-scrim").hidden = false;
  $("rm-cancel").focus();
}
function closeRemove() { $("rm-scrim").hidden = true; removing = null; opener?.focus(); }
$("rm-cancel").onclick = closeRemove;
$("rm").onkeydown = (e) => {
  if (e.key === "Escape") { e.preventDefault(); closeRemove(); }
  if (e.key === "Tab") { // foco preso no diálogo
    const [a, b] = [$("rm-cancel"), $("rm-ok")];
    if (e.shiftKey && document.activeElement === a) { e.preventDefault(); b.focus(); }
    else if (!e.shiftKey && document.activeElement === b) { e.preventDefault(); a.focus(); }
  }
};
$("rm-ok").onclick = async () => {
  const d = removing;
  try { await invoke("revoke", { id: d.id }); } catch { $("rm-err").hidden = false; return; }
  $("rm-scrim").hidden = true; removing = null;
  await loadDevices();
  $("toast").textContent = `${d.label} foi removido.`;
  $("toast-wrap").hidden = false;
  setTimeout(() => { $("toast-wrap").hidden = true; }, 4000);
  (document.querySelector("#dev-ul button") || $("pp-x")).focus();
};

// ---------- Configurações ----------
const THEME_NAME = { dark: "Tema Aurora", cosmo: "Tema Cosmo", light: "Tema Claro", contrast: "Alto contraste" };
function themePicked() {
  const v = document.documentElement.dataset.theme || "dark";
  for (const i of document.querySelectorAll("[name=theme-pick]")) i.checked = i.value === v;
  $("cfg-theme-t").textContent = THEME_NAME[v] || THEME_NAME.dark;
}
for (const i of document.querySelectorAll("[name=theme-pick]")) i.onchange = () => {
  $("theme").value = i.value;
  $("theme").dispatchEvent(new Event("change"));
  themePicked();
};
$("cfg-pane").addEventListener("click", (e) => {
  // Agentes e SSH têm painel próprio no trilho: o cartão abre o painel (o mesmo botão do trilho).
  const open = e.target.closest("[data-open]");
  if (open) return $(open.dataset.open).click();
  const go = e.target.closest("[data-go]");
  if (!go) return;
  for (const c of $("cfg-pane").querySelectorAll(".cfg-nav .card")) { c.classList.toggle("on", c === go); c.toggleAttribute("aria-current", c === go); }
  const f = $(go.dataset.go);
  f.scrollIntoView({ block: "start", behavior: "smooth" });
  f.querySelector("input, select, button")?.focus({ preventScroll: true });
});
// Mesma regra do Rust (settings::valid_server); o Rust valida de novo ao salvar.
function validServer(v) {
  const m = /^(wss?):\/\/([A-Za-z0-9.-]+)(:\d{1,5})?([/?#][^\s]*)?$/.exec(v);
  if (!m || (m[3] && +m[3].slice(1) > 65535)) return false;
  return m[1] === "wss" || /^(localhost|127\.0\.0\.1)$/i.test(m[2]);
}

async function loadSettings() {
  const s = await invoke("get_settings");
  $("autostart").checked = s.autostart;
  $("cfg-auto").textContent = s.autostart ? "Sim" : "Não";
  $("srv").value = s.server;
  $("sb").value = s.scrollback;
  srvCheck();
  $("ver").textContent = $("cfg-ver").textContent = s.version;
  $("cfg-sb").textContent = Number(s.scrollback).toLocaleString("pt-BR");
  themePicked();
  checkUpdate(true);
  $("prof-err").hidden = s.profiles_error_line == null;
  if (s.profiles_error_line != null) $("prof-err").firstElementChild.textContent = `O arquivo profiles.json tem um erro na linha ${s.profiles_error_line}. Os perfis padrão estão em uso.`;
  $("prof-default").hidden = !!s.profiles;
  const ed = await invoke("get_profiles").catch(() => ({ profiles: null, shells: [] }));
  $("prof-shells").replaceChildren(...ed.shells.map((sh) => { const o = document.createElement("option"); o.value = sh.command; o.label = sh.label; return o; }));
  // Sem arquivo: começa com os shells detectados como sugestão (só grava ao salvar).
  const list = ed.profiles || ed.shells.map((sh) => ({ label: sh.label, command: sh.command }));
  $("prof-body").replaceChildren(...list.map(profRow));
  $("prof-table").hidden = false;
  $("prof-msg").textContent = "";
}

// ---------- Editor de perfis (F4) ----------
// Cada linha guarda profile_id/osc no dataset: editar o rótulo não quebra o id nem a integração OSC.
function profRow(p = { label: "", command: "" }) {
  const tr = document.createElement("tr");
  tr.dataset.id = p.profile_id || "";
  tr.dataset.osc = p.osc ? "1" : "";
  const cell = (cls, input) => { const td = document.createElement("td"); if (cls) td.className = cls; td.append(input); return td; };
  const n = document.createElement("input"); n.type = "text"; n.maxLength = 40; n.value = p.label; n.setAttribute("aria-label", "Nome do perfil"); n.className = "p-label";
  const c = document.createElement("input"); c.type = "text"; c.value = p.command; c.setAttribute("aria-label", "Comando"); c.className = "mono p-cmd"; c.spellcheck = false; c.setAttribute("list", "prof-shells");
  const rm = document.createElement("button"); rm.type = "button"; rm.textContent = "Remover";
  rm.onclick = () => { const next = tr.nextElementSibling || tr.previousElementSibling; tr.remove(); (next?.querySelector("input") || $("prof-add")).focus(); };
  rm.setAttribute("aria-label", "Remover perfil " + (p.label || "novo"));
  tr.append(cell(null, n), cell(null, c), cell(null, rm));
  return tr;
}
$("prof-add").onclick = () => { const tr = profRow(); $("prof-body").append(tr); tr.querySelector("input").focus(); };
$("prof-save").onclick = async () => {
  const bad = /[\u0000-\u001f\u007f-\u009f]/;
  const rows = [...$("prof-body").rows];
  const profiles = rows.map((tr) => ({ profile_id: tr.dataset.id, osc: !!tr.dataset.osc, label: tr.querySelector(".p-label").value.trim(), command: tr.querySelector(".p-cmd").value.trim() }));
  const wrong = profiles.findIndex((p) => !p.label || [...p.label].length > 40 || !p.command || bad.test(p.label + p.command));
  $("prof-e").hidden = wrong < 0 && profiles.length > 0;
  $("prof-msg").textContent = "";
  if (!$("prof-e").hidden) { (rows[wrong]?.querySelector("input") || $("prof-add")).focus(); return; }
  try {
    const saved = await invoke("save_profiles", { profiles });
    $("prof-body").replaceChildren(...saved.map(profRow));
    $("prof-default").hidden = true; $("prof-err").hidden = true;
    $("prof-msg").textContent = "Perfis salvos. Valem para os próximos terminais.";
  } catch { $("prof-e").hidden = false; }
};

$("sb-save").onclick = async () => {
  const n = Number($("sb").value);
  const ok = Number.isInteger(n) && n >= 100 && n <= 100000;
  $("sb").setAttribute("aria-invalid", !ok); $("sb-e").hidden = ok; $("sb-msg").textContent = "";
  if (!ok) return $("sb").focus();
  try { await invoke("set_scrollback", { lines: n }); $("sb-msg").textContent = "Salvo. Vale para as próximas abas."; $("cfg-sb").textContent = n.toLocaleString("pt-BR"); }
  catch { $("sb-e").hidden = false; }
};

$("autostart").onchange = async (e) => {
  $("autostart-err").hidden = true;
  try { await invoke("set_autostart", { on: e.target.checked }); $("cfg-auto").textContent = e.target.checked ? "Sim" : "Não"; }
  catch { e.target.checked = !e.target.checked; $("autostart-err").hidden = false; }
};

function srvCheck() {
  const ok = validServer($("srv").value.trim());
  $("srv").setAttribute("aria-invalid", !ok);
  $("srv-e").hidden = ok;
  $("srv-save").disabled = !ok;
  return ok;
}
$("srv").oninput = () => { $("srv-msg").textContent = ""; $("srv-err").hidden = true; srvCheck(); };
$("settings").onsubmit = async (e) => {
  e.preventDefault();
  if (!srvCheck()) return;
  $("srv-err").hidden = true;
  $("srv-msg").textContent = "Salvo. Reconectando…";
  try {
    await invoke("set_server", { server: $("srv").value.trim() });
    expires = null; // o host reiniciou: OTK antiga não vale mais
    const st = await invoke("status");
    $("srv-msg").textContent = st.state === "error" ? "" : "Conectado";
    $("srv-err").hidden = st.state !== "error";
  } catch { $("srv-msg").textContent = ""; $("srv-err").hidden = false; }
};
$("open-dir").onclick = () => invoke("open_data_dir");
$("logs").onclick = () => invoke("open_data_dir");

async function checkUpdate(silent) {
  if (!silent) $("upd-msg").textContent = "Verificando…";
  try {
    const v = await invoke("check_update");
    $("cfg-upd-go").hidden = !v;
    if (v) $("upd-msg").textContent = `Versão ${v} disponível.`;
    else if (!silent) $("upd-msg").textContent = "Você já está na versão mais recente.";
  } catch { if (!silent) $("upd-msg").textContent = "Não foi possível verificar atualizações. Tente de novo."; }
}
$("cfg-upd").onclick = () => checkUpdate(false);
$("cfg-upd-go").onclick = async () => {
  $("cfg-upd-go").disabled = true;
  $("upd-msg").textContent = "Baixando e instalando…";
  try { await invoke("install_update"); }
  catch { $("cfg-upd-go").disabled = false; $("upd-msg").textContent = "Não foi possível atualizar. Tente de novo."; }
};

// Bandeja pede uma aba com a janela já aberta.
window.__TAURI__.event.listen("tab", (e) => tab(e.payload));
// Minimizada: o QR sai da tela (P13); ao restaurar, volta o da OTK pendente.
window.__TAURI__.event.listen("visible", (e) => {
  if (!e.payload) { clearInterval(ticker); expires = null; $("qr").removeAttribute("src"); show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-busy"); }
  else if (current === "pair" && !$("pair-pane").hidden && !expires) pair(true);
});
tab(location.hash.slice(1));

// Janela única com 3 abas (design §2–§5). Sem bundler: usa window.__TAURI__ (withGlobalTauri).
// Regra: nada de URI, OTK ou caminho em texto, log ou atributo. O QR chega pronto como imagem.
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
const show = (ids, on) => ids.forEach((id) => { $(id).hidden = id !== on; });
const TABS = ["pair", "devices", "settings"];

// ---------- Abas ----------
let current = null;
function tab(name) {
  if (!TABS.includes(name)) name = "pair";
  for (const t of TABS) {
    const sel = t === name;
    $("tab-" + t).setAttribute("aria-selected", sel);
    $("tab-" + t).tabIndex = sel ? 0 : -1;
    $(t).hidden = !sel;
  }
  // Trocar de aba não invalida a OTK (P13); entrar em Parear sem QR válido gera um.
  if (name === "pair" && (current !== "pair" || !expires)) pair();
  if (name === "devices") loadDevices();
  if (name === "settings") loadSettings();
  current = name;
}
for (const t of TABS) $("tab-" + t).onclick = () => tab(t);
document.querySelector("[role=tablist]").onkeydown = (e) => {
  const d = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
  if (!d) return;
  const next = TABS[(TABS.indexOf(current) + d + 3) % 3];
  tab(next);
  $("tab-" + next).focus();
};
document.addEventListener("click", (e) => {
  const t = e.target.closest("[data-tab]");
  if (t) tab(t.dataset.tab);
  if (e.target.closest("[data-act=pair]")) pair();
});

// ---------- Parear ----------
let expires = null, skew = 0, known = null, ticker = null;
const now = () => Date.now() + skew;

async function pair() {
  show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-busy");
  $("copied").hidden = true;
  try {
    known = new Set((await invoke("devices")).map((d) => d.id));
    const r = await invoke("pair");
    skew = r.now_ms - Date.now();
    expires = r.expires_ms;
    $("qr").src = r.qr;
    $("qr-box").classList.remove("expired");
    $("pair-live").hidden = false;
    $("pair-expired").hidden = true;
    $("pair-hint").hidden = false;
    $("copy").hidden = false;
    show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-show");
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
}

async function tick() {
  const left = Math.max(0, expires - now());
  const s = Math.ceil(left / 1000);
  $("left").textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  $("bar").style.width = (left / 3000) + "%";
  $("bar").parentElement.classList.toggle("warn", s <= 60);
  const st = await invoke("status");
  if (st.pairing_expires_ms) return;
  // OTK sumiu antes do prazo: foi usada (aparelho novo) ou expirou.
  const fresh = (await invoke("devices")).find((d) => !known.has(d.id));
  if (fresh) {
    clearInterval(ticker);
    expires = null;
    $("done-t").textContent = `${fresh.label} pareado`;
    show(["pair-busy", "pair-show", "pair-done", "pair-error"], "pair-done");
  } else expire();
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
  (document.querySelector("#dev-ul button") || $("tab-devices")).focus();
};

// ---------- Configurações ----------
// Mesma regra do Rust (settings::valid_server); o Rust valida de novo ao salvar.
function validServer(v) {
  const m = /^(wss?):\/\/([A-Za-z0-9.-]+)(:\d{1,5})?([/?#][^\s]*)?$/.exec(v);
  if (!m || (m[3] && +m[3].slice(1) > 65535)) return false;
  return m[1] === "wss" || /^(localhost|127\.0\.0\.1)$/i.test(m[2]);
}

async function loadSettings() {
  const s = await invoke("get_settings");
  $("autostart").checked = s.autostart;
  $("srv").value = s.server;
  srvCheck();
  $("ver").textContent = s.version;
  $("prof-err").hidden = s.profiles_error_line == null;
  if (s.profiles_error_line != null) $("prof-err").firstElementChild.textContent = `O arquivo profiles.json tem um erro na linha ${s.profiles_error_line}. Os perfis padrão estão em uso.`;
  $("prof-default").hidden = !!s.profiles;
  $("prof-table").hidden = !s.profiles;
  $("prof-body").replaceChildren(...(s.profiles || []).map((p) => {
    const tr = document.createElement("tr");
    const n = document.createElement("td"); n.textContent = p.name;
    const c = document.createElement("td"); c.className = "mono"; c.textContent = p.command;
    tr.append(n, c);
    return tr;
  }));
}

$("autostart").onchange = async (e) => {
  $("autostart-err").hidden = true;
  try { await invoke("set_autostart", { on: e.target.checked }); }
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

// Bandeja pede uma aba com a janela já aberta.
window.__TAURI__.event.listen("tab", (e) => tab(e.payload));
tab(location.hash.slice(1));

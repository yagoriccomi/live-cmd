// Aparência (tema, fonte, opacidade, desfoque, brilho, animações) em appearance.json: a mesma que o celular lê e grava
// (settings_get/settings_set). Toda mudança, daqui ou do celular, chega pelo evento "appearance" e é aplicada na hora.
(() => {
  const { invoke } = window.__TAURI__.core;
  const $ = (id) => document.getElementById(id);
  const root = document.documentElement;
  const GLASS = new Set(["dark", "cosmo"]);
  const NO_GLOW = ["--glass-glow", "--hero-glow", "--grad-glow", "--glow", "--shadow"];
  let cur = null;

  function apply(a) {
    cur = a;
    try { if (localStorage.theme !== a.theme) localStorage.theme = a.theme; } catch {}
    root.dataset.theme = a.theme;
    root.dataset.anim = a.animations ? "on" : "off";
    root.style.setProperty("--font-mono", `"${a.font_family}","Cascadia Mono",Consolas,monospace`);
    if (GLASS.has(a.theme)) root.style.setProperty("--glass-blur", a.blur ? `blur(${a.blur}px)` : "none");
    else root.style.removeProperty("--glass-blur");
    for (const v of NO_GLOW) a.glow ? root.style.removeProperty(v) : root.style.setProperty(v, "none");
    window.setFontBase?.(a.font_size);
    const sel = $("theme");
    if (sel) sel.value = a.theme;
    for (const i of document.querySelectorAll("[name=theme-pick]")) i.checked = i.value === a.theme;
    if ($("cfg-theme-t")) $("cfg-theme-t").textContent = { dark: "Tema Aurora", cosmo: "Tema Cosmo", light: "Tema Claro", contrast: "Alto contraste" }[a.theme];
    if ($("cfg-theme-s")) $("cfg-theme-s").textContent = GLASS.has(a.theme) ? `vidro ${a.blur} px · brilho ${a.glow ? "neon" : "desligado"}` : `sem vidro · brilho ${a.glow ? "neon" : "desligado"}`;
    if ($("cfg-look-st")) {
      const edited = a.theme !== "dark" || a.font_family !== "Fira Code" || a.font_size !== 13 || a.opacity !== 100 || a.blur !== 30 || !a.glow || !a.animations;
      $("cfg-look-st").textContent = edited ? "● editado" : "● padrão";
    }
    if ($("cfg-font")) {
      $("cfg-font").textContent = a.font_family; $("cfg-font-s").textContent = `${a.font_size} px${a.font_family === "Fira Code" ? " · ligaduras" : ""}`;
      $("cfg-op").textContent = a.opacity + "%"; $("cfg-blur").textContent = GLASS.has(a.theme) ? a.blur + " px" : "—";
    }
    if (!$("ap-font")) return;
    $("ap-font").value = a.font_family;
    $("ap-size").value = a.font_size; $("ap-size-v").value = a.font_size + " px";
    $("ap-opacity").value = a.opacity; $("ap-opacity-v").value = a.opacity + "%";
    $("ap-blur").value = a.blur; $("ap-blur-v").value = a.blur + " px";
    $("ap-blur").disabled = !GLASS.has(a.theme);
    $("ap-glow").checked = a.glow;
    $("ap-anim").checked = a.animations;
  }

  async function save(patch) {
    $("ap-err") && ($("ap-err").hidden = true);
    try { apply(await invoke("set_appearance", { patch })); }
    catch { if ($("ap-err")) $("ap-err").hidden = false; if (cur) apply(cur); }
  }

  invoke("get_appearance").then(apply).catch(() => {});
  window.__TAURI__.event.listen("appearance", (e) => apply(e.payload));

  // Tema como texto: Copiar tema coloca o JSON na área de transferência; Importar tema aplica o que estiver lá.
  const KEYS = { theme: ["dark", "cosmo", "light", "contrast"] };
  $("cfg-copy")?.addEventListener("click", async () => {
    if (!cur) return;
    const { theme, font_family, font_size, opacity, blur, glow, animations } = cur;
    try { await navigator.clipboard.writeText(JSON.stringify({ theme, font_family, font_size, opacity, blur, glow, animations })); } catch {}
  });
  $("cfg-import")?.addEventListener("click", async () => {
    let t;
    try { t = JSON.parse(await navigator.clipboard.readText()); } catch { return; }
    const patch = {};
    if (KEYS.theme.includes(t.theme)) patch.theme = t.theme;
    if (typeof t.font_family === "string") patch.font_family = t.font_family;
    for (const k of ["font_size", "opacity", "blur"]) if (Number.isInteger(t[k])) patch[k] = t[k];
    for (const k of ["glow", "animations"]) if (typeof t[k] === "boolean") patch[k] = t[k];
    if (Object.keys(patch).length) save(patch); // o Rust valida faixas e fontes
  });
  $("cfg-edit")?.addEventListener("click", () => { $("cfg-look").scrollIntoView({ block: "start", behavior: "smooth" }); document.querySelector("[name=theme-pick]:checked")?.focus({ preventScroll: true }); });
  $("theme")?.addEventListener("change", () => { if (cur && $("theme").value !== cur.theme) save({ theme: $("theme").value }); });
  if (!$("ap-font")) return;
  $("ap-font").onchange = () => save({ font_family: $("ap-font").value });
  for (const [id, key, unit] of [["ap-size", "font_size", " px"], ["ap-opacity", "opacity", "%"], ["ap-blur", "blur", " px"]]) {
    $(id).oninput = () => { $(id + "-v").value = $(id).value + unit; };
    $(id).onchange = () => save({ [key]: +$(id).value });
  }
  $("ap-glow").onchange = () => save({ glow: $("ap-glow").checked });
  $("ap-anim").onchange = () => save({ animations: $("ap-anim").checked });
})();

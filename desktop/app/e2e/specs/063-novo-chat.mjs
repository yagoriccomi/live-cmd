// #63 (018/019): novo chat numa lista só com grupos "Shells" (inclui cadastrados) e "Agentes"; o padrão do agente salva no host.
const P = (h, body) => h.js(`return new Promise((done) => { (async () => { ${body} })().then((v) => done(JSON.stringify(v)), (e) => done("ERR " + e)); });`);
export default async (h) => {
  // Shell cadastrado no editor de perfis (o host reinicia para reler).
  const saved = await P(h, `const ed = await invoke("get_profiles"); const l = ed.profiles || ed.shells.map((s) => ({ label: s.label, command: s.command }));
    return invoke("save_profiles", { profiles: [...l.filter((p) => p.label !== "Meu CMD"), { label: "Meu CMD", command: "cmd.exe" }] });`);
  if (saved.startsWith('"ERR')) throw new Error("save_profiles: " + saved);
  await h.until(`refresh(LOCAL); return conns[LOCAL].profiles.some((p) => p.label === "Meu CMD")`, 20000);
  await h.js(`profileMenu(document.querySelector("#sidebar .add"), LOCAL)`);
  await h.until(`return !document.getElementById("menu").hidden`, 3000);
  const items = JSON.parse(await h.js(`return JSON.stringify([...document.getElementById("menu").children].map((x) => x.textContent.trim()))`));
  const iS = items.indexOf("Shells"), iA = items.indexOf("Agentes"), iC = items.findIndex((t) => t.includes("Meu CMD")), iX = items.findIndex((t) => t.includes("Codex"));
  if (!(iS >= 0 && iS < iC && iC < iA && iA < iX)) throw new Error("menu sem grupos/itens: " + JSON.stringify(items));
  // Agente pelo menu abre o formulário com ele; "Salvar como padrão" grava no host.
  await h.js(`[...document.querySelectorAll("#menu button")].find((x) => x.textContent.includes("Codex")).click()`);
  await h.until(`return !document.getElementById("ai-scrim").hidden && document.getElementById("ai-agent").value === "codex"`, 5000);
  await h.js(`document.getElementById("ai-args").value = "--full-auto"; document.getElementById("ai-def").checked = true; document.getElementById("ai-ok").click()`);
  await h.until(`return document.getElementById("ai-scrim").hidden`, 5000);
  const d = await P(h, `return invoke("agent_default_get", { conn: LOCAL })`);
  h.eq(d, JSON.stringify({ agent: "codex", args: "--full-auto" }), "padrão salvo no host");
};

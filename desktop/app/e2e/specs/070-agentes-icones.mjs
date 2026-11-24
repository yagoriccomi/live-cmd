// 025: Configurações > Agentes lista todos os agentes com inicial colorida (cores distintas) e "Trocar imagem";
// agent_icon_get responde sem INVALID_MESSAGE (pedido com api).
export default async (h) => {
  await h.until(`return document.querySelectorAll("#cfg-agents-list li").length >= 3`, 5000);
  const r = await h.js(`const li = [...document.querySelectorAll("#cfg-agents-list li")];
    return { btn: li.every((l) => /Trocar imagem/.test(l.querySelector("button")?.textContent)),
      cores: new Set(li.map((l) => l.querySelector(".ag-ico").style.backgroundColor)).size, n: li.length }`);
  if (!r.btn || r.cores !== r.n) throw new Error(JSON.stringify(r));
  const e = await h.js(`return new Promise((d) => invoke("agent_icon_get", { conn: LOCAL, agent: "codex" }).then(() => d("ok"), (x) => d(String(x))))`);
  if (e !== "ok") throw new Error("agent_icon_get: " + e);
};

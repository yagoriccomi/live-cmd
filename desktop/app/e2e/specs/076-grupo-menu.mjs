// 017 (v0.2.4): grupo tem menu (renomear, mover, excluir); excluir devolve os cards a "Sem grupo" sem fechar sessões; soltar card anima.
export default async (h) => {
  while ((await h.js(`return conns[LOCAL].sessions.length`)) < 2) { const n = await h.js(`return conns[LOCAL].sessions.length`); await h.js(`newSession(LOCAL)`); await h.until(`return conns[LOCAL].sessions.length > ${n}`, 15000); }
  const sids = await h.js(`return conns[LOCAL].sessions.map((s) => s.session_id)`);
  await h.js(`const L = layoutOf(LOCAL); L.group[arguments[0]] = "G017"; renderSidebar()`, sids[0]);
  const hd = `[...document.querySelectorAll("#local-ul .group-h")].find((b) => b.title === "G017")`;
  await h.until(`return ${hd}`, 3000);
  await h.js(`${hd}.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }))`);
  const items = await h.until(`const m = [...document.querySelectorAll("[role=menuitem]")].map((b) => b.textContent); return m.length && m`, 3000);
  for (const w of ["Renomear", "Excluir grupo"]) h.eq(items.some((t) => t.includes(w)), true, "menu sem " + w + ": " + items);
  h.eq(items.some((t) => /Mover para (cima|baixo)/.test(t)), true, "menu sem mover: " + items);
  await h.js(`[...document.querySelectorAll("[role=menuitem]")].find((b) => /Excluir grupo/.test(b.textContent)).click()`);
  await h.until(`return !${hd} && layoutOf(LOCAL).group[arguments[0]] === "Sem grupo"`, 3000, sids[0]);
  h.eq(await h.js(`return conns[LOCAL].sessions.map((s) => s.session_id).filter((x) => arguments[0].includes(x)).length`, sids), sids.length, "sessão fechou");
  // Soltar card antes do outro: cards deslizam.
  await h.js(`window.__anim = 0; const o = Element.prototype.animate; Element.prototype.animate = function (...a) { __anim++; return o.apply(this, a); }`);
  await h.js(`const a = document.querySelector("#local-ul button[data-key$='/" + arguments[1] + "']").closest("[data-drop]") || document.querySelector("#local-ul [data-drop]"); a.__drop(arguments[0])`, sids[1], sids[0]);
  h.eq(await h.js(`return __anim > 0 || matchMedia("(prefers-reduced-motion: reduce)").matches`), true, "cards não animaram");
  await h.js(`delete layoutOf(LOCAL).group[arguments[0]]; saveSideOrder(LOCAL); renderSidebar()`, sids[0]);
};

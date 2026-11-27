// 022 (v0.2.4): tabela de atalhos sem texto espremido: nada estoura, nada corta, tecla e descrição com folga.
export default async (h) => {
  await h.js(`window.liveTab?.("settings")`);
  await h.js(`document.querySelector('[data-go="cfg-keys"]')?.click()`);
  await h.until(`const l = document.getElementById("keys-list"); return l && l.offsetWidth > 0`, 5000);
  const bad = await h.js(`return [...document.querySelectorAll("#keys-list li")].flatMap((li) => {
    const out = [], [t, k] = li.children, r = li.getBoundingClientRect();
    for (const e of [t, k]) {
      const cs = getComputedStyle(e);
      if (e.scrollWidth > e.clientWidth + 1) out.push("estoura: " + e.textContent);
      if (cs.textOverflow === "ellipsis" && e.scrollWidth > e.clientWidth) out.push("corta: " + e.textContent);
      if (parseFloat(cs.fontSize) < 13) out.push("fonte " + cs.fontSize + ": " + e.textContent);
    }
    // tecla não pode quebrar em várias linhas
    if (k.getClientRects().length > 1 || k.getBoundingClientRect().height > parseFloat(getComputedStyle(k).lineHeight || 20) * 1.6 + 8) out.push("tecla quebrou: " + k.textContent);
    if (t.getBoundingClientRect().right > k.getBoundingClientRect().left + 1) out.push("sobrepõe: " + t.textContent);
    return out;
  })`);
  h.eq(bad, [], "tabela espremida");
  await h.js(`document.getElementById("cfg-x")?.click()`);
};

// 021 (v0.2.4): arrastar guia da barra solta na posição do cursor, com as guias deslizando até o lugar.
export default async (h) => {
  while ((await h.js(`return tabs.length`)) < 3) { const n = await h.js(`return tabs.length`); await h.js(`newSession(LOCAL)`); await h.until(`return tabs.length > ${n}`, 15000); }
  await h.until(`return document.querySelectorAll("#tabs [role=tab][data-key]").length >= 3`, 5000);
  const keys = await h.js(`return tabs.map((t) => t.key)`);
  const css = (k) => `#tabs [data-key="${k}"]`;
  // 3ª guia (a última pode estar fora da barra visível na suíte) solta no vão entre a 1ª e a 2ª (borda esquerda da 2ª): vira a 2ª.
  const w = await h.js(`return document.querySelector(arguments[0]).getBoundingClientRect().width`, css(keys[1]));
  await h.js(`window.__anim = 0; const o = Element.prototype.animate; Element.prototype.animate = function (...a) { __anim++; return o.apply(this, a); }`);
  await h.drag(css(keys[2]), css(keys[1]), -w / 2 + 3, 0);
  await h.until(`return tabs[1].key === arguments[0]`, 3000, keys[2])
    .catch(async () => { throw new Error("ordem: " + JSON.stringify(await h.js(`return tabs.map((t) => t.key)`)) + " esperava 2ª = " + keys[2]); });
  h.eq(await h.js(`return __anim > 0 || matchMedia("(prefers-reduced-motion: reduce)").matches`), true, "guias não animaram");
};

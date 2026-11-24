// 021: atrás das pílulas das guias não há barra escura nem linha clara (fundo, sombra e borda vazios).
export default async (h) => {
  const s = await h.js(`return [document.getElementById("tabs"), document.querySelector(".term-bar")].map((e) => { const c = getComputedStyle(e); return [c.backgroundColor, c.boxShadow, c.borderTopWidth, c.backdropFilter].join(" ") })`);
  for (const v of s) if (!/^rgba\(0, 0, 0, 0\) none 0px none$/.test(v)) throw new Error("barra visível: " + v);
};

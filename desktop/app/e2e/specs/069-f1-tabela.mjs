// 022: F1 em tabela de duas colunas alinhadas; tecla com cor plena do texto (não apagada).
export default async (h) => {
  await h.js(`help()`);
  await h.until(`return !document.getElementById("help-scrim").hidden`, 3000);
  const r = await h.js(`const l = document.getElementById("help-list"), c = getComputedStyle(l), dts = [...l.querySelectorAll("dt")], dds = [...l.querySelectorAll("dd")];
    const x = new Set(dds.map((d) => Math.round(d.getBoundingClientRect().left)));
    return { grid: c.display, cols: x.size, ink: getComputedStyle(dts[0]).color === getComputedStyle(document.body).color, n: dts.length }`);
  if (r.grid !== "grid" || r.cols !== 1 || !r.ink || r.n < 10) throw new Error(JSON.stringify(r));
  await h.js(`document.getElementById("help-scrim").hidden = true`);
};

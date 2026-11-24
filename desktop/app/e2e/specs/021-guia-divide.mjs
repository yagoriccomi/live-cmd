// 021: arrastar uma guia para a área do terminal divide a guia ativa em dois painéis.
export default async (h) => {
  while ((await h.js(`return tabs.length`)) < 2) { await h.js(`newSession(LOCAL)`); await h.until(`return true`); }
  await h.until(`return tabs.length >= 2 && document.querySelectorAll("#tabs [role=tab]").length >= 2`, 15000);
  const [a, b] = await h.js(`return [tabs[0].key, tabs[1].key]`);
  await h.js(`document.querySelector('#tabs [data-key="' + arguments[0] + '"]').click()`, a);
  await h.until(`return active === arguments[0]`, 5000, a);
  const n = await h.js(`return tabs.length`);
  await h.drag(`#tabs [data-key="${b}"]`, `#screen`, 0, 0);
  await h.until(`return tabs.find((t) => t.key === arguments[0])?.split?.key === arguments[1]`, 5000, a, b);
  h.eq(await h.js(`return tabs.length`), n - 1, "guias após dividir");
  h.eq(await h.js(`return document.getElementById("screen2").hidden`), false, "segundo painel visível");
};

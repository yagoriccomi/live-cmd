// 016 (v0.2.4): hover no trilho só expande o menu; clique abre a página sem mudar o menu; 2º clique no mesmo ícone inverte.
export default async (h) => {
  const col = `return $("layout").classList.contains("collapsed")`;
  h.eq(await h.js(`return !!document.getElementById("fold-side")`), false, "botão de alternar existe");
  const c0 = await h.js(col);
  const [hx, hy] = await h.at("#ssh-open");
  await h.hover([[hx, hy, 500], [5, 5, 300]]);
  h.eq(await h.js(`return $("ssh-panel").hidden`), true, "hover abriu página");
  await h.click("#ssh-open");
  await h.until(`return !$("ssh-panel").hidden`, 3000);
  h.eq(await h.js(col), c0, "clique mudou o menu");
  await h.click("#ssh-open");
  await h.until(`return ${col.slice(7)} === ${!c0}`, 3000);
  h.eq(await h.js(`return !$("ssh-panel").hidden`), true, "2º clique fechou a página");
  await h.click("#ssh-open");
  await h.until(`return ${col.slice(7)} === ${c0}`, 3000);
  await h.js(`$("nav-term").click()`);
};

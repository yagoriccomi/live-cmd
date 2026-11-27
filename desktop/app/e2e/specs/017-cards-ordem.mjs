// 017: arrastar um card reordena a lista e a ordem persiste ao reabrir o app; soltar na zona final cria grupo e no cabeçalho entra nele.
const ids = `return [...document.querySelectorAll("#local-ul [data-key]")].map((b) => b.dataset.key)`;
export default async (h) => {
  for (let i = 0; i < 3; i++) await h.js(`newSession(LOCAL)`);
  await h.until(`return document.querySelectorAll("#local-ul [data-key]").length >= 3`, 15000);
  // Mesmo grupo (mesma pasta): o arraste é reordenação, não troca de grupo.
  const before = await h.js(ids);
  await h.drag(`#local-ul [data-key="${before[2]}"]`, `#local-ul [data-key="${before[0]}"]`, 0, -5);
  await h.until(`return document.querySelector("#local-ul [data-key]")?.dataset.key === arguments[0]`, 5000, before[2]);
  const after = await h.js(ids);
  // Só os 3 primeiros: numa 2a tentativa (run.mjs) as sessões da primeira continuam na lista.
  h.eq(after.slice(0, 3), [before[2], before[0], before[1]], "ordem após arrastar");
  await new Promise((r) => setTimeout(r, 1500)); // `term_layout_set` é assíncrono
  await h.reopen();
  await h.until(`return document.querySelectorAll("#local-ul [data-key]").length >= 3`, 15000);
  await new Promise((r) => setTimeout(r, 1500));
  h.eq(await h.js(ids), after, "ordem após reabrir");

  // Grupos: card na zona final vira "Grupo N"; outro card solto no cabeçalho desse grupo entra nele.
  const [x, y] = after;
  await h.drag(`#local-ul [data-key="${x}"]`, `#local-ul .group-new`);
  const g = await h.until(`return layouts.local.group[arguments[0]]`, 5000, x.split("/")[1]);
  await h.until(`return [...document.querySelectorAll("#local-ul .group-h")].some((b) => b.title === arguments[0])`, 5000, g);
  await new Promise((r) => setTimeout(r, 300)); // FLIP de 180 ms (017) ainda move os cards
  const hd = await h.js(`return [...document.querySelectorAll("#local-ul .group-h")].findIndex((b) => b.title === arguments[0]) + 1`, g);
  await h.drag(`#local-ul [data-key="${y}"]`, `#local-ul .sess-group:nth-of-type(${hd}) .group-h`);
  await h.until(`return layouts.local.group[arguments[0]] === arguments[1]`, 5000, y.split("/")[1], g);
  const inG = `return [...document.querySelectorAll("#local-ul .sess-group")].find((li) => li.querySelector(".group-h").title === arguments[0])?.querySelectorAll("[data-key]").length`;
  h.eq(await h.js(inG, g), 2, "cards no grupo novo");
  await new Promise((r) => setTimeout(r, 1500));
  await h.reopen();
  h.eq(await h.until(inG, 15000, g), 2, "grupo após reabrir");
};

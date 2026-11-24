// #60/#61: badge pelo clique no espaço do card; sai do menu de contexto; segue a sessão ao inverter e mover.
export default async (h) => {
  while ((await h.js(`return tabs.length`)) < 2) { await h.js(`newSession(LOCAL)`); await h.until(`return true`); }
  await h.until(`return tabs.length >= 2 && document.querySelectorAll("#sidebar .badge-slot").length >= 2`, 15000);
  const [a, b] = await h.js(`return [tabs[0].sid, tabs[1].sid]`);
  const card = (sid) => h.js(`return '#sidebar [data-key="' + LOCAL + '/' + arguments[0] + '"]'`, sid);
  // Clique no espaço abre a lista; escolhe "Travado" para a sessão b.
  await h.js(`document.querySelector(arguments[0] + " .badge-slot").click()`, await card(b));
  await h.until(`return !document.getElementById("menu").hidden`, 3000);
  await h.js(`[...document.querySelectorAll("#menu button")].find((x) => x.textContent.includes("Travado")).click()`);
  await h.until(`return layoutOf(LOCAL).badge[arguments[0]] === "lock"`, 3000, b);
  h.eq(await h.js(`return document.querySelector(arguments[0] + " .badge-slot").textContent`, await card(b)), "🔒", "badge no card");
  // Menu de contexto do card não tem mais a seção Badge.
  await h.js(`document.querySelector(arguments[0]).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }))`, await card(b));
  await h.until(`return !document.getElementById("menu").hidden`, 3000);
  h.eq(await h.js(`return [...document.querySelectorAll("#menu .m-lbl")].some((x) => x.textContent === "Badge")`), false, "Badge fora do menu de contexto");
  await h.js(`closeMenu(false)`);
  // Divide a guia de a com b, inverte: o badge segue b.
  const tb = (sid) => h.js(`return [...document.querySelectorAll("#tabs [role=tab]")].find((x) => x.dataset.key.endsWith("/" + arguments[0]) || tabs.find((t) => t.key === x.dataset.key)?.split?.sid === arguments[0])?.textContent || ""`, sid);
  await h.js(`activate(LOCAL + "/" + arguments[0])`, a);
  await h.js(`split(activeTab(), LOCAL, arguments[0])`, b);
  await h.until(`return activeTab()?.split?.sid === arguments[0]`, 5000, b);
  await h.js(`swapSides()`);
  await h.until(`return activeTab()?.sid === arguments[0] && activeTab().split?.sid === arguments[1]`, 5000, b, a);
  h.eq(await h.js(`return layoutOf(LOCAL).badge[arguments[0]] || null`, b), "lock", "badge de b após inverter");
  h.eq(await h.js(`return layoutOf(LOCAL).badge[arguments[0]] || null`, a), null, "a sem badge após inverter");
  if (!(await tb(b)).includes("🔒")) throw new Error("guia sem o badge após inverter");
  // Desfaz: b vira guia própria e leva o badge.
  await h.js(`unsplit(activeTab())`);
  await h.until(`return tabs.some((t) => t.sid === arguments[0] && !t.split) && tabs.some((t) => t.sid === arguments[1] && !t.split)`, 5000, a, b);
  const own = await h.js(`return [...document.querySelectorAll("#tabs [role=tab]")].map((x) => [x.dataset.key.split("/").pop(), x.textContent])`);
  const ta = own.find(([s]) => s === a)?.[1] || "", tbb = own.find(([s]) => s === b)?.[1] || "";
  if (!tbb.includes("🔒") || ta.includes("🔒")) throw new Error("badge não seguiu a sessão ao mover: " + JSON.stringify(own));
  // Remove pelo espaço.
  await h.js(`document.querySelector(arguments[0] + " .badge-slot").click()`, await card(b));
  await h.until(`return !document.getElementById("menu").hidden`, 3000);
  await h.js(`[...document.querySelectorAll("#menu button")].find((x) => x.textContent.includes("Nenhuma")).click()`);
  await h.until(`return !layoutOf(LOCAL).badge[arguments[0]]`, 3000, b);
};

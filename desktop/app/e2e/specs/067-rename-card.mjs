// 014: renomear um card pelo F2 não trava a seleção dos cards depois.
export default async (h) => {
  while ((await h.js(`return document.querySelectorAll("#local-ul button[data-key]").length`)) < 2) { await h.js(`newSession(LOCAL)`); await h.until(`return true`); }
  await h.js(`const b = document.querySelector("#local-ul button[data-key]"); b.focus(); b.dispatchEvent(new KeyboardEvent("keydown", { key: "F2", bubbles: true }))`);
  await h.until(`return !!document.querySelector("#local-ul input.rename")`, 3000);
  await h.js(`const i = document.querySelector("#local-ul input.rename"); i.value = "renomeado-e2e"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))`);
  await h.until(`return !document.querySelector("#local-ul input.rename")`, 3000);
  // Duplo clique e clique em outro card (fluxo real com mouse).
  await h.js(`document.querySelector("#local-ul .card-t").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }))`);
  await h.until(`return !!document.querySelector("#local-ul input.rename")`, 3000);
  await h.click(`#local-ul li:nth-child(2) button[data-key]`).catch(() => h.js(`document.querySelectorAll("#local-ul button[data-key]")[1].click()`));
  await h.until(`return !document.querySelector("#local-ul input.rename")`, 3000);
  for (const n of [1, 0, 1]) {
    const k = await h.js(`const b = document.querySelectorAll("#local-ul button[data-key]")[${n}]; b.click(); return b.dataset.key`);
    await h.until(`return document.querySelector('#local-ul button[aria-current="true"]')?.dataset.key === ${JSON.stringify(k)}`, 5000);
  }
};

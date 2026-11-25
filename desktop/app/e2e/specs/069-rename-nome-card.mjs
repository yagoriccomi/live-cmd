// Nome dado no F2 fica no card (não volta ao nome da pasta) e sobrevive a reabrir a lista.
export default async (h) => {
  if (!(await h.js(`return document.querySelectorAll("#local-ul button[data-key]").length`))) await h.js(`newSession(LOCAL)`);
  await h.until(`return document.querySelector("#local-ul button[data-key]")`, 15000);
  await h.js(`const b = document.querySelector("#local-ul button[data-key]"); b.focus(); b.dispatchEvent(new KeyboardEvent("keydown", { key: "F2", bubbles: true }))`);
  await h.until(`return !!document.querySelector("#local-ul input.rename")`, 3000);
  await h.js(`const i = document.querySelector("#local-ul input.rename"); i.value = "nome-e2e"; i.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }))`);
  await h.until(`return document.querySelector("#local-ul .card-t")?.textContent === "nome-e2e"`, 5000);
  await h.until(`return layoutFetched.delete(LOCAL) || true`);
  await h.js(`refresh(LOCAL)`);
  await h.until(`return document.querySelector("#local-ul .card-t")?.textContent === "nome-e2e"`, 5000);
};

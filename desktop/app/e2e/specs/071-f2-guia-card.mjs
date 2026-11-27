// 014 (v0.2.4): clicar na guia ou no card e apertar F2 (tecla real) renomeia; o foco não vai ao terminal.
export default async (h) => {
  if (!(await h.js(`return tabs.length`))) await h.js(`newSession(LOCAL)`);
  await h.until(`return document.querySelector("#tabs [role=tab][data-key]") && document.querySelector("#local-ul button[data-key]")`, 15000);
  for (const css of ["#tabs [role=tab][data-key]", "#local-ul button[data-key]"]) {
    await h.click(css);
    await h.key("\uE032");
    await h.until(`return !!document.querySelector("input.rename")`, 3000)
      .catch(async () => { throw new Error("F2 não abriu renomear em " + css + "; foco: " + await h.js(`return document.activeElement?.id || document.activeElement?.className`)); });
    await h.js(`document.querySelector("input.rename").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }))`);
  }
};

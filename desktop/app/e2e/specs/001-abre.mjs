// O app abre a janela do terminal com o script carregado (sem erro no carregamento).
export default async (h) => {
  await h.until(`return typeof newSession === "function" && document.getElementById("local-ul") !== null`);
  const fits = await h.js(`
    const screen = document.querySelector("#screen");
    const probe = screen.cloneNode(false);
    probe.removeAttribute("id");
    probe.hidden = false;
    probe.style.cssText = "position:fixed;inset:auto;left:0;top:0;width:420px;height:160px";
    const row = document.createElement("div");
    probe.append(row);
    screen.after(probe);
    try {
      const previous = document.body.style.zoom;
      document.body.style.zoom = ".75";
      try {
        const { cols } = size(probe);
        const range = document.createRange();
        const edge = row.getBoundingClientRect().right;
        row.textContent = "W".repeat(cols);
        range.selectNodeContents(row);
        const right = range.getBoundingClientRect().right;
        row.append("W");
        range.selectNodeContents(row);
        return edge > 0 && right <= edge + .5 && range.getBoundingClientRect().right > edge;
      } finally { document.body.style.zoom = previous; }
    } finally { probe.remove(); }
  `);
  if (!fits) throw new Error("Colunas calculadas não correspondem à largura visível com zoom");
};

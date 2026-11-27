// 025: Configurações > Agentes envia imagem (reduzida a 128 px) que vira o ícone do card; "Remover" volta ao logo oficial.
export default async (h) => {
  await h.js(`
    const c = Object.assign(document.createElement("canvas"), { width: 300, height: 200 });
    c.getContext("2d").fillRect(0, 0, 300, 200);
    const bin = atob(c.toDataURL("image/png").split(",")[1]);
    const dt = new DataTransfer(); dt.items.add(new File([Uint8Array.from(bin, (x) => x.charCodeAt(0))], "a.png", { type: "image/png" }));
    const click = HTMLInputElement.prototype.click;
    HTMLInputElement.prototype.click = function () {
      if (this.type !== "file") return click.call(this);
      HTMLInputElement.prototype.click = click;
      this.files = dt.files; this.dispatchEvent(new Event("change"));
    };
    conns[LOCAL].sessions.push({ session_id: "fake080", label: "claude", agent: "claude" }); renderSidebar();
    document.querySelector('#cfg-agents-list button[aria-label="Trocar imagem de Claude Code"]').click()`);
  await h.until(`return [...document.querySelectorAll("#local-ul .ag-ico")].some((i) => i.style.backgroundImage.startsWith('url("data:image'))`);
  await h.js(`document.querySelector('#cfg-agents-list button[aria-label="Remover imagem de Claude Code"]').click()`);
  await h.until(`const a = [...document.querySelectorAll("#local-ul .ag-ico")]; return a.length && a.every((i) => !i.style.backgroundImage.includes("data:image")) && a.some((i) => i.style.backgroundImage.includes("agentes/claude.svg"))`);
  await h.js(`conns[LOCAL].sessions = conns[LOCAL].sessions.filter((s) => s.session_id !== "fake080"); renderSidebar()`);
};

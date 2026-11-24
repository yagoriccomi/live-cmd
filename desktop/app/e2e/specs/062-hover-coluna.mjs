// #62 (016): hover no item abre o painel dele sobre o terminal; o vão não fecha; sair fecha; clique fixa; QR navega no clique.
export default async (h) => {
  const vis = (id) => h.js(`return !document.getElementById(arguments[0]).hidden`, id);
  const [bx, by] = await h.at("#ssh-open");
  const [px, py] = await h.at("#ssh-panel .pane-l", 0, 0).catch(() => [bx + 120, by]);
  await h.hover([[bx, by, 200]]);
  await h.until(`return !document.getElementById("ssh-panel").hidden`, 3000);
  // Atravessa o vão (borda direita do trilho até o painel) devagar: segue aberto.
  const r = await h.js(`const a = document.querySelector(".rail").getBoundingClientRect(), b = document.querySelector("#ssh-panel .pane-l").getBoundingClientRect(); return [a.right, b.left]`);
  await h.hover([[Math.round(r[0]) + 2, by, 60], [Math.round((r[0] + r[1]) / 2), by, 60], [Math.round(r[1]) + 20, by, 500]]);
  h.eq(await vis("ssh-panel"), true, "painel aberto após o vão");
  // Outro item troca o painel.
  const [hx, hy] = await h.at("#hist-open");
  await h.hover([[hx, hy, 300]]);
  await h.until(`return !document.getElementById("hist").hidden && document.getElementById("ssh-panel").hidden`, 3000);
  // Sair para longe fecha.
  await h.hover([[px + 600, py + 300, 800]]);
  await h.until(`return document.getElementById("hist").hidden && document.getElementById("ssh-panel").hidden`, 3000);
  // Clique fixa: sair não fecha.
  await h.hover([[bx, by, 200]]);
  await h.until(`return !document.getElementById("ssh-panel").hidden`, 3000);
  await h.click("#ssh-open");
  await h.hover([[px + 600, py + 300, 800]]);
  h.eq(await vis("ssh-panel"), true, "clique fixa o painel");
  await h.js(`document.getElementById("ssh-x").click()`);
  // Hover no QR não abre; clique navega.
  const [qx, qy] = await h.at("#nav-pair");
  await h.hover([[qx, qy, 500]]);
  h.eq(await vis("pair-pane"), false, "hover no QR não abre");
  await h.click("#nav-pair");
  await h.until(`return !document.getElementById("pair-pane").hidden`, 3000);
  await h.js(`document.getElementById("pp-x").click()`);
  await h.hover([[px + 600, py + 300, 100]]);
};

// 027 (v0.2.4): soltar o botão fora da janela (sem mouseup) para a rolagem da seleção.
export default async (h) => {
  const n0 = await h.js(`return tabs.length`);
  await h.js(`newSession(LOCAL)`);
  await h.until(`return tabs.length > ${n0} && activeTab()?.sid && rowsEl().some((r) => />/.test(r.textContent))`, 15000);
  const t = await h.js(`const t = activeTab(); return { conn: t.conn, sid: t.sid }`);
  await h.js(`invoke("term_input", { conn: arguments[0].conn, sessionId: arguments[0].sid, data: "1..300 | % { 'linha' + $_ }\\r" })`, t);
  await h.until(`return rowsEl().some((r) => /linha300/.test(r.textContent))`, 15000);
  await h.js(`const s = pane(), r = s.getBoundingClientRect(), rs = rowsEl(s), b = rs[rs.length - 1].getBoundingClientRect();
    s.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, buttons: 1, clientX: r.left + 10, clientY: b.top + 2 }));
    dispatchEvent(new MouseEvent("mousemove", { buttons: 1, clientX: r.left + 10, clientY: r.top - 20 }));`);
  await h.until(`return (screens[activeTab().key]?.off || 0) > 5`, 8000);
  // Cursor volta à janela já sem botão: nenhum mouseup chega.
  await h.js(`const r = pane().getBoundingClientRect(); dispatchEvent(new MouseEvent("mousemove", { buttons: 0, clientX: r.left + 10, clientY: r.top - 20 }))`);
  await h.until(`return /copiada/.test(document.getElementById("banner-t").textContent)`, 5000);
  const a = await h.js(`return screens[activeTab().key].off`);
  await new Promise((r) => setTimeout(r, 800));
  h.eq(await h.js(`return screens[activeTab().key].off`), a, "rolagem continuou depois de soltar");
  await h.js(`pane().dispatchEvent(new KeyboardEvent("keydown", { key: "End", ctrlKey: true, bubbles: true }))`);
};

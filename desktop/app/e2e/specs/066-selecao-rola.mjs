// 027: arrastar a seleção acima do topo rola o histórico e copia todas as linhas; Ctrl+Home vai ao topo e Ctrl+End volta.
export default async (h) => {
  const n0 = await h.js(`return tabs.length`);
  await h.js(`newSession(LOCAL)`);
  await h.until(`return tabs.length > ${n0} && activeTab()?.sid && rowsEl().some((r) => />/.test(r.textContent))`, 15000);
  const t = await h.js(`const t = activeTab(); return { conn: t.conn, sid: t.sid, key: t.key }`);
  await h.js(`invoke("term_input", { conn: arguments[0].conn, sessionId: arguments[0].sid, data: "1..120 | % { 'linha' + $_ }\\r" })`, t);
  await h.until(`return rowsEl().some((r) => /linha120/.test(r.textContent))`, 15000);
  const key = (k) => h.js(`pane().dispatchEvent(new KeyboardEvent("keydown", { key: "${k}", ctrlKey: true, bubbles: true, cancelable: true }))`);
  await key("Home");
  await h.until(`return (screens[activeTab().key]?.off || 0) > 50`, 5000);
  await key("End");
  await h.until(`return (screens[activeTab().key]?.off || 0) === 0`, 5000);
  // Arrasta da última linha até acima do painel e segura até rolar.
  await h.js(`const s = pane(), r = s.getBoundingClientRect(), rs = rowsEl(s), b = rs[rs.length - 1].getBoundingClientRect();
    s.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, clientX: r.left + 10, clientY: b.top + 2 }));
    dispatchEvent(new MouseEvent("mousemove", { clientX: r.left + 10, clientY: r.top - 200 }));`);
  await h.until(`return (screens[activeTab().key]?.off || 0) > 30`, 8000);
  await h.js(`document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }))`);
  await h.until(`return /copiada/.test(document.getElementById("banner-t").textContent)`, 5000);
  const n = await h.js(`return new Promise((d) => invoke("term_lines", { conn: arguments[0].conn, sessionId: arguments[0].sid, hi: 40, lo: 0 }).then((x) => d(x.split("\\n").length)))`, t);
  if (n < 30) throw new Error("term_lines curto: " + n);
  await key("End");
};

// #26: F5 (fora do terminal) e Ctrl+Shift+R reanexam a sessão ativa com as dimensões atuais do painel.
export default async (h) => {
  const n0 = await h.js(`return tabs.length`);
  await h.js(`newSession(LOCAL)`);
  await h.until(`return tabs.length > ${n0} && activeTab()?.sid`, 15000);
  // __TAURI_INTERNALS__ é só leitura; no WebView2 a IPC sai por chrome.webview.postMessage, então espia ali.
  await h.js(`const w = window.chrome.webview, p = w.postMessage; window.__att = [];
    w.postMessage = function (m) { const o = typeof m === "string" ? JSON.parse(m) : m; if (o?.cmd === "term_attach") __att.push(o.payload); return p.apply(this, arguments); };
    window.__restore = () => { w.postMessage = p; }`);
  const want = await h.js(`const t = activeTab(), { cols, rows } = size($("screen")); return { conn: t.conn, sessionId: t.sid, cols, rows }`);
  const fire = (o) => h.js(`__att.length = 0; document.activeElement?.blur?.();
    document.dispatchEvent(new KeyboardEvent("keydown", Object.assign({ bubbles: true, cancelable: true }, arguments[0])))`, o);
  const got = () => h.until(`return __att.length && __att[0]`, 5000);
  try {
    for (const o of [{ key: "F5" }, { key: "R", ctrlKey: true, shiftKey: true }]) {
      await fire(o);
      const a = await got();
      h.eq([a.conn, a.sessionId, a.cols, a.rows], [want.conn, want.sessionId, want.cols, want.rows], `term_attach após ${o.key}`);
    }
  } finally { await h.js(`__restore()`); }
};

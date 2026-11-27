// 023 (v0.2.4): menu de contexto legível: texto, atalho, ícone e rótulo com contraste >= 4.5 e fonte >= 13px.
export default async (h) => {
  const sid = await h.js(`return conns[LOCAL].sessions[0]?.session_id`);
  if (!sid) { await h.js(`newSession(LOCAL)`); await h.until(`return conns[LOCAL].sessions.length`, 15000); }
  await h.js(`const L = layoutOf(LOCAL); L.group[conns[LOCAL].sessions[0].session_id] = "G023"; renderSidebar()`);
  const hd = `[...document.querySelectorAll("#local-ul .group-h")].find((b) => b.title === "G023")`;
  await h.until(`return ${hd}`, 3000);
  await h.js(`${hd}.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }))`);
  await h.until(`return document.querySelector(".menu [role=menuitem]")`, 3000);
  const bad = await h.js(`
    const rgb = (s) => (s.match(/[\\d.]+/g) || []).map(Number);
    const lum = ([r, g, b]) => { const f = (c) => (c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
    const m = document.querySelector(".menu"), bg = rgb(getComputedStyle(m).backgroundColor);
    const out = [];
    for (const e of m.querySelectorAll(".m-t, .m-k, .m-ic, .m-lbl, [role=menuitem]")) {
      if (!e.textContent.trim() || e.querySelector(".m-t")) continue;
      let a = 1; for (let x = e; x && x !== m; x = x.parentElement) a *= +getComputedStyle(x).opacity;
      const c = rgb(getComputedStyle(e).color); a *= c[3] ?? 1;
      const fg = c.slice(0, 3).map((v, i) => v * a + bg[i] * (1 - a));
      const [L1, L2] = [lum(fg), lum(bg)].sort((p, q) => q - p), cr = (L1 + 0.05) / (L2 + 0.05);
      if (cr < 4.5) out.push("contraste " + cr.toFixed(2) + ": " + e.className + " " + e.textContent.trim());
      if (parseFloat(getComputedStyle(e).fontSize) < 13) out.push("fonte " + getComputedStyle(e).fontSize + ": " + e.className + " " + e.textContent.trim());
    }
    return out`);
  h.eq(bad, [], "menu ilegível");
  await h.js(`document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); const L = layoutOf(LOCAL); delete L.group[conns[LOCAL].sessions[0].session_id]; renderSidebar()`);
};

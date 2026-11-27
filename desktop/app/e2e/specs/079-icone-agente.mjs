// 025 (v0.2.4): ícone do agente ocupa bem o card da lista: altura >= 60% da do card e inicial >= 50% do ícone.
export default async (h) => {
  await h.js(`conns[LOCAL].sessions.push({ session_id: "fake025", label: "claude", agent: "claude" }); renderSidebar()`);
  const r = await h.js(`
    const i = document.querySelector("#local-ul .ag-ico"); if (!i) return "sem ícone";
    const c = i.closest("li > button, li").getBoundingClientRect(), b = i.getBoundingClientRect();
    return { ico: b.height, card: c.height, fonte: parseFloat(getComputedStyle(i).fontSize) }`);
  await h.js(`conns[LOCAL].sessions = conns[LOCAL].sessions.filter((s) => s.session_id !== "fake025"); renderSidebar()`);
  h.eq(typeof r === "object" && r.ico >= r.card * 0.6 && r.fonte >= r.ico * 0.5, true, "ícone pequeno no card: " + JSON.stringify(r));
};

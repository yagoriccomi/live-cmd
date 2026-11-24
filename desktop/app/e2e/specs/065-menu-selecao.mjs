// #65/#66 (023): Shift+clique direito abre o menu sem mexer na seleção; com seleção, o menu exporta só o trecho.
export default async (h) => {
  await h.until(`return rowsEl().some((r) => r.textContent.trim())`, 15000);
  await h.js(`pane().focus(); selectAll()`);
  const before = await h.js(`return getSelection().toString()`);
  if (!before.trim()) throw new Error("sem seleção para testar");
  const r = await h.js(`const s = pane(), b = s.getBoundingClientRect(), o = { bubbles: true, cancelable: true, shiftKey: true, button: 2, clientX: b.left + 30, clientY: b.top + 30 };
    const d = s.dispatchEvent(new MouseEvent("mousedown", o)); s.dispatchEvent(new MouseEvent("contextmenu", o));
    return { blocked: !d, menu: !$("menu").hidden, exp: [...$("menu").querySelectorAll("button")].some((x) => /Exportar seleção/.test(x.textContent + (x.title || "") + (x.getAttribute("aria-label") || ""))) }`);
  h.eq([r.blocked, r.menu, r.exp], [true, true, true], "menu com seleção");
  h.eq(await h.js(`return getSelection().toString()`), before, "seleção depois do menu");
  await h.js(`closeMenu(false); getSelection().removeAllRanges()`);
};

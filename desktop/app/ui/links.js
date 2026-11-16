// Links clicáveis (F2): URL http(s) e caminho Windows na linha da tela.
const LINK_RE = /https?:\/\/[^\s"'<>`]+|[A-Za-z]:\\[^\s"'<>|*?]*/g;
/** Link que cobre a coluna `i` de `text`, ou null. Tira pontuação final. */
function linkAt(text, i) {
  for (const m of text.matchAll(LINK_RE)) {
    const s = m[0].replace(/[.,;:!?)\]}]+$/, "");
    if (i >= m.index && i < m.index + s.length) return s;
  }
  return null;
}
if (typeof module !== "undefined") module.exports = { linkAt };

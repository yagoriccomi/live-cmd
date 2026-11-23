// Contraste WCAG dos pares texto/fundo do Aurora. Fundo: #140b2e e mancha roxa (#8b5cf6 a 50%) sob vidro branco 8%.
const hex = h => h.match(/\w\w/g).map(x => parseInt(x, 16));
const mix = (a, b, t) => a.map((v, i) => v * (1 - t) + b[i] * t);
const lum = c => { const [r, g, b] = c.map(v => { v /= 255; return v <= .03928 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4; }); return .2126 * r + .7152 * g + .0722 * b; };
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + .05) / (y + .05); };
const base = hex('140b2e'), roxo = mix(base, hex('8b5cf6'), .5), W = [255, 255, 255];
const fundos = { escuro: mix(base, W, .08), roxo: mix(roxo, W, .08) };
const textos = { text: 'f4efff', muted: 'd6c8ff', off: process.argv[2] || 'cdc3ea', term: 'e6dcff' };
let falhou = 0;
for (const [t, c] of Object.entries(textos)) for (const [f, bg] of Object.entries(fundos)) {
  const r = ratio(hex(c), bg); if (r < 4.5) falhou++; console.log(t.padEnd(6), f.padEnd(7), r.toFixed(2));
}
console.log('card ativo', ratio(hex('2a1460'), W).toFixed(2));
process.exitCode = falhou ? 1 : 0;

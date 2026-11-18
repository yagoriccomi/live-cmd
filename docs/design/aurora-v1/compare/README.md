# Comparação pixel a pixel: mockup V1 Aurora × app desktop

Gerado por `desktop/app/scripts/pixel/run.py` (Edge headless, 1440×900, stub de `window.__TAURI__` com os dados dos mockups).
Para cada tela: `<Tela>-mock.png`, `<Tela>-app.png` e `<Tela>-diff.png` (rosa = pixel com algum canal diferente em mais de 32).
PNGs quantizados em 256 cores para caber no repositório; a medida foi feita nas capturas originais.

| Tela | Diferença |
|---|---|
| Terminal | 6,66% |
| Agentes | 9,27% |
| SSH | 4,71% |
| Histórico | 6,26% |
| Estados | 11,17% |
| Config | 23,44% |
| Pareamento | 15,23% |

Rodar de novo: `python desktop/app/scripts/pixel/run.py <pasta-de-trabalho> [Tela ...]`.

Config e Pareamento agora são painéis da janela do terminal. O que sobra de diferença nelas é conteúdo real
no lugar do conteúdo ilustrativo do mockup: o formulário de servidor, rolagem e perfis em vez de "Fonte",
"Opacidade" e "Desfoque" (que o app não tem), três temas em vez de quatro, e os passos de pareamento sem a
etapa "Confirme os números", que o protocolo não tem.

# Comparação pixel a pixel: mockup V1 Aurora × app desktop

Gerado por `desktop/app/scripts/pixel/run.py` (Edge headless, 1440×900, stub de `window.__TAURI__` com os dados dos mockups).
Para cada tela: `<Tela>-mock.png`, `<Tela>-app.png` e `<Tela>-diff.png` (rosa = pixel com algum canal diferente em mais de 32).
PNGs quantizados em 256 cores para caber no repositório; a medida foi feita nas capturas originais.

| Tela | Diferença |
|---|---|
| Terminal | 6,58% |
| Agentes | 9,27% |
| SSH | 4,71% |
| Histórico | 6,16% |
| Estados | 11,11% |
| Config | 40,69% |
| Pareamento | 34,02% |

Rodar de novo: `python desktop/app/scripts/pixel/run.py <pasta-de-trabalho> [Tela ...]`.

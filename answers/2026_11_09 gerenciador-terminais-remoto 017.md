# Decisões — gerenciador-terminais-remoto — 2026-11-09

**Contexto:** `/perguntas-simples "Onde paramos?"` após D3a e M3a concluídas e verificadas; pendências do `PLANO-M3.md`.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Terminal no PC | "No PC o terminal não deve aparecer, roda em segundo plano, não atrapalha quem usa o PC" (resposta livre) | ConPTY sem console visível (D3c) |
| 2 | Bloqueante | Tamanho com vários aparelhos | Último que digitou ou redimensionou | Regra de resize de D3b fechada |
| 3 | Bloqueante | Deps `windows` e `vt100` | Liberar as duas | D3c e D3d desbloqueadas (download único) |
| 4 | Bloqueante | Aprovar `PLANO-M3.md` | Aprovado | Status APROVADO |
| 5 | Importante | Commit | Commit e push | Commit do trabalho em `main` e push para `origin` |

**Confirmações:** decisões de answers 015 (tamanho independente por aparelho, ConPTY direto, `vt100`, terminal nativo, perfis em arquivo) continuam valendo.
**Premissas:** nenhuma.
**Ainda em aberto:** referência "9remote" não verificada; gates M1/M2 em aparelho real (travam M3b, M3c, D3c pela fila e o aceite M3).

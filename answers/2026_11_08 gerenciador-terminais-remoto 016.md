# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** `/loop` "pode executar tudo que não depende de mim até eu mandar parar ou seu trabalho acabar".
**Modo:** executar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Crítico | Liberar fatias de M3 sem pendência do usuário | Liberado (resposta livre) | D3a e M3a (puras, sem deps novas, sem rede) executadas com testes locais |

**Confirmações:** sem commit, push ou deploy.
**Premissas:** "não depende de mim" exclui: D3b (resize com vários aparelhos em aberto), D3c/D3d (deps novas: crate `windows`, `vt100`), M3b/M3c (aparelho real, gates M1/M2), aprovação formal do `PLANO-M3.md`.
**Ainda em aberto:** qual resize vence com vários aparelhos; referência "9remote"; liberação de deps de D3c/D3d.

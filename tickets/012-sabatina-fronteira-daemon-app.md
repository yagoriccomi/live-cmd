# Ticket 012 — O que fica no daemon e o que fica no app

**Labels:** `wayfinder:grilling` · **Pai:** [mapa-daemon-terminais](mapa-daemon-terminais.md) · **Responsável:** Claude (premissa do loop) · **Status:** Fechado · **Bloqueado por:** [010](010-pesquisa-conpty-fora-do-app.md)

## Pergunta

Qual a fronteira entre daemon e app: o daemon leva só as Sessões PTY, ou também o Canal P2P, sinalização, pareamento, Transferência Resumível e `sessions.json`? Qual o contrato de IPC local entre eles (transporte, autenticação, versionamento)? O celular deve continuar conectado com o app desktop fechado?

## Resolução

Premissas do modo loop (recomendadas), a revisar: `answers/2026_11_21 gerenciador-terminais-remoto 023.md`.

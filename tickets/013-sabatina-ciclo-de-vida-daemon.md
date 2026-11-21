# Ticket 013 — Ciclo de vida e atualização do daemon

**Labels:** `wayfinder:grilling` · **Pai:** [mapa-daemon-terminais](mapa-daemon-terminais.md) · **Responsável:** Claude (premissa do loop) · **Status:** Fechado · **Bloqueado por:** [011](011-pesquisa-updater-e-processos.md), [012](012-sabatina-fronteira-daemon-app.md)

## Pergunta

Quem inicia o daemon (autostart, app sob demanda), quem o encerra ("Sair" da bandeja fecha terminais?), e quando o daemon é atualizado: só quando o contrato de IPC muda, com aviso ao usuário? Como fica a migração da versão atual (tudo num processo) para a primeira com daemon?

## Resolução

Premissas do modo loop (recomendadas), a revisar: `answers/2026_11_21 gerenciador-terminais-remoto 023.md`.

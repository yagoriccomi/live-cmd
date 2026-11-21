# Mapa de Caminho: Terminais que sobrevivem à atualização do app

**Labels:** `wayfinder:map`
**Tracker:** markdown local (`tickets/`)
**Status do Mapeamento:** Concluído — execução em `docs/planos/PLANO-daemon-terminais.md`

## Destino

Spec aprovada da separação entre **processo de terminais** (daemon que mantém as Sessões PTY e o Canal P2P vivos) e **app desktop** (UI/cliente), de modo que atualizar, fechar ou reabrir o app não encerre os programas rodando nos terminais. Pronta para virar fatias de implementação.

## Notas

- **Domínio:** glossário em `CONTEXT.md`; arquitetura atual em `006-spec-arquitetura-host-rust.md`.
- **Estado atual:** `desktop/app/src/main.rs` chama `Service::start` (crate `live-cmd-echo`, `desktop/src/lib.rs`) dentro do processo Tauri; o updater (`tauri_plugin_updater`, NSIS) fecha o processo e leva junto o ConPTY e os filhos. A retomada hoje recria abas com pasta e saída salvas (`sessions.json`), mas os programas morrem.
- **Skills:** `sabatina-mp` e `modelagem-de-dominio-mp` nos tickets de sabatina; `pesquisar-mp` nos de pesquisa.
- Planejamento apenas; nada de implementação dentro do mapa.

## Decisões até aqui

<!-- uma linha por ticket fechado -->

- [ConPTY vivo fora do processo do app (010)](010-pesquisa-conpty-fora-do-app.md): processo de usuário destacado em `%LOCALAPPDATA%`, named pipe com DACL do usuário; Serviço do Windows descartado.
- [Updater/NSIS frente a um daemon em execução (011)](011-pesquisa-updater-e-processos.md): daemon em `%LOCALAPPDATA%\live-cmd\daemon\<versao>\`; troca só quando o IPC muda, versão antiga coexiste até esvaziar.
- [O que fica no daemon e o que fica no app (012)](012-sabatina-fronteira-daemon-app.md): daemon leva todo o `Service` e o `Provider`; IPC por named pipe com as mensagens do protocolo 007 + RPC de serviço (premissa).
- [Ciclo de vida e atualização do daemon (013)](013-sabatina-ciclo-de-vida-daemon.md): app sobe o daemon; "Sair" fecha só o app; "Encerrar terminais" na bandeja; troca só com `ipc_version` nova (premissa).

## Ainda não especificado

- Migração: como a primeira versão com daemon herda as sessões do app antigo (provavelmente uma última reinicialização inevitável).
- Atualização do próprio daemon: quando ele precisa reiniciar e como avisar o usuário.
- Impacto em testes (`cargo test` do host roda o serviço no mesmo processo).
- Bandeja, autostart e "Sair": quem é dono de cada um após a separação.

## Fora de escopo

- Sobreviver a reinício/desligamento do Windows (limite já aceito na decisão 022).
- Mudanças no app mobile e no protocolo P2P além do necessário.

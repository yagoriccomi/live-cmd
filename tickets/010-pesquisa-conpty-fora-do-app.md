# Ticket 010 — ConPTY vivo fora do processo do app

**Labels:** `wayfinder:research` · **Pai:** [mapa-daemon-terminais](mapa-daemon-terminais.md) · **Responsável:** Claude · **Bloqueado por:** — · **Status:** Fechado

## Pergunta

No Windows 11, quais formas um processo separado do app Tauri pode manter as sessões ConPTY (`portable-pty`) e seus filhos vivos enquanto o app fecha e reinstala? Comparar: processo de usuário destacado (`DETACHED_PROCESS`/`CREATE_BREAKAWAY_FROM_JOB`), Serviço do Windows (sessão 0 — ConPTY e programas de usuário funcionam lá?), e tarefa agendada no logon. Para cada: custo, limitações (sessão de usuário, variáveis de ambiente, credenciais, `claude`/`codex` rodando), e como o app reconecta (named pipe local e controle de acesso).

## Resolução

Relatório completo: `pesquisas/010-conpty-fora-do-app.md` na branch `research/conpty-fora-do-app` (commit `55b972a`).

**Recomendado: processo de usuário destacado.** Binário separado (`live-cmd-termd`) dono do ConPTY, do `Service` e do Canal P2P; o app vira cliente.
- Lançado com `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB` (sem breakaway se o job do pai negar).
- Executa de cópia versionada em `%LOCALAPPDATA%`, fora de `$INSTDIR`: o NSIS nunca sobrescreve exe em uso.
- IPC por named pipe com `FILE_FLAG_FIRST_PIPE_INSTANCE`, `PIPE_REJECT_REMOTE_CLIENTS` e DACL só do SID do usuário; handshake com token em arquivo de ACL do usuário; app confere PID do servidor.
- Autostart via `HKCU\Run` existente também sobe o daemon.

**Descartado:** Serviço do Windows (sessão 0, exige LocalSystem + `WTSQueryUserToken`, UAC, `keyring`/`HKCU` por usuário). Tarefa agendada no logon fica como complemento opcional (limite de 72 h, possível encerramento de filhos — não verificado).

**Fato lateral:** o host usa wrapper próprio `desktop/src/conpty.rs`, não `portable-pty`.

**Lacunas a validar em protótipo:** casamento do `CheckIfAppIsRunning` do NSIS do Tauri; `GetNamedPipeServerProcessId` em fonte primária; seção 9 do relatório (5 validações).

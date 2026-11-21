# PLANO — daemon de terminais separado do app (v0.1.22)

Origem: mapa `tickets/mapa-daemon-terminais.md` (tickets 010–013), premissas em `answers/2026_11_21 gerenciador-terminais-remoto 023.md`. Pesquisas: branches `research/conpty-fora-do-app` (`pesquisas/010-conpty-fora-do-app.md`) e `research/updater-e-processos` (`pesquisas/011-updater-e-processos.md`).

## Objetivo

- **Deve:** fechar, reabrir ou atualizar o app desktop sem encerrar as Sessões PTY nem os programas dentro delas; o celular continua conectado com o app fechado; o app reconecta às mesmas abas.
- **Não deve:** mudar o protocolo P2P/mobile; exigir admin; quebrar a retomada atual (pasta/saída) quando o daemon é reiniciado.
- **Oráculo:** (1) `cargo test --lib -p live-cmd-echo -j 2 -- --test-threads=4` passa; (2) teste novo de IPC: cliente pelo pipe cria sessão, desconecta, reconecta e reencontra a sessão com o processo vivo; (3) teste/roteiro: matar o processo do app não mata o daemon nem o filho do ConPTY; (4) `cargo build --release` do app e do daemon.

## Fatias

| Fatia | Escopo | Nível | Estado |
|---|---|---|---|
| A | Daemon `live-cmd-termd` (bin no crate `live-cmd-echo` ou crate novo), servidor de named pipe (DACL do usuário, token, `ipc_version`), `Wire::Pipe` no `Client`, RPC de serviço; app sobe/conecta o daemon (cópia em `%LOCALAPPDATA%\live-cmd\daemon\<versao>\`, `DETACHED_PROCESS\|CREATE_NEW_PROCESS_GROUP\|CREATE_BREAKAWAY_FROM_JOB`), `Provider` no daemon, "Encerrar terminais" na bandeja, bundle do daemon no instalador, hooks NSIS `PREUNINSTALL` | alto | feito (6 commits, `3ccef1d`..`df1888a`) |
| B | Pendências pequenas: fixar versão do `@tauri-apps/cli` no `release.ps1`; `app.restart()` morto em `install_update` (`main.rs:73-76`) no Windows; label com mojibake em `sessions.json` (`live-cmd Â· claude`) | medio | feito (`e8ed3da` `818052a` `4ff07e5` `8aaa654`; mojibake: arquivo real estava limpo, defesa `fix_mojibake`) |
| C | Limpeza de worktrees/branches de agentes desta sessão | coordenador | feito (5 worktrees e 7 branches locais; pesquisas seguem no origin) |
| E | Modo dividido estilo VS Code: divisor arrastável, colar/arrastar no painel focado, aba única para o par, fechar por painel, foco, orientação, maximizar, dividir com terminal novo | medio | feito (`53a9ea9` `ab1c10f`; inclui seleção por painel) |
| D | Bateria, release v0.1.22 (desktop + APK) | coordenador | feito (host 176 ok, app 17 ok; v0.1.22 publicada) |

## Regras para os agentes

- Worktree próprio; `CARGO_TARGET_DIR` próprio fora do repositório; `cargo` com `-j 2` e testes com `--test-threads=4` (memória curta nesta máquina).
- `--exact` exige o caminho completo do teste (`tests::x`).
- Commits atômicos em Conventional Commits terminando com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Sem push, sem PR.
- Relatório: o que mudou, testes rodados, o que não foi verificado.

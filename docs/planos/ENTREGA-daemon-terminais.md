# ENTREGA — daemon de terminais + modo dividido (v0.1.22)

Plano: `docs/planos/PLANO-daemon-terminais.md`. Premissas: `answers/2026_11_21 gerenciador-terminais-remoto 023.md`. Release: https://github.com/yagoriccomi/live-cmd/releases/tag/v0.1.22 (sem PR: push direto no `main`, autorizado no handoff 002).

| Fatia | Commits | Resultado |
|---|---|---|
| A | `3ccef1d` `c0a5024` `d76c786` `46947a2` `40ee36c` `df1888a` | Daemon `live-cmd-termd` com `Service` + provedor; IPC por named pipe (`ipc.rs`, DACL do usuário, token, `ipc_version`); app usa `Client::pipe`/`ServiceClient`; cópia em `%LOCALAPPDATA%\live-cmd\daemon\<versão>\`; "Encerrar terminais…" na bandeja; `externalBin` + `PREUNINSTALL`. Dados do app movidos para `desktop/src/appdata/`. |
| B | `a3b4a9b` `423577f` `fd8416d` `80290b4` | CLI do Tauri fixado em 2.12.5; `app.restart()` morto removido; `fix_mojibake` em criar/renomear/restaurar sessão. |
| C | — | 5 worktrees e 7 branches locais de agentes removidos. |
| E | `b2caf09` `3677128` | Modo dividido estilo VS Code, incluindo seleção confinada ao painel. |

## Verificação

- `cargo test --lib -p live-cmd-echo -j 2 -- --test-threads=4`: 176 passed, 0 failed, 11 ignored.
- `cargo test -j 2 -- --test-threads=4` em `desktop/app`: 17 passed.
- Teste novo `client::tests::pipe_sessao_sobrevive_a_reconexao`.
- `release.ps1`: instalador assinado 0.1.22 com `live-cmd-termd` empacotado; `latest.json` 0.1.22. APK gerado.

## Não verificado

- App rodando: fechar/reabrir/atualizar com abas reanexando; instalar/desinstalar; fallback sem `CREATE_BREAKAWAY_FROM_JOB`; filhos ao `shutdown` do daemon.
- Modo dividido no app rodando (divisor, colar no painel direito, seleção, vertical, arrastar aba).
- Riscos aceitos (comentários no código): saída acumula no daemon se o app travar; avisos do celular perdidos com o canal de avisos caído; corrida em `agents.json` entre app e daemon.
- A atualização de 0.1.21 para 0.1.22 ainda reinicia os terminais uma vez (migração).

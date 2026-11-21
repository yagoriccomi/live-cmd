# Ticket 011 — Updater/NSIS frente a um daemon em execução

**Labels:** `wayfinder:research` · **Pai:** [mapa-daemon-terminais](mapa-daemon-terminais.md) · **Responsável:** Claude · **Bloqueado por:** — · **Status:** Fechado

## Pergunta

Como o `tauri-plugin-updater` e o instalador NSIS do Tauri 2 tratam um segundo executável em execução (sidecar ou binário separado) na pasta de instalação? É possível atualizar só o app sem tocar no daemon (binário em pasta versionada, hooks NSIS `PREINSTALL`, arquivo travado)? Que mecanismo outros apps usam (ex.: VS Code server, WezTerm mux, tmux-like no Windows) para trocar o daemon só quando necessário?

## Resolução

Relatório completo: `pesquisas/011-updater-e-processos.md` na branch `research/updater-e-processos` (commit `188d248`).

**Recomendado: daemon em pasta versionada fora do `$INSTDIR`** (`%LOCALAPPDATA%\live-cmd\daemon\<versao>\`), coerente com o ticket 010.
- `tauri-plugin-updater` 2.14.1 roda o setup NSIS com `/UPDATE` e faz `exit(0)` só no app; o NSIS só verifica o exe principal (bundler ≥ 2.10.0 via Restart Manager; 2.7.0 por nome de imagem); `/UPDATE` sobrescreve sem desinstalar.
- Risco real: `File` do NSIS falha ao sobrescrever `.exe` em execução — por isso o daemon não vive no `$INSTDIR`.
- Troca do daemon só quando o protocolo de IPC muda (handshake de versão); o antigo coexiste até esvaziar (modelo VS Code server).
- Rede de segurança: `installerHooks` `PREINSTALL` renomeia exe em uso; `PREUNINSTALL` encerra o daemon; `on_before_exit` do updater avisa o daemon.

**Descartado:** mesmo exe com `--daemon` (Restart Manager fecha junto); deixar o instalador matar (derrota o objetivo). WezTerm: troca de versão do mux mata sessões — evitar esse modelo.

**Fatos laterais:** `release.ps1` usa `npx @tauri-apps/cli` sem versão fixa (template NSIS varia); `app.restart()` em `install_update` (`main.rs:73-76`) nunca roda no Windows.

**Lacuna:** sem teste real de `File` sobre exe em uso com `/P` + `/UPDATE`.

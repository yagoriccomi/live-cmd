# Pesquisa 011 — Updater/NSIS frente a um daemon em execução

Ticket: `tickets/011-pesquisa-updater-e-processos.md` (mapa: `mapa-daemon-terminais`). Data: 2026-11-21.
Versões do projeto (`desktop/app/Cargo.lock`): `tauri 2.12.5`, `tauri-plugin-updater 2.14.1`, `tauri-build 2.7.4`. O `release.ps1` usa `npx @tauri-apps/cli` sem versão fixa, então o template NSIS efetivo é o do CLI mais novo no momento do build.

## 1. Resumo e recomendação

**Dá para atualizar só o app sem tocar no daemon, desde que o daemon NÃO seja um arquivo do instalador.** O updater e o NSIS do Tauri só encerram o processo do app principal; um segundo `.exe` em execução não é morto. O problema real é outro: o NSIS sobrescreve todo arquivo que ele próprio instala em `$INSTDIR`, e o Windows não deixa sobrescrever um `.exe` em execução.

Recomendação (por ordem):

1. **Daemon fora do `$INSTDIR`, em pasta versionada, fora do alcance do NSIS.** O app (ou o próprio instalador via hook) copia o binário do daemon para `%LOCALAPPDATA%\live-cmd\daemon\<versao>\live-cmd-daemon.exe` e o inicia de lá. O instalador distribui a cópia "semente" (como recurso ou `[[bin]]` extra), mas o daemon em execução roda da cópia versionada. Atualizar o app nunca encosta no arquivo em uso. Mesmo padrão do VS Code Server (pasta por commit) e do WezTerm (daemon independente, cliente e servidor com versões compatíveis).
2. **Daemon só troca quando o protocolo app↔daemon muda.** O app novo faz handshake com versão de protocolo. Compatível: reaproveita o daemon vivo. Incompatível: sobe um daemon novo em pasta nova ao lado; o antigo continua servindo as sessões existentes até esvaziar (ou o usuário aceitar a reinicialização). Nunca matar o antigo silenciosamente.
3. **Rede de segurança no instalador:** `installerHooks` com `NSIS_HOOK_PREINSTALL` que renomeia (não apaga) qualquer `.exe` do daemon que esteja em `$INSTDIR` (renomear executável em uso é permitido no Windows) e `NSIS_HOOK_PREUNINSTALL` que pede ao daemon para encerrar (só na desinstalação explícita; em `/UPDATE` o uninstaller nem roda).
4. Usar `on_before_exit` do updater para avisar o daemon/desanexar a UI antes do `std::process::exit(0)`.

Pergunta em aberto para a sabatina: no formato 1, quem copia a semente para a pasta versionada (app na primeira execução, ou hook POSTINSTALL)? Recomendo o app, porque o hook não conhece a versão de protocolo e o app já precisa falar com o daemon.

## 2. Achados (fatos verificados na fonte)

### 2.1 `tauri-plugin-updater` no Windows
- `install_inner` extrai o instalador NSIS, roda `on_before_exit` se houver, dispara o setup com `ShellExecuteW` e chama **`std::process::exit(0)`** no processo atual. Nada além do processo do app (o que chama `download_and_install`) é encerrado pelo plugin. Os argumentos do NSIS são `installMode` (`/P` passivo), **`/UPDATE`** e, se `restart_after_install`, `/ARGS ...`. [updater.rs]
- Doc: "On Windows the application is automatically exited when the install step is executed due to a limitation of Windows installers" e o hook `on_before_exit` serve para agir antes disso. [docs updater]
- Implicação para o live-cmd: `install_update` em `desktop/app/src/main.rs:73-76` chama `download_and_install` e depois `app.restart()`; no Windows o `restart()` nunca roda (o `exit(0)` vem antes). `process::exit` não executa os destrutores do Rust nem `RunEvent::Exit`; hoje isso derruba `Service` e o ConPTY junto, sem limpeza.

### 2.2 Instalador NSIS (template do Tauri)
- **Verificação de "app rodando" olha só o binário principal.** Template atual (`includes/process.nsh`, desde bundler 2.10.0, PR #14479): usa o Restart Manager registrado **só em `$INSTDIR\${MAINBINARYNAME}.exe`**, e pede `RmShutdown` (envia `WM_ENDSESSION`, depois força). Em `/P` ou silencioso, encerra sem perguntar. Template antigo (bundler 2.7.0, `utils.nsh`): `FindProcessCurrentUser`/`KillProcessCurrentUser` pelo **nome de imagem** `${MAINBINARYNAME}.exe` (por usuário, em `installMode=currentUser`). Nos dois casos, um daemon com nome de exe diferente **não é morto**. [installer.nsi dev, process.nsh, utils.nsh v2.7.0]
  - Cuidado: o Restart Manager reporta "todo processo que tem o executável aberto". Se o daemon fosse o MESMO exe do app (ex.: `live-cmd.exe --daemon`), ele seria fechado junto. Por isso o daemon precisa ser um binário separado.
- **Arquivos instalados:** o instalador copia o principal, `resources` e `externalBin`/binários extras com `File /a` para `$INSTDIR` (`$LOCALAPPDATA\live-cmd` em `currentUser`). `File` em arquivo travado falha: o NSIS mostra Abortar/Tentar/Ignorar (`AllowSkipFiles`), então um daemon bundled e em execução quebra a atualização, silenciosa ou não. [installer.nsi linhas 403-425, 351; NSIS ref `SetOverwrite`/`AllowSkipFiles`]
- **Modo `/UPDATE` não desinstala a versão antiga:** a página `reinstall.nsh` faz "In update mode, always proceeds without uninstalling" e sobrescreve por cima. O desinstalador (que apaga principal, recursos e binários e roda `PREUNINSTALL`) só roda na desinstalação explícita ou ao escolher "desinstalar antes". [reinstall.nsh linhas 208-246, installer.nsi 557-575]
- **Hooks suportados:** `NSIS_HOOK_PREINSTALL`, `POSTINSTALL`, `PREUNINSTALL`, `POSTUNINSTALL`, via `bundle.windows.nsis.installerHooks` apontando para um `.nsh`. `PREINSTALL` roda antes do `CheckIfAppIsRunning` (bundler 2.0.1-beta.19). [docs windows-installer; CHANGELOG bundler]
- O live-cmd hoje não tem hooks em `desktop/app/tauri.conf.json` (só `installMode: currentUser`, `createUpdaterArtifacts`, updater `installMode: passive`).

### 2.3 Windows: arquivo em uso
- Executável mapeado em memória (image section): **escrever ou apagar falha** com `ERROR_SHARING_VIOLATION`/`ACCESS_DENIED`; **renomear funciona**, porque o loader abre o arquivo com `FILE_SHARE_DELETE`. [Elastic/Process Ghosting; Raymond Chen; issues golang/rustup]
- `Rename`/`Delete` com `/REBOOTOK` do NSIS agendam para o próximo boot e exigem administrador; em `currentUser` não servem. [NSIS ref 4.9.1]
- Padrão consagrado: renomear o exe antigo e gravar o novo no caminho original. O CLI do VS Code faz exatamente isso na autoatualização (renomeia o exe em uso para a pasta temporária ou `.old`, e `cleanup_old_update` apaga depois "May fail if old versions are still running"). [vscode cli/src/self_update.rs]

### 2.4 O que outros apps fazem
- **VS Code Server / `code tunnel`:** cada versão do servidor fica em pasta própria `<quality>-<commit>` (cache do launcher); o cliente pede um commit específico e servidores novos ignoram pedido de commit antigo; `prune_stopped_servers` só apaga as pastas cujo servidor **não está rodando** (`get_running_pid().is_none()`). Ou seja: versões coexistem e a velha só some quando esvazia. [vscode cli/src/tunnels/paths.rs, control_server.rs, code_server.rs]
- **WezTerm mux:** daemon separado (`wezterm-mux-server --daemonize`), fala por unix domain socket (suportado também no Windows). Doc exige "versão compatível" entre cliente e servidor; em divergência o erro pede "same version"; a prática da comunidade é reiniciar o mux, **o que mata as sessões**. Ou seja, o WezTerm não resolve a troca de daemon sem perda; é a lacuna que o live-cmd precisaria tratar com handshake de versão. [wezterm.org/multiplexing; issue wez/wezterm#5200]
- **tmux:** mesma conclusão (após atualizar, o servidor velho continua com o protocolo velho; a recomendação é reiniciar tudo). Não há equivalente tmux nativo no Windows com ConPTY; este ponto é do ticket 010.

## 3. Alternativas

| # | Alternativa | Prós | Contras |
|---|---|---|---|
| A | **Daemon em pasta versionada fora do `$INSTDIR` (recomendada)** | O NSIS nunca toca no exe em uso; versões coexistem; reversível; sem admin; padrão VS Code | App/daemon precisa gerenciar a cópia e a limpeza de versões velhas; duas cópias em disco (semente + versionada) |
| B | Daemon como `externalBin`/`[[bin]]` em `$INSTDIR` + hook `PREINSTALL` que renomeia o exe em uso | Layout simples, uma fonte do binário, instalador resolve sozinho | Depende de hook NSIS (difícil de testar); sobra `.old` em uso até o daemon sair; `File` ainda falha se outro processo segurar o arquivo; desinstalação apaga o exe em uso (precisa parar o daemon) |
| C | Instalador mata o daemon (comportamento padrão se for o mesmo exe do app) | Zero código extra | Derrota o objetivo: sessões morrem em toda atualização |
| D | Daemon e app no mesmo exe (`live-cmd.exe --daemon`) | Um só artefato | O Restart Manager/`taskkill` fecha o daemon junto; atualização trava no exe em uso |
| E | Serviço do Windows / tarefa agendada que lança o daemon de pasta própria | Reinício automático | Custo e limites de sessão (ticket 010); não resolve por si só a troca de arquivo |

Para trocar o daemon só quando necessário (qualquer alternativa): versão de protocolo no handshake, daemon antigo coexiste até esvaziar (modelo `prune_stopped_servers`), aviso ao usuário só quando a nova versão exigir.

## 4. Impacto no repo (sem alterar código)
- `desktop/app/src/main.rs`: `check_update`/`install_update` e `Service::start` no processo Tauri; `app.restart()` morto no Windows.
- `desktop/app/tauri.conf.json`: sem `installerHooks`; `installMode: currentUser` mantém `$INSTDIR` em `%LOCALAPPDATA%\live-cmd`.
- `desktop/app/release.ps1`: gera um único `-setup.exe`; o daemon novo só entraria no pacote como recurso/`[[bin]]` (alternativas A/B).

## 5. Lacunas e riscos não verificados
- Não testei em máquina real o `File` sobre exe em uso sob `/P`+`/UPDATE` (a leitura da fonte e da doc NSIS indica falha com diálogo Abortar/Tentar; vale um teste prático antes de escolher B).
- Não confirmei o comportamento do Restart Manager (`WM_ENDSESSION`) sobre um daemon sem janela se o daemon for o mesmo exe (alternativa D); a leitura do código sugere que fecha.
- O template NSIS efetivo depende do CLI usado no `release.ps1` (sem versão fixa); fixar a versão do `@tauri-apps/cli` evita mudança de comportamento (ex.: troca de `FindProcess` para Restart Manager em 2.10.0).
- Como o ConPTY sobrevive ao processo pai e como o app reconecta: ticket 010.

## 6. Fontes
- tauri-plugin-updater `updater.rs` (v2): https://raw.githubusercontent.com/tauri-apps/plugins-workspace/v2/plugins/updater/src/updater.rs
- Docs do updater: https://raw.githubusercontent.com/tauri-apps/tauri-docs/v2/src/content/docs/plugin/updater.mdx (https://v2.tauri.app/plugin/updater/)
- Docs do instalador Windows e hooks: https://raw.githubusercontent.com/tauri-apps/tauri-docs/v2/src/content/docs/distribute/windows-installer.mdx (https://v2.tauri.app/distribute/windows-installer/)
- Template NSIS: https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi , `includes/process.nsh`, `includes/pages/reinstall.nsh`; versão antiga: tag `tauri-bundler-v2.7.0` (`utils.nsh`)
- CHANGELOG do bundler (2.10.0 Restart Manager; 2.0.1-beta.19 ordem dos hooks): https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/CHANGELOG.md
- NSIS referência (Rename, Delete `/REBOOTOK`, `AllowSkipFiles`): https://nsis.sourceforge.io/Docs/Chapter4.html
- Microsoft, MoveFileEx: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
- Raymond Chen, renomear arquivo e `FILE_SHARE_DELETE`: https://devblogs.microsoft.com/oldnewthing/20211022-00/?p=105822
- Elastic, Process Ghosting (restrições de escrita/exclusão em imagem mapeada): https://www.elastic.co/blog/process-ghosting-a-new-executable-image-tampering-attack
- VS Code CLI: https://raw.githubusercontent.com/microsoft/vscode/main/cli/src/self_update.rs , `cli/src/tunnels/paths.rs`, `control_server.rs`, `code_server.rs`; doc https://code.visualstudio.com/docs/remote/tunnels
- WezTerm: https://wezterm.org/multiplexing.html ; issue https://github.com/wez/wezterm/issues/5200

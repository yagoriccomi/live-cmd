# Pesquisa Técnica: ConPTY vivo fora do processo do app (Ticket 010)

**Status:** Concluído (Veredito: processo de usuário destacado, binário separado, reconexão por named pipe com ACL restrita)
**Data:** 2026-11-21 · **Windows alvo:** 11 (ConPTY exige 10 1809+)

Convenção: **[V]** = verificado em fonte primária citada; **[I]** = inferência minha, a validar em protótipo; **[S]** = fonte secundária.

## 0. Fatos do código atual (relevantes)

- O host **não usa `portable-pty`**: `desktop/src/conpty.rs` é um wrapper próprio sobre `CreatePseudoConsole` + `CreateProcessW` (crate `windows`). `Cargo.toml` do host não depende de `portable-pty` (o ticket 001 recomendou, mas o código divergiu). Isso não muda a análise, só tira uma dependência da conta.
- `conpty.rs` fixa `FLAGS = EXTENDED_STARTUPINFO_PRESENT` e há teste que proíbe `DETACHED_PROCESS`/`CREATE_NO_WINDOW`/`CREATE_NEW_CONSOLE` **no filho do pseudoconsole**. Esses flags da pergunta valem para **lançar o daemon**, não para os filhos dos terminais. Sem conflito.
- `conpty.rs` chama `SetConsoleCtrlHandler(None, false)` antes de criar o filho porque herdar "Ctrl+C ignorado" (de `CREATE_NEW_PROCESS_GROUP`) quebraria o `0x03`. Se o daemon for lançado com `CREATE_NEW_PROCESS_GROUP`, a chamada já cobre o caso.
- `Service::start` (`desktop/src/lib.rs`) é chamado em `main.rs` (setup do Tauri, e em `restart`/troca de servidor). O host usa `keyring` (Credential Manager do usuário), WebRTC (Canal P2P) e `extras::set_provider` (callbacks do app: notificações, histórico).
- Autostart atual: valor em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (`settings::autostart`); `tauri_plugin_single_instance` ativo; bandeja própria.

## 1. Premissa: o ConPTY tem de viver no daemon

`CreatePseudoConsole` "aloca um novo pseudoconsole para o processo chamador" e recebe dois handles de pipe (`hInput`/`hOutput`) criados por quem chama [V: doc CreatePseudoConsole]. Os filhos ficam anexados ao pseudoconsole; quem fecha os pipes/pseudoconsole encerra a sessão. Logo, para os programas sobreviverem, **quem chama `CreatePseudoConsole` e segura os pipes precisa ser o processo que sobrevive**. Passar um ConPTY vivo do app para outro processo (duplicar `HPCON`/pipes) não é um caminho documentado [I]; descartado. O daemon é dono do ConPTY, do `Service` e do Canal P2P; o app vira cliente.

## 2. Alternativa A: processo de usuário destacado (RECOMENDADA)

**Como:** o app (ou o autostart) lança `live-cmd-termd.exe` com `CreateProcessW` e flags `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`, `bInheritHandles=false`. O daemon não precisa de console próprio (os filhos usam o pseudoconsole).

- `DETACHED_PROCESS`: processo sem console do pai. `CREATE_BREAKAWAY_FROM_JOB`: o filho não entra no job do pai; "se o chamador está num job, o job deve ter `JOB_OBJECT_LIMIT_BREAKAWAY_OK`", senão a criação falha [V: doc Process Creation Flags]. Portanto: tentar com o flag; se falhar com `ERROR_ACCESS_DENIED`, repetir sem ele e registrar log [I].
- **Job do pai:** se o app for iniciado por algo que o ponha num job "kill on close", o filho morre junto sem breakaway. Job "kill on close" encerra todos os processos quando o último handle do job fecha [V: Raymond Chen, devblogs 2013-12-09]. Filho de processo em job fica no job por padrão [V: mesma doc de flags].
- **Sessão/ambiente/credenciais:** roda na sessão interativa do usuário, com o token, perfil, `HKCU`, `%USERPROFILE%`, Credential Manager e variáveis exatamente como o app hoje. `claude`/`codex` (que leem `~/.claude`, `~/.codex`, login por navegador) funcionam como hoje [I, mesmo contexto de segurança do app atual].
- **Custo:** baixo. Sem admin, sem UAC, compatível com instalação NSIS `currentUser` (padrão, sem privilégio de administrador) e com o updater `passive`/`quiet` [V: docs Tauri windows-installer e updater]. Trabalho real: novo binário, protocolo app<->daemon, ciclo de vida (§6).
- **Limites:** morre com logoff/desligamento (já fora de escopo no mapa). Se o usuário matar o processo pelo Gerenciador de Tarefas, perde tudo (aceitável; é o comportamento de qualquer multiplexador).
- **Precedente:** multiplexadores com servidor separado (WezTerm `wezterm-mux-server`, que usa `portable-pty`) seguem o mesmo modelo; a doc oficial do WezTerm não detalha o mecanismo de daemonização no Windows [V parcial: wezterm.org/multiplexing], então não uso como prova técnica.

## 3. Alternativa B: Serviço do Windows

- Serviços rodam na **sessão 0**; UI de serviço só é visível ao usuário da sessão 0, e não se deve configurar serviço interativo com Terminal Services/troca rápida de usuário [V: doc Interactive Services]. `NoInteractiveServices` vale 1 por padrão [V].
- **ConPTY em serviço funciona** em princípio (é pseudoconsole sem janela); a dificuldade não é o ConPTY, é **rodar o programa como o usuário**. Para pegar o token do usuário logado é preciso `WTSQueryUserToken`, que exige conta **LocalSystem** com `SE_TCB_NAME` [V: doc WTSQueryUserToken]. Depois: `CreateEnvironmentBlock` + `CreateProcessAsUser` (+ perfil carregado) com o pseudoconsole [I; padrão usado por servidores de shell remoto, não confirmado em fonte primária nesta pesquisa; busca por sshd/ConPTY só achou issue sobre fallback do `ssh-shellhost`].
- **Custo alto:** instalar serviço exige **admin/UAC**. O instalador `perMachine`/`both` exige administrador [V: docs Tauri], e o updater `quiet` "não consegue pedir privilégio de admin" [V]. Quebra a atualização silenciosa por usuário que o app tem hoje. Serviço também precisa de ciclo de atualização próprio (parar/substituir/iniciar).
- **Limites específicos do live-cmd:** `keyring` (Credential Manager) é por usuário: um serviço SYSTEM enxerga outro cofre [I]; teria de impersonar. Sem usuário logado não há token (`WTSQueryUserToken` falha), logo os terminais não existiriam sem sessão. Para uma ferramenta pessoal por usuário, o benefício (sobreviver a logoff) já está declarado fora de escopo.
- **Reconexão:** igual à A (named pipe), mas com ACL mais delicada (serviço SYSTEM versus cliente de usuário).
- Veredito: complexidade e privilégio sem ganho dentro do escopo. **Não recomendado.**

## 4. Alternativa C: Tarefa Agendada no logon

- Roda na sessão do usuário (opção "executar somente quando o usuário estiver conectado"), então herda ambiente/credenciais como a A. "Executar estando conectado ou não" tira da sessão interativa (volta aos problemas da B) [I].
- **Armadilha do limite de tempo:** o padrão é parar a tarefa após **72 h**; `PT0S` no `ExecutionTimeLimit` desliga [V: doc TaskSettings.ExecutionTimeLimit via busca; schema ExecutionTimeLimit]. Também ajustar condições de bateria (`DisallowStartIfOnBatteries`, `StopIfGoingOnBatteries`) para notebook [I].
- **Armadilha de job:** relatos indicam que o Agendador encerra os filhos quando termina a tarefa, porque o processo roda em job com kill-on-close (crate `uv-windows` documenta isso) [S]. Se a tarefa lançar o daemon como processo principal, ok; se a tarefa só "dispara" o daemon, usar breakaway (mesma regra de `JOB_OBJECT_LIMIT_BREAKAWAY_OK`) ou o daemon ser o próprio processo da ação. Validar em protótipo [I].
- **Custo:** registrar tarefa por usuário (COM `ITaskService` ou `schtasks`); criar trigger de logon pode exigir elevação em alguns cenários [I, não verificado]. Ganho real: reinício automático em falha (`RestartOnFailure`) e controle de condições.
- O app **já** usa a chave `HKCU\...\Run` como autostart, mais barata e sem elevação. Para lançar o daemon no logon, o mesmo mecanismo basta. Tarefa agendada só se for desejado supervisão/restart automático.
- Veredito: alternativa válida como **complemento opcional**, não base.

## 5. Quadro comparativo

| Critério | A. Destacado (usuário) | B. Serviço | C. Tarefa no logon |
|---|---|---|---|
| Privilégio para instalar | nenhum | admin/UAC | nenhum a admin [I] |
| Sessão e ambiente do usuário | idêntico ao hoje | exige token via `WTSQueryUserToken` (SYSTEM+TCB) | idêntico (modo "conectado") |
| `claude`/`codex`, `keyring`, `HKCU` | funcionam | precisam de impersonação/perfil | funcionam |
| Compatível com updater `quiet` por usuário | sim | não | sim |
| Sobrevive a fechar/atualizar o app | sim | sim | sim |
| Sobrevive a logoff | não | parcial, só com token na sessão 0 [I] | não |
| Auto-restart do daemon | não (app relança) | sim (SCM) | sim (config) |
| Risco principal | job do pai mata filho | complexidade/privilégio | limite 72 h, job do Agendador |

## 6. Reconexão: named pipe local com controle de acesso

- Servidor: `CreateNamedPipe` com **`FILE_FLAG_FIRST_PIPE_INSTANCE`** (só um servidor) e **`PIPE_REJECT_REMOTE_CLIENTS`** ("conexões remotas são rejeitadas") [V: doc CreateNamedPipe]. Em tokio: `ServerOptions::first_pipe_instance(true)` (instâncias seguintes falham com `PermissionDenied`) e `reject_remote_clients(true)` (padrão já recusa remotos) [V: docs.rs tokio]. `create_with_security_attributes_raw` aceita `SECURITY_ATTRIBUTES` (é `unsafe`) [V].
- **DACL explícita é obrigatória.** O descritor padrão concede controle total a SYSTEM, administradores e dono, e **leitura ao grupo Everyone e ao anônimo** [V: doc Named Pipe Security and Access Rights]. Montar DACL só com o SID do usuário atual (e SYSTEM se quiser), via SDDL, por exemplo `D:P(A;;GA;;;<SID-do-usuário>)`.
- Nome com o SID do usuário (`\\.\pipe\live-cmd-<sid>`) para multiusuário/troca rápida.
- **Anti-impostor (lado cliente):** quem cria o pipe primeiro "ganha" o nome; `FIRST_PIPE_INSTANCE` protege o daemon, não o app. O app deve conferir que o servidor é o daemon esperado (`GetNamedPipeServerProcessId` + caminho/assinatura do executável) [I; função da API Win32, não verificada em fonte nesta pesquisa]. Defesa em profundidade: token aleatório gravado em arquivo com ACL só do usuário (`%LOCALAPPDATA%`), enviado no handshake. O daemon pode checar o cliente com `GetNamedPipeClientProcessId` (existe na API; página não recuperada nesta pesquisa).
- Descoberta/ciclo: app tenta conectar; se `ERROR_FILE_NOT_FOUND`, lança o daemon e reintenta com espera; `single_instance` do app e `FIRST_PIPE_INSTANCE` do daemon evitam duplicata.

## 7. Atualização e instalador (risco que decide o desenho)

- O updater do Tauri **encerra o app** antes de instalar (limitação do Windows) e oferece hook `on_before_exit` [V: docs Tauri updater]. O NSIS chama `CheckIfAppIsRunning` com `$INSTDIR\${MAINBINARYNAME}.exe` antes de copiar arquivos [V: `installer.nsi` do Tauri]; **como ele detecta/encerra não foi confirmado** (definição em macro não recuperada) [lacuna].
- Há relato de `externalBin` (sidecar) **não substituído** na reinstalação, com hipótese de arquivo em uso; o workaround relatado é hook de pré-instalação com `taskkill` [S: tauri#15134, issue aberta, sem confirmação de mantenedores]. Para este projeto isso é **o oposto do desejado**: o hook não pode matar o daemon.
- Desenho sugerido [I]: o instalador entrega `live-cmd-termd.exe` em `$INSTDIR`; ao iniciar, o app (ou o próprio instalador pós-instalação) **copia o daemon para uma pasta versionada** em `%LOCALAPPDATA%\live-cmd\daemon\<versão>\` e executa **dessa cópia**. Assim o instalador nunca tenta sobrescrever um executável em uso (Windows trava escrita em exe em execução [I, comportamento padrão]) e o nome distinto evita varredura por nome do exe principal. Trocar o daemon (nova versão) é decisão posterior: tickets sobre "atualização do próprio daemon" no mapa.
- Nenhum hook de `taskkill` pode citar o daemon.

## 8. Recomendação

**Alternativa A**: binário `live-cmd-termd` separado, lançado destacado (com fallback sem breakaway), rodando de cópia versionada fora de `$INSTDIR`, dono de ConPTY + `Service` + Canal P2P; app como cliente por named pipe com FIRST_PIPE_INSTANCE, rejeição de remotos, DACL só do usuário, verificação do PID do servidor e token no handshake; autostart pelo `HKCU\Run` já existente (também sobe o daemon). Tarefa agendada fica como opção futura de supervisão; serviço fica descartado.

## 9. Lacunas e validações pendentes (protótipo curto antes do spec)

1. Provar que filho `DETACHED_PROCESS|CREATE_BREAKAWAY_FROM_JOB` sobrevive ao `taskkill`/saída do app, e que o NSIS do Tauri não encerra o daemon (nome e pasta distintos). Testar também com o app lançado pelo autostart `Run` e pelo updater.
2. Como o `CheckIfAppIsRunning` do Tauri casa processos (código de `process.nsh`/utils não recuperado).
3. Comportamento de job do Agendador de Tarefas (só fonte secundária) e se criar trigger de logon pede elevação.
4. `GetNamedPipeServerProcessId`/`ClientProcessId` na prática com tokio.
5. Se `HPCON`/pipes poderiam ser duplicados entre processos (hoje assumido impossível/não suportado).

## Fontes

- Process Creation Flags (CREATE_BREAKAWAY_FROM_JOB, DETACHED_PROCESS): https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
- CreatePseudoConsole (1809+): https://learn.microsoft.com/en-us/windows/console/createpseudoconsole
- Interactive Services / sessão 0: https://learn.microsoft.com/en-us/windows/win32/services/interactive-services
- Service User Accounts: https://learn.microsoft.com/en-us/windows/win32/services/service-user-accounts
- WTSQueryUserToken (LocalSystem + SE_TCB_NAME): https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsqueryusertoken
- Named Pipe Security and Access Rights (DACL padrão): https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- CreateNamedPipe (FIRST_PIPE_INSTANCE, REJECT_REMOTE_CLIENTS): https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea
- Task Scheduler ExecutionTimeLimit (72 h, PT0S): https://learn.microsoft.com/en-us/windows/win32/taskschd/taskschedulerschema-executiontimelimit-settingstype-element
- tokio `ServerOptions`: https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html
- Raymond Chen, job objects "kill on close": https://devblogs.microsoft.com/oldnewthing/20131209-00/?p=2433
- Tauri updater (saída automática, installMode): https://v2.tauri.app/plugin/updater/
- Tauri Windows installer (modos, hooks): https://v2.tauri.app/distribute/windows-installer/
- Template NSIS do Tauri (`CheckIfAppIsRunning`): https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
- [S] Tauri #15134 (sidecar não substituído): https://github.com/tauri-apps/tauri/issues/15134
- [S] `uv-windows` spawn.rs (Agendador + job kill-on-close): https://docs.rs/crate/uv-windows/0.0.68/source/src/spawn.rs
- WezTerm multiplexing: https://wezterm.org/multiplexing.html
- Código: `desktop/src/conpty.rs`, `desktop/src/lib.rs`, `desktop/app/src/main.rs`, `desktop/app/src/settings.rs`

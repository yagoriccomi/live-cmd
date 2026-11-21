# Respostas 023 — separação daemon de terminais × app (premissas do modo loop)

Contexto: mapa `tickets/mapa-daemon-terminais.md`, tickets 012 e 013. Invocado em `/executar-projeto /loop pode fazer tudo e lança o release resolvendo todas pendências`; perguntas viraram premissas com a opção recomendada, para revisão do usuário.

## Premissas (recomendadas)

1. **Fronteira (012):** o daemon `live-cmd-termd` leva todo o `Service` (Sessões PTY, Canal P2P, sinalização, pareamento, Transferência Resumível, `sessions.json`) e o `Provider` de extras (SSH, Agentes, Histórico do host). O app desktop fica com UI, bandeja, configurações e o cliente para outros PCs.
2. **IPC (012):** named pipe local `\.\pipe\live-cmd-termd-<sid>` com DACL só do usuário, `PIPE_REJECT_REMOTE_CLIENTS`, token em arquivo com ACL do usuário; transporta as mesmas mensagens de controle (JSON) e frames do protocolo 007, mais RPC de serviço (status, pair, devices, revoke, restart). Handshake com `ipc_version`.
3. **Celular com app fechado (012):** continua conectado — quem atende é o daemon.
4. **Início (013):** o app sobe o daemon se não houver um respondendo no pipe; o autostart (`HKCU\Run`) continua abrindo o app, que sobe o daemon.
5. **Encerramento (013):** "Sair" da bandeja fecha só o app; item novo "Encerrar terminais" na bandeja para o daemon (com confirmação). Desinstalar encerra o daemon.
6. **Atualização do daemon (013):** binário copiado para `%LOCALAPPDATA%\live-cmd\daemon\<versao>\`; o app só sobe daemon novo quando `ipc_version` muda; o antigo segue até não ter sessões.
7. **Migração (013):** a primeira versão com daemon reinicia os terminais uma última vez (inevitável; retomada atual de pasta/saída continua valendo).

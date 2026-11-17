# PLANO — Telas Agentes, SSH e Histórico (Glass Aurora)

**Problema:** o app só tem terminal, pareamento e configurações; os mockups V1 Aurora mostram Agentes, SSH e Histórico.
**Resultado esperado:** três telas novas no desktop (Tauri, `desktop/app/ui`) com funcionalidade real no backend Rust.
**Como validar:** `cargo test` em `desktop/` e `desktop/app`; abrir cada tela no app.

**Premissas (modo Loop):**
- Mobile entra como fatia D, com mensagens de protocolo novas entre celular e host.
- Cada tela é um painel em `terminal.html`, aberto pela barra lateral, no tema Aurora já aplicado (`77b2319`).
- Agentes reaproveita as sessões de IA com `--resume` que já existem; SSH usa `ssh.exe` do Windows; Histórico grava os comandos localmente, sem enviar para fora.

| Fatia | Escopo | Nível | Estado |
|---|---|---|---|
| A | Agentes: lista de sessões de IA por grupo (pasta), retomar, nova sessão | alto | concluída |
| B | SSH: cadastro de servidores (host, usuário, porta, chave), conectar abre aba ssh.exe, status | alto | concluída |
| C | Histórico: gravar comandos por sessão, busca, filtros, reexecutar | alto | concluída |
| D | Mobile: abas Sessões/Agentes/SSH/Histórico + mensagens de protocolo e lado host | alto | concluída |
| E | Ligação celular↔host: `mobile::AppProvider` (app) registrado com `extras::set_provider` no setup; Agentes de `agents.json`, SSH de `ssh.json` (latência em cache, teste TCP em segundo plano) e Histórico de `history.jsonl`; `ssh_connect` abre o `ssh.exe` direto com destino validado | alto | concluída no código; falta teste ponta a ponta no celular |

**Pendências da fatia E:**
- Sessões de agente criadas pelo celular não entram em `agents.json`; depois de fechadas somem da lista (desktop e celular).
- Retomar pelo celular não atualiza `used_ms` no registro.
- `history_query` do celular só filtra por texto (sem dias, tipo ou só erros).

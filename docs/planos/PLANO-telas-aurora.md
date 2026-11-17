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

| F | Espelhamento desktop × celular: pergunta de capacidades (`session_attach` da sessão reservada `CAPS_PROBE`: host novo responde `caps_ok`, host 0.1.8 responde `NOT_FOUND` sem derrubar), registro único de agentes, filtros e limpar do histórico com `cwd`, SSH restaurado desconectado com Reconectar, cadastro SSH e nova sessão de agente pelo celular, teste do ConPTY sem OSC | alto | concluída no código; falta teste ponta a ponta no celular |

**Pendências da fatia E:** resolvidas na fatia F.

## Paridade desktop × celular

| Tela | Função | Desktop | Celular | Pedido / comando |
|---|---|---|---|---|
| Agentes | Listar sessões de IA por pasta (estado, agente, args, último uso) | sim | sim | `agent_list` |
| Agentes | Abrir sessão em execução / retomar gravada (`--resume`) | sim | sim | `agent_resume` |
| Agentes | Nova sessão (agente, pasta, comandos extras) | sim | sim | `term_create` com agente / `agent_create` |
| Agentes | Remover (encerra se aberta) com confirmação | sim | sim | `agent_remove` |
| Agentes | Registro em `agents.json` com `used_ms` | sim | sim | `Provider::agent_started` (ponto único no host) |
| Agentes | "Salvar como padrão" do agente | sim | não | só configuração local do desktop |
| Agentes | Escolher outro PC pareado | sim | não | o celular fala com um PC por vez |
| SSH | Listar servidores com busca | sim | sim | `ssh_list` |
| SSH | Status de alcance (testando, ms, host não encontrado, sem resposta, offline) | sim | sim | `ssh_list.status` / `ssh_probe` |
| SSH | Testar alcance agora | sim | sim | `ssh_probe` |
| SSH | Adicionar / editar servidor (nome, usuário, host, porta, chave; sem senha) | sim | sim | `ssh_save` (validado por `Ssh::check`) |
| SSH | Remover servidor com confirmação | sim | sim | `ssh_remove` |
| SSH | Conectar (abre `ssh.exe` no PC) | sim | sim | `ssh_connect` |
| SSH | Sessões SSH: abrir conectada, Reconectar desconectada | sim | sim | `ssh_list.sessions` / `ssh_reconnect` |
| SSH | Procurar arquivo da chave | sim | não | seletor de arquivo do PC; no celular digita o caminho |
| Histórico | Busca por texto | sim | sim | `history_query.query` |
| Histórico | Filtros últimos 7 dias, tipo de sessão, só erros | sim | sim | `history_query.days/kind/errors` |
| Histórico | Agrupar por dia; hora, PC, rótulo, tipo, pasta, código de saída | sim | sim | `history_query_ok` |
| Histórico | Reexecutar na sessão ativa | sim | sim | `term_input` / `history_run` |
| Histórico | Limpar com confirmação | sim | sim | `hist_clear` / `history_clear` (mesmo store) |
| Todas | Host antigo sem a tela | n/a | aviso "Atualize o live-cmd no PC" | `caps_ok` (pergunta `CAPS_PROBE`) |

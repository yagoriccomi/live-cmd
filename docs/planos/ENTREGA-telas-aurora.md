# ENTREGA — Telas Agentes, SSH e Histórico (Glass Aurora)

Plano: `docs/planos/PLANO-telas-aurora.md`. Branch: `claude/desktop-mobile-mockups-c96692`.

## O que entrou
- Tema Glass Aurora (mockup V1) como padrão no desktop e no mobile.
- Desktop: painéis Agentes de IA (lista por pasta, retomar com `--resume`, remover), Servidores SSH (cadastro em `ssh.json`, sem senha; latência TCP; conectar abre `ssh.exe` direto no ConPTY, argumentos validados) e Histórico (`history.jsonl` local, 5000 entradas, ignora prompts de senha, sem eco, tela alternativa e sessões de IA; busca, filtros, reexecutar, limpar).
- Mobile: barra de abas flutuante Sessões/Agentes/SSH/Histórico; 6 pedidos novos no protocolo (`api: 1`), ligados aos dados reais pelo `AppProvider` (`desktop/app/src/mobile.rs`).
- Cherry-pick de `c8cc053` (retomar agente sem conversa gravada).
- Fatia F (espelhamento): pergunta de capacidades (`session_attach` da sessão reservada `CAPS_PROBE`: host novo responde `caps_ok`, host 0.1.8 responde `NOT_FOUND` sem derrubar); 7 pedidos novos (`agent_create`, `agent_remove`, `ssh_save`, `ssh_remove`, `ssh_probe`, `ssh_reconnect`, `history_clear`) e filtros em `history_query`, todos `api: 1`, validados no host e sem campo de senha; registro de agentes num ponto só (`Provider::agent_started`); histórico com store único e `cwd`; SSH restaurado volta desconectado com Reconectar nas duas telas; teste do ConPTY ignora OSC.

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

## Verificação
- `cargo build` e `cargo test` em `desktop/app`: 37/37, sem avisos.
- `cargo test --lib` em `desktop/`: 140 passaram, 0 falharam, 11 ignorados (marcados `#[ignore]` na base). Nenhum teste de rede instável falhou nesta rodada.
- `npm test` (24/24) e `npx tsc --noEmit` em `mobile/`.

## Compatibilidade (cliente × host)
O `auth_ok` não mudou: os validadores da versão 0.1.8 (host e clientes) recusam campo desconhecido e fecham a conexão, e o host 0.1.8 também fecha com tipo de pedido desconhecido. Por isso as capacidades vão numa pergunta que o 0.1.8 já entende.

| Cliente | Host | Resultado |
|---|---|---|
| novo | novo | `caps_ok` com `agents`, `ssh`, `history`; abas completas |
| novo | 0.1.8 | `error NOT_FOUND` com `request_id` (não fatal); abas mostram "Atualize o live-cmd no PC" |
| 0.1.8 | novo | `auth_ok` idêntico ao antigo; o cliente antigo nunca manda a pergunta nem pedidos extras |
| 0.1.8 | 0.1.8 | sem mudança |

O cliente desktop só manda pedidos extras ao host do próprio PC (mesma versão); para outros PCs usa só `session_*`.

## Não verificado
- Nenhuma tela foi aberta no app nem no celular; SSH real, Reconectar real, retomar e criar agente real e reexecutar pelo celular sem teste manual.

## Pendências
- Comandos digitados pelo celular não entram no histórico (só os do terminal desktop).
- "Salvar como padrão" do agente e o seletor de arquivo da chave SSH existem só no desktop.

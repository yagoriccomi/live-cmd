# PLANO — testes instáveis do host (desktop)

Origem: `answers/2026_11_19 gerenciador-terminais-remoto 022.md`, decisão 4 (corrigir agora).

## Objetivo

Cada teste abaixo passa 30 vezes seguidas, isolado (`cargo test --lib -p live-cmd-echo <nome> -- --exact`), e a bateria `cargo test --lib -p live-cmd-echo` passa uma vez no fim.

Não fazer: mudar comportamento do produto só para o teste passar, aumentar timeout às cegas, marcar `#[ignore]`. Corrigir a causa (corrida, espera por tempo fixo, porta/estado global compartilhado, ordem de eventos). Se a causa for bug real do host, corrigir o host e dizer isso no relatório.

## Fatias

| Fatia | Testes | Arquivo | Nível | Estado |
|---|---|---|---|---|
| A | `loopback_backpressure`, `s2_two_clients_simultaneous`, `d4c_a7_typing_during_upload`, `d4c_n12_revoke_during_upload` | `desktop/src/lib.rs` | medio | pendente |
| B | `n25_slow_network_expires` | `desktop/src/files.rs` | medio | pendente |
| C | `remote_upload_file` | `desktop/src/client.rs` | medio | pendente |

## Regras para os agentes

- Worktree próprio; `CARGO_TARGET_DIR` próprio (o `target/` do repositório pode estar travado por antivírus/processo).
- Reproduzir antes de corrigir: rodar o teste em laço (até 100x ou com carga paralela) e registrar a taxa de falha e a mensagem.
- Commit atômico em Conventional Commits (`test(host): ...` ou `fix(host): ...`), terminando com `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Relatório final: causa, correção, taxa antes/depois, comandos rodados.

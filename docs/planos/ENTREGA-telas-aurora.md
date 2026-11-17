# ENTREGA — Telas Agentes, SSH e Histórico (Glass Aurora)

Plano: `docs/planos/PLANO-telas-aurora.md`. Branch: `claude/desktop-mobile-mockups-c96692`.

## O que entrou
- Tema Glass Aurora (mockup V1) como padrão no desktop e no mobile.
- Desktop: painéis Agentes de IA (lista por pasta, retomar com `--resume`, remover), Servidores SSH (cadastro em `ssh.json`, sem senha; latência TCP; conectar abre `ssh.exe` direto no ConPTY, argumentos validados) e Histórico (`history.jsonl` local, 5000 entradas, ignora prompts de senha, sem eco, tela alternativa e sessões de IA; busca, filtros, reexecutar, limpar).
- Mobile: barra de abas flutuante Sessões/Agentes/SSH/Histórico; 6 pedidos novos no protocolo (`api: 1`), ligados aos dados reais pelo `AppProvider` (`desktop/app/src/mobile.rs`).
- Cherry-pick de `c8cc053` (retomar agente sem conversa gravada).

## Verificação
- `cargo build` e `cargo test` em `desktop/app`: 33/33.
- `cargo test --lib` em `desktop/`: falhas só em testes de rede instáveis já existentes na base (passam isolados) e em `conpty::tests::hidden_conpty_io_resize_ctrl_c_exit`, que falha só quando o repositório está num caminho longo (o título da janela entra antes do `\n`); o mesmo código passa num caminho curto e na `main`.
- `npm test` (20/20) e `npx tsc --noEmit` em `mobile/`.

## Não verificado
- Nenhuma tela foi aberta no app nem no celular; SSH real, retomar agente real e reexecutar pelo celular sem teste manual.

## Pendências
- Celular novo contra host antigo: abrir aba nova derruba a conexão (falta capacidades no handshake).
- Sessões de agente criadas pelo celular não entram em `agents.json`.
- `history_query` do celular só filtra por texto; `cwd` sempre nulo.
- Sessões SSH são reabertas ao reiniciar o app e podem pedir senha de novo.
- Teste do ConPTY sensível ao caminho do repositório.

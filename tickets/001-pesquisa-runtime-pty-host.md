# Ticket 001: Pesquisa de Runtime e Biblioteca PTY para o Host

**Labels:** `wayfinder:research`, `host`, `pty`  
**Status:** Fechado (Resolvido)  
**Parent:** [Mapa](mapa.md)

## Pergunta

Qual é a melhor escolha de runtime e biblioteca PTY para o Host daemon (Node.js/TypeScript com `node-pty` vs Rust com `portable-pty` vs Go com `creack/pty`), considerando facilidade de empacotamento cross-platform (Windows PowerShell/CMD, Linux bash/zsh, macOS zsh), multiplexação de processos de terminal persistentes e baixo consumo de recursos?

## Resolução

**Veredito:** **Rust com `portable-pty`** (ecossistema WezTerm).
- Relatório técnico detalhado: [pesquisas/001-runtime-pty-host.md](../pesquisas/001-runtime-pty-host.md).
- Suporte nativo ao Windows ConPTY API (`CreatePseudoConsole`) e POSIX (`openpty`/`forkpty`) no Linux/macOS sob a trait unificada `PtySystem`.
- Compilação direta em executável único estático (~3 a 8 MB) sem runtime no host.
- Pegada de memória de apenas 2 a 6 MB RSS em repouso com I/O assíncrono via `tokio`.

# Pesquisa Técnica: Runtime e Biblioteca PTY para o Host (Ticket 001)

**Status:** Concluído (Veredito: Rust + portable-pty)

## 1. Resumo Executivo e Recomendação
Para o Host daemon, a combinação recomendada é **Rust com portable-pty** (ecossistema WezTerm).
- **Empacotamento Standalone Real:** Compila nativamente para executável único estático sem runtime (~3 a 8 MB).
- **Suporte ConPTY Nativo no Windows:** Utiliza a API moderna `CreatePseudoConsole` no Windows 10/11 e POSIX (`openpty`/`forkpty`) no Linux/macOS sob a trait `PtySystem`.
- **Consumo Mínimo de Recursos:** 2 a 6 MB de RSS em repouso, I/O assíncrono com tokio.

## 2. Comparativo
- **Rust + portable-pty:** ConPTY nativo, binário único direto, 2-6 MB RSS, latência em microssegundos.
- **Go + creack/pty:** Não suporta Windows (retorna ErrUnsupported, PR #155 sem merge).
- **Node.js/TS + node-pty:** Exige C++ addon; Node SEA não carrega addons nativos da memória sem extrair em disco.

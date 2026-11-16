# PLANO — terminal-ux (2026-11-16)

**Origem:** feedback v0.1.2 + 20 sugestões aprovadas ("/loop pode executar os 20 com agentes simultâneos médios").
**Decisões:** `answers/2026_11_16 gerenciador-terminais-remoto 019.md`. Menu de contexto do terminal expõe todas as ações.

## Premissas Assumidas (Loop)
- Nível `medio` (pedido do usuário). 4 agentes, worktrees isoladas; coordenador integra em `main`.
- Split (5): duas sessões lado a lado, uma divisão por aba; sem árvore de divisões.
- Notificação (9): só quando a janela não está em foco e o comando levou > 10 s.
- Escopo negativo: sem TURN, sem Android, sem mudança de protocolo além de `session_rename`.

## Fatias
| Fatia | Itens | Arquivos principais | Branch | Estado |
|---|---|---|---|---|
| F1 core | lixo OSC, 9, 11, 12, 18, rename sincronizado (host) | `app/src/term.rs`, `src/session.rs`, `src/lib.rs` | worktree | rodando |
| F2 entrada | copiar/colar WT, menu de contexto completo, 1, 2, 3, 10, 19 | `app/ui/terminal.js` (teclado/tela), css | worktree | rodando |
| F3 abas/lateral | renomear (UI), 5, 6, 7, 8, 13, 14, 20 | `app/ui/terminal.js` (abas/lateral), html | worktree | rodando |
| F4 perfis | 4, 15, 16, 17 | `app/ui/*`, `settings.rs` | worktree | rodando |

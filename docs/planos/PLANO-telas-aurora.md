# PLANO — Telas Agentes, SSH e Histórico (Glass Aurora)

**Problema:** o app só tem terminal, pareamento e configurações; os mockups V1 Aurora mostram Agentes, SSH e Histórico.
**Resultado esperado:** três telas novas no desktop (Tauri, `desktop/app/ui`) com funcionalidade real no backend Rust.
**Como validar:** `cargo test` em `desktop/` e `desktop/app`; abrir cada tela no app.

**Premissas (modo Loop):**
- Telas só no desktop nesta rodada; o mobile fica para depois.
- Cada tela é um painel em `terminal.html`, aberto pela barra lateral, no tema Aurora já aplicado (`77b2319`).
- Agentes reaproveita as sessões de IA com `--resume` que já existem; SSH usa `ssh.exe` do Windows; Histórico grava os comandos localmente, sem enviar para fora.

| Fatia | Escopo | Nível | Estado |
|---|---|---|---|
| A | Agentes: lista de sessões de IA por grupo (pasta), retomar, nova sessão | alto | em andamento |
| B | SSH: cadastro de servidores (host, usuário, porta, chave), conectar abre aba ssh.exe, status | alto | em andamento |
| C | Histórico: gravar comandos por sessão, busca, filtros, reexecutar | alto | em andamento |

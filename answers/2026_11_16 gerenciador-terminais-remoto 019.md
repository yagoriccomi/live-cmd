# Decisões — gerenciador-terminais-remoto — 2026-11-16

**Contexto:** feedback do Terminal Desktop v0.1.2 após pareamento PC-PC funcionar: lixo no prompt, copiar/colar, renomear.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | "Nome de usuário ruim" | Lixo `e]7;file://` / `e]133;D` no prompt | Bug: sequências OSC 7/133 do PowerShell aparecem como texto; corrigir |
| 2 | Importante | Copiar/colar | Igual Windows Terminal | Ctrl+C copia com seleção, senão interrompe; Ctrl+V cola; botão direito cola; Ctrl+Shift+C/V mantidos |
| 3 | Importante | Renomear abas e PCs | Duplo clique e menu | Edição no lugar + item "Renomear" no menu de contexto, para aba, sessão e PC |
| 4 | Detalhe | Nome da sessão sincronizado | Sim, fica na sessão | Nome guardado no host; todos os PCs veem o mesmo |

**Confirmações:** nenhuma
**Premissas:** nenhuma
**Ainda em aberto:** nenhuma

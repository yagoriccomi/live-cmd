# Decisões — gerenciador-terminais-remoto — 2026-11-17

**Contexto:** melhorias não estéticas após v0.1.14 (histórico salvo, prints colados, APK).
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Saída do terminal em `sessions.json` (128 KiB texto puro) | Cifrar | Histórico continua restaurado, ilegível em disco |
| 2 | Importante | Onde salvar print colado em aba local | Subpasta `.live-cmd` da pasta atual | Precisa estar no .gitignore dos projetos |
| 3 | Detalhe | Limpeza de prints enviados ao outro PC | Nunca apagar | Arquivos acumulam no outro PC |
| 4 | Importante | Instalação do APK | Anexar na release do GitHub | APK fica público |

**Confirmações:** nenhuma
**Premissas:** nenhuma
**Ainda em aberto:** nenhuma

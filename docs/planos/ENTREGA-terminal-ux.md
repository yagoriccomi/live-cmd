# ENTREGA — terminal UX v0.1.4

Plano: `PLANO-terminal-ux-v014.md`.

## Entregue
- Ditado Win+H nos terminais (`contenteditable` + `beforeinput`).
- Botão direito cola (copia se houver seleção); Shift+botão direito, ☰ ou Shift+F10 abre o menu.
- Zoom de fonte salvo por PC.
- cmd: OSC 133/7 via `PROMPT`, com indicador de atividade, aviso de fim de comando, cwd e confirmação ao fechar.
- Histórico de rolagem configurável na tela.
- Exportar para `.txt` pelo "Salvar como", com linhas quebradas unidas.
- Qualidade da conexão na lateral: latência e direta/relay (`term_conn_stats`).
- Perfis valem sem reiniciar.
- Atalhos remapeáveis no F1 (salvos em localStorage `term-keys`, com proteção contra dado corrompido).
- "Limpar" funciona no cmd (`term_clear`).

## Não entregue
- D4 (nome da sessão salvo em disco no host): fora por decisão. A sessão ConPTY morre com o host; o nome fica em memória e sincronizado entre os PCs.
- Item 10 (arrastar arquivo para sessão remota envia ao outro PC): parcial, cola o caminho. O cliente desktop não tem `file-channel`. Fica para a v0.1.5.

## Verificação
- `node --check` em `terminal.js`, `extras.js` e `app.js`: ok.
- `cargo test` em `desktop/app`: 16 passaram.
- `cargo test` em `desktop`: 120 passaram, 11 ignorados.
- Não verificado: teste manual da interface com o app aberto (ditado, menus, exportar).

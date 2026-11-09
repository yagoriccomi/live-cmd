# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** perguntas do rascunho `docs/planos/PLANO-M3.md` (seção 6).
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Crítico | Tamanho com vários aparelhos | "Tamanho independente. O PC captura o texto e envia igual a todos; cada app monta a tela sozinho; sincronização se mantém" (resposta livre) | Bytes idênticos para todos; cada cliente renderiza no próprio tamanho. Em aberto: tamanho do PTY no PC e efeito em apps de tela cheia (vim/htop) |
| 2 | Importante | `cmd` sem aviso de fim de comando | Sem suporte no M3 | Aviso de fim e pasta atual só em pwsh/WSL |
| 3 | Importante | Biblioteca de PTY | ConPTY direto | Chamadas à API do Windows via crate `windows`; só Windows; mais código no D3c |
| 4 | Crítico | Terminal no celular | Terminal nativo | Sem xterm.js/WebView; emulador e renderização próprios em React Native; M3b bem maior |

| 5 | Importante | Tamanho do PTY no PC | "Editável pelo user; deve se adaptar à mudança de tamanho de janelas e à divisão do monitor em várias janelas, como o 9remote faz" (resposta livre) | PTY redimensionável: resize vindo da janela/divisão de tela do cliente e ajuste manual; referência "9remote" não verificada |
| 6 | Importante | Recuperar tela após perda | `vt100` no PC | Host mantém cópia da tela e envia `screen_snapshot` |
| 7 | Normal | Perfis de terminal | Arquivo de configuração | Perfis lidos de arquivo no host, validados na carga |

**Confirmações:** nenhuma.
**Premissas:** "independente" vale para a exibição; o PTY tem um tamanho por vez, ajustável pelo usuário e pelo redimensionamento das janelas.
**Ainda em aberto:** como o 9remote se comporta (referência a confirmar); com vários aparelhos ativos, qual resize vence; aprovação do `PLANO-M3.md`.

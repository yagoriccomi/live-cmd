# Mapa de Caminho: Gerenciador de Terminais Remoto

**Labels:** `wayfinder:map`  
**Tracker:** markdown local (`tickets/`)  
**Status do Mapeamento:** Concluído (Destino Alcançado)

## Destino

Especificação técnica e arquitetural completa do sistema de terminais remotos (Host/Cliente Desktop híbrido em Rust com Tauri, Cliente Mobile em React Native com xterm.js Canvas, protocolo P2P WebRTC com dois canais dedicados, pareamento instantâneo via QR Code com chaves permanentes e transferência resumível de arquivos), validada e decomposta em fatias executáveis de implementação.

## Notas

- **Domínio:** Seguir rigorosamente o glossário em `CONTEXT.md` (App Desktop Unificado, Cliente Mobile, Sessão PTY, Servidor de Sinalização, Canal P2P, Transferência Resumível, OTK, Dispositivo Emparelhado, Reconexão Transparente, Notificação de Término).
- **Referência:** Inspirado e aprimorado a partir do [9remote](https://github.com/decolua/9remote).
- **Decisões base:** Registradas em `answers/2026_11_05 gerenciador-terminais-remoto 001.md`, `answers/2026_11_05 gerenciador-terminais-remoto 002.md` e `answers/2026_11_06 gerenciador-terminais-remoto 003.md`.
- **Relatórios de Pesquisa:** Salvos em `pesquisas/`.
- **Especificações Técnicas:** Consolidadas nos tickets 006, 007, 008 e 009.

## Decisões até aqui

- [Escopo do MVP e Topologia de Conexão](../answers/2026_11_05%20gerenciador-terminais-remoto%20001.md): Terminal PTY + Abas + Pareamento QR + Envio de arquivos; arquitetura WebRTC DataChannel direta; Cliente em React Native/Expo.
- [Ticket 001 — Runtime e Biblioteca PTY do Host](001-pesquisa-runtime-pty-host.md): Adoção de **Rust com `portable-pty`** para ConPTY nativo no Windows, POSIX unificado, binário único (~5 MB) e 2-6 MB RSS.
- [Ticket 002 — Sinalização e NAT Traversal](002-pesquisa-sinalizacao-webrtc-nat.md): Adoção de **Cloudflare Workers + Durable Objects (WebSocket Hibernation)** a custo zero e **Cloudflare Realtime TURN** (1 TB/mês grátis com fallback TLS 443) para furar CGNAT 4G/5G.
- [Ticket 005 — Emulador de Terminal e Teclado Mobile](005-emulador-terminal-react-native.md): Adoção de **xterm.js via `react-native-webview` com Canvas Addon** e barra de atalhos com `react-native-keyboard-controller` (estados latched/locked para Ctrl/Alt e fix de IME).
- [Ticket 003 — Protocolo e Transferência de Arquivos](003-protocolo-mensagens-datachannel.md): DataChannels isolados (`pty-channel` e `file-channel`); envio de arquivos em chunks indexados de 64 KB com retomada automática (estilo torrent) e expurgo automático de arquivos incompletos após 24 horas.
- [Ticket 004 — Segurança e Pareamento por QR Code](004-modelo-seguranca-pareamento-qr.md): Pareamento inicial instantâneo via QR Code efêmero com troca de chaves Ed25519 permanentes; conexão remota persistente com autorização contínua; bloqueio biométrico (Face ID / digital) configurável no app mobile.
- [Operações e Comportamento Operacional](../answers/2026_11_06%20gerenciador-terminais-remoto%20003.md): Arquivos salvos na pasta atual do terminal (`cwd`) com seletor livre; reconexão silenciosa com replay de scrollback; encerramento de terminais inativos após 24 horas; app desktop híbrido (Host + Cliente) na bandeja (Tray) com autostart silencioso; e notificações push no celular ao fim de comandos.

## Especificações Técnicas Finalizadas

- [Especificação da Arquitetura do Host Daemon em Rust](006-spec-arquitetura-host-rust.md) (`wayfinder:task` — Concluído)
- [Especificação do Protocolo de Mensagens e Transferência Resumível](007-spec-protocolo-mensagens.md) (`wayfinder:task` — Concluído)
- [Especificação do Cliente Mobile em React Native e Interface de Terminal](008-spec-app-mobile-react-native.md) (`wayfinder:task` — Concluído)
- [Plano de Implementação em Fatias Verticais (Tickets Bala Traçante)](009-plano-implementacao-fatias.md) (`wayfinder:task` — Concluído)

## Ainda não especificado

*(Nenhuma névoa restante: o caminho até a construção do software está completamente mapeado e especificado)*

## Fora de escopo

- Transmissão de área de trabalho / tela e controle de mouse remoto (foco estrito em terminal e arquivos).
- Autenticação por conta centralizada em nuvem (usuário, senha, banco de dados de clientes).
- Editor de código completo no mobile com árvores de arquivos extensas e syntax highlight pesado.

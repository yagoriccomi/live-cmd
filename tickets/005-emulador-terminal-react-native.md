# Ticket 005: Arquitetura do Emulador de Terminal e Teclado Estendido no React Native

**Labels:** `wayfinder:research`, `mobile`, `react-native`  
**Status:** Fechado (Resolvido)  
**Parent:** [Mapa](mapa.md)

## Pergunta

Qual é a melhor estratégia técnica para renderizar o emulador de terminal dentro do React Native (xterm.js via WebView com ponte IPC otimizada vs biblioteca nativa de terminal para Android/iOS) e como implementar uma barra de teclado virtual ergonômica com teclas de controle modificadoras (`Ctrl`, `Alt`, `Esc`, `Tab`, setas direcionais) compatível com teclados virtuais do Android e iOS?

## Resolução

**Veredito:** **xterm.js 5.x/6.0 via `react-native-webview`** com `@xterm/addon-canvas` e `@xterm/addon-fit`.
- Relatório técnico detalhado: [pesquisas/005-emulador-terminal-react-native.md](../pesquisas/005-emulador-terminal-react-native.md).
- **Renderização:** Canvas Addon evita crash por perda de contexto WebGL no mobile, mantendo 60 FPS com 15–30 MB RAM.
- **Teclado:** `react-native-keyboard-controller` com `KeyboardStickyView` para sincronia suave a 60/120 FPS.
- **Modificadores:** Máquina de estados *Latched* (1 toque) e *Locked* (2 toques) para `Ctrl` (`char & 0x1F`) e `Alt` (prefixo `\x1b`).
- **IME Fix:** Desativação de autocorreção e captura de evento `beforeinput` para evitar retenção de comandos no Android (GBoard).

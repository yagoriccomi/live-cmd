# Especificação Técnica: Cliente Mobile em React Native (Ticket 008)

**Data:** 2026-11-06  
**Status:** Especificação Aprovada  
**Stack:** React Native 0.76+ / Expo SDK 52+ / TypeScript / `react-native-webview` / `react-native-webrtc`

---

## 1. Arquitetura de Telas e Navegação

O aplicativo adota navegação estruturada via `expo-router`:

```
app/
├── (auth)/
│   └── biometric-lock.tsx     # Desbloqueio biométrico opcional (Face ID / Touch ID)
├── (tabs)/
│   ├── index.tsx              # Lista de sessões de terminal e hosts pareados
│   ├── terminal/
│   │   └── [sessionId].tsx    # Tela principal do emulador de terminal
│   ├── files.tsx              # Explorador de arquivos e fila de transferências
│   └── settings.tsx           # Configurações (biometria, chaves, lista de hosts)
└── pair-modal.tsx             # Câmera para leitura do QR Code do PC
```

---

## 2. Emulador de Terminal Móvel (`xterm.js` via Canvas)

### 2.1. Container WebView
- Renderizado via `react-native-webview` com HTML local empacotado nos assets.
- **Addons ativos:**
  - `@xterm/addon-canvas`: Renderização 2D Canvas de alto desempenho e baixo consumo de bateria (evitando falhas de WebGL).
  - `@xterm/addon-fit`: Redimensionamento automático da matriz de caracteres de acordo com a orientação da tela e dimensões do teclado.
  - `@xterm/addon-web-links`: Detecção de links clicáveis na saída do terminal.

### 2.2. Barra Auxiliar de Teclado (`KeyboardAccessoryBar`)
- Posicionada dinamicamente acima do teclado virtual usando `react-native-keyboard-controller` com `KeyboardStickyView`.
- **Botões Disponíveis:**
  - `Esc`, `Tab`, `Ctrl`, `Alt`, setas direcionais (`▲`, `▼`, `◀`, `▶`), `|`, `/`, `-`, `~`.
- **Comportamento das Teclas Modificadoras (`Ctrl` e `Alt`):**
  - **Latched (1 toque):** Fica com fundo destacado; a próxima tecla pressionada no teclado virtual aplica o modificador e o estado desativa sozinho.
  - **Locked (2 toques):** Trava o modificador até ser tocado novamente.
- **Prevenção de Perda de Foco:** Todos os botões usam `onPressIn` com `preventDefault` para evitar que o teclado virtual do celular feche ao tocar nos atalhos.

---

## 3. Gerenciamento de Arquivos e Fila de Transferência

- **Navegador Integrado:** Permite visualizar o diretório atual do terminal (`cwd`) ou navegar pelas pastas do computador remoto.
- **Upload:** Seleção de arquivos locais do celular via `expo-document-picker` ou fotos via `expo-image-picker`.
- **Barra de Progresso Flutuante:** Exibe velocidade de transmissão, percentual concluído e tempo estimado para chunks em andamento no `file-channel`.
- **Retomada Automática:** Se o sinal de celular for perdido no meio de um upload grande, o app pausa a transmissão e retoma do mesmo bloco assim que a conexão se restabelecer.

---

## 4. Segurança, Biometria & Notificações Locais

- **Autenticação Biométrica:** Integrada com `expo-local-authentication`. Se habilitada em configurações, exige Face ID / impressão digital ao abrir o app ou ao alternar de segundo plano para primeiro plano.
- **Notificações Push Locais:** Integradas com `expo-notifications`.
  - Quando um comando é executado e o usuário minimiza o app, o Host envia o sinal `0x05` ao terminar o processo.
  - O app dispara uma notificação local: *"Comando concluído: retorno ao prompt"*.

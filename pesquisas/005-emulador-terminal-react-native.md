# Pesquisa Técnica — Ticket 005: Arquitetura do Emulador de Terminal e Teclado Estendido no React Native

**Data:** 05 de Novembro de 2026  
**Status:** Concluído  
**Autor:** Engenheiro de Pesquisa e Arquitetura Mobile  
**Projeto:** Gerenciador de Terminais Remoto  
**Ticket:** [Ticket 005](../../tickets/005-emulador-terminal-react-native.md)  
**Parent:** [Mapa de Caminho](../../tickets/mapa.md)  
**Branch:** `research/005-emulador-terminal-react-native`

---

## 1. Resumo Executivo & Decisão Recomendada

Esta pesquisa investiga a viabilidade técnica e a arquitetura ideal para atender aos dois pilares centrais do **Ticket 005**:
1. **Renderização do Terminal no Mobile:** Escolha entre **xterm.js embarcado via `react-native-webview`** (com ponte IPC otimizada) versus **bibliotecas nativas de terminal** (SwiftTerm no iOS e Termux `TerminalView` no Android).
2. **Barra de Teclado Virtual Ergonômica (Accessory Bar):** Implementação de uma barra sobre o teclado virtual do Android/iOS que ofereça teclas de controle modificadoras (`Ctrl`, `Alt`, `Esc`, `Tab`, setas direcionais), com máquina de estados "sticky/latched" e prevenção contra perda de foco (*blur*).

### Decisão Técnica Recomendada

| Componente | Recomendação | Justificativa Sintética |
|---|---|---|
| **Renderizador do Terminal** | **xterm.js 5.x/6.0 via `react-native-webview`** com `@xterm/addon-canvas` e `@xterm/addon-fit` | Paridade 100% multiplataforma (Android, iOS e Web); ecossistema maduro com total fidelidade VT100/ANSI/Truecolor; evita a manutenção de duas bases de código nativas divergentes (Swift vs Kotlin). O renderizador Canvas entrega 60 FPS estáveis com baixo consumo de memória no mobile. |
| **Ponte de Comunicação (IPC)** | **PostMessage bidirecional com batching em `requestAnimationFrame`** | Elimina saturação da bridge JS do React Native durante rajadas de output (`cat`, logs em streaming), acumulando chunks binários/textuais antes do commit no DOM/Canvas do xterm. |
| **Barra de Teclado (Accessory Bar)** | **Componente React Native nativo com `react-native-keyboard-controller`** (`KeyboardStickyView`) | Sincronização fluida com a animação de abertura/fechamento do teclado virtual (60/120 FPS em iOS e Android), suporte nativo a *Edge-to-Edge* no Android 15+ e prevenção de recuo do teclado via `onPressIn` com `preventDefault`. |
| **Tratamento de Modificadores (`Ctrl`/`Alt`)** | **Máquina de estados Latch / Lock** | 1 toque ativa modo *latched* (modifica o próximo caractere e desativa); 2 toques ativam modo *locked*. O caractere é transformado na sequência de controle ASCII apropriada (ex: `Ctrl+C` $\to$ `\x03`) antes do envio pelo DataChannel. |
| **Correção de IME / Android GBoard** | **Configuração estrita do textarea do xterm + interceptação `beforeinput`** | Resolve o bug crítico do xterm.js no Android onde o teclado retém caracteres aguardando Enter duplo (issue xtermjs/xterm.js #5108), aplicando `autocorrect="off"`, `inputmode="text"` e ouvindo deltas de entrada diretamente. |

---

## 2. Análise Comparativa Profunda: WebView + xterm.js vs Terminais Nativos

### 2.1. Abordagem A: xterm.js via `react-native-webview`

O [xterm.js](https://github.com/xtermjs/xterm.js) é a biblioteca padrão da indústria para emulação de terminal no navegador (utilizada pelo VS Code, Hyper, plataformas Cloud e clientes como o 9remote).

```
+-------------------------------------------------------------------+
| React Native Mobile App (JavaScript / Hermes Thread)              |
|   - WebRTC DataChannel (P2P Stream com Host)                      |
|   - Virtual Keyboard Controller & Modifier State Machine          |
|   - Custom Accessory Bar (Ctrl, Alt, Esc, Tab, Setas)             |
+---------------------------------+---------------------------------+
                                  |
               postMessage IPC    | (Chunks de I/O em Batch)
                                  v
+---------------------------------+---------------------------------+
| react-native-webview (WKWebView no iOS / Chromium WebView Android)|
|   +-------------------------------------------------------------+ |
|   | xterm.js Core Terminal Instance                             | |
|   |   - @xterm/addon-canvas (Renderização direta em HTML5 Canvas)| |
|   |   - @xterm/addon-fit (Cálculo dinâmico de cols e rows)       | |
|   |   - @xterm/addon-unicode11 (Suporte a emojis e CJK)         | |
|   |   - Interceptador de IME para Android GBoard                | |
|   +-------------------------------------------------------------+ |
+-------------------------------------------------------------------+
```

#### Modos de Renderização no Mobile WebView:
1. **DOM Renderer (Padrão):** Cria elementos `<span>` e `<div>` para cada caractere/linha.
   * *Avaliação no Mobile:* Inadequado para fluxos intensos de dados; causa gargalo severo de garbage collection e quedas de taxa de quadros (*jank*).
2. **WebGL Addon (`@xterm/addon-webgl`):** Aceleração total via GPU.
   * *Avaliação no Mobile:* Embora seja o mais rápido em desktops, no contexto de WebViews móveis apresenta um risco crítico: **perda de contexto WebGL (`webglcontextlost`)** quando o aplicativo entra em background ou sofre pressão de memória do sistema operacional (Android LMK / iOS Jetsam). Ao retornar ao primeiro plano, a tela frequentemente fica em branco ou requer recarga forçada.
3. **Canvas Addon (`@xterm/addon-canvas`):** Renderiza o grid de células em um elemento `<canvas>` 2D.
   * *Avaliação no Mobile (Recomendado):* Proporciona performance consistente a 60 FPS, consumo de memória previsível (~15 a 30 MB) e não sofre com perda de contexto quando o app é minimizado.

#### Prós:
- **100% Multiplataforma:** O mesmo código de emulação roda idêntico no Android, no iOS e em uma eventual versão Web Desktop.
- **Fidelidade Terminal Completa:** Suporte integral a sequências de escape ANSI/VT100/VT220, Truecolor (24-bit RGB), SGR, modos alternados de tela (*alternate screen buffer* para `vim`, `htop`, `tmux`), mouse tracking e OSC 52 (copiar para clipboard).
- **Ecossistema:** Addons mantidos pela equipe do VS Code (`@xterm/addon-fit`, `@xterm/addon-unicode11`, `@xterm/addon-search`, `@xterm/addon-serialize`).

#### Desvantagens e Mitigações:
- **Sobrecarga de IPC:** Se cada byte do terminal for enviado via chamadas individuais de `postMessage`, a ponte JS pode engasgar.  
  *Mitigação:* Acúmulo de dados (*buffering*) e envio em lotes (*batching*) com `requestAnimationFrame` ou timer de 8ms a 16ms.
- **Tamanho do Buffer:** Rolar 100.000 linhas no WebView consome memória excessiva.  
  *Mitigação:* Limitar `scrollback` a 2.000 ou 5.000 linhas no cliente móvel.

---

### 2.2. Abordagem B: Bibliotecas Nativas de Terminal

A alternativa nativa consiste em utilizar componentes de terminal desenvolvidos diretamente nas linguagens de cada sistema operacional:
- **iOS:** [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) (desenvolvido por Miguel de Icaza, utilizado no Blink Shell, Prompt 3 e cmux-mobile). Renderização nativa Metal/CoreText.
- **Android:** Componentes do [Termux](https://github.com/termux/termux-app) (`termux-view` / `terminal-emulator`). Renderização via Android Native Canvas/Views.

#### Prós:
- **Desempenho Extremo:** Inicialização em milissegundos sem sobrecarga de motor web.
- **Menor Consumo de RAM:** ~5 a 10 MB de memória por instância de terminal.
- **Interação Nativa com o Teclado do Sistema:** Melhor suporte nativo a seleção de texto e menus de contexto do SO.

#### Contras (Fatores Eliminatórios para o Projeto):
- **Inexistência de Módulo Unificado para React Native:** Não há nenhuma biblioteca comunitária mantida e estável que encapsule tanto o SwiftTerm (iOS) quanto o `termux-view` (Android) com API única para React Native / Expo.
- **Duplicação de Manutenção:** O projeto precisaria manter dois módulos nativos locais (um em Swift com Expo Module API e outro em Kotlin/Java com JNI).
- **Disparidade de Recursos:** Pequenas variações na interpretação de sequências ANSI, renderização de fontes monoespaçadas (Nerd Fonts, Powerline) e tratamento de redimensionamento exigiriam correções de bugs em duplicidade.
- **Incompatibilidade com Expo Go:** Exige obrigatoriamente fluxos de *Prebuild* / *Custom Development Client*, dificultando testes rápidos de UI.

---

### 2.3. Matriz de Decisão Comparativa

| Critério | xterm.js via WebView (Canvas) | SwiftTerm (iOS) + TermuxView (Android) |
|---|:---:|:---:|
| **Suporte Multiplataforma (iOS + Android)** | ⭐⭐⭐⭐⭐ (Código único) | ⭐⭐ (Dois módulos independentes) |
| **Compatibilidade com Expo / React Native** | ⭐⭐⭐⭐⭐ (Suportado via `react-native-webview`) | ⭐⭐ (Requer Expo Local Modules em Swift e Kotlin) |
| **Fidelidade ANSI / VT100 / Truecolor** | ⭐⭐⭐⭐⭐ (Padrão ouro de mercado) | ⭐⭐⭐⭐ (Alta, mas com variações entre SOs) |
| **Performance de Renderização** | ⭐⭐⭐⭐ (60 FPS com Canvas Addon) | ⭐⭐⭐⭐⭐ (Nativo Metal / Android View) |
| **Latência de Teclado** | ⭐⭐⭐⭐ (<16ms com IPC otimizado) | ⭐⭐⭐⭐⭐ (<5ms nativo) |
| **Suporte a Fontes Nerd / Ligaduras** | ⭐⭐⭐⭐ (Via Web Fonts `@font-face`) | ⭐⭐⭐ (Requer registro de fontes nativas em bundle) |
| **Custo de Manutenção / Evolução** | ⭐⭐⭐⭐⭐ (Baixo) | ⭐ (Elevadíssimo) |

**Conclusão:** A abordagem **xterm.js via WebView** é a decisão técnica mais sólida, garantindo velocidade de entrega para o MVP, estabilidade e consistência visual total entre dispositivos iOS e Android.

---

## 3. Investigação de Implementações Reais

### 3.1. Caso 1: 9Remote (`decolua/9remote`)

O [9remote](https://github.com/decolua/9remote) ("Terminal in Your Pocket") é a principal referência arquitetural do nosso projeto. Analisando a estrutura do repositório monorepo (`host/`, `web/`, `desktop/`):

1. **Stack de Transporte e Terminal:**
   - **Host:** Desenvolvido em Node.js com `node-pty` para gerenciar sessões PTY locais e `node-datachannel` para estabelecer conexões WebRTC P2P diretas.
   - **Cliente:** Desenvolvido com React 19 / Next.js e empacotado para mobile com Expo / React Native.
   - **Renderizador:** Utiliza xterm.js no frontend. O tráfego de dados da sessão de terminal flui **diretamente via WebRTC DataChannel** até o cliente sem transitar por servidores centrais.
2. **Abordagem de Teclado no Mobile:**
   - O 9remote fornece uma barra horizontal de toque com teclas modificadoras (`Esc`, `Tab`, `Ctrl`, `Alt`, setas direcionais), permitindo acionar atalhos do shell e navegar no histórico de comandos sem depender das limitações do teclado virtual padrão do celular.
3. **Lições Aprendidas com Issues do 9remote:**
   - *Issue #31:* Problemas de sincronização de estado na reconexão WebRTC. É vital que o cliente xterm re-emita o evento de redimensionamento (`cols`, `rows`) via DataChannel assim que a conexão P2P for restabelecida após oscilações de rede móvel (4G/5G).
   - *Issue #28:* Desacoplamento entre eventos de touch e foco do terminal. Evitar que gestos de rolagem no mobile disparem seleções acidentais de texto.

### 3.2. Caso 2: cmux-mobile (`richardhowes/cmux-mobile`)

O `cmux-mobile` é um aplicativo móvel complementar em Expo 55 / React Native 19 para controle de agentes Claude Code via Tailscale.
- **Implementação do Terminal:** Utiliza um módulo nativo local (`modules/swiftterm/ios`) integrando o SwiftTerm via Swift para renderização nativa ANSI no iOS.
- **Limitação Observada:** O projeto é restrito ao iOS; não foi capaz de oferecer suporte a Android devido à ausência de uma biblioteca equivalente que compartilhasse a mesma camada de abstração em React Native.
- **Barra de Controle:** Implementa botões rápidos para `Enter`, `Ctrl+C`, `Ctrl+D`, setas e `Esc`, integrados com feedback háptico (`Haptic feedback on key interactions`).

---

## 4. O Problema do Teclado Virtual Mobile (Android/iOS) e Como Resolver

### 4.1. O Bug do GBoard / IME no Android (xterm.js #5108, #3600, #6045)

No ambiente desktop, o xterm.js posiciona um elemento invisível `<textarea class="xterm-helper-textarea">` na tela e captura eventos `keydown` e `keypress` para enviar ao terminal.

No mobile (especialmente no Android com Google GBoard ou Samsung Keyboard):
1. Os teclados móveis operam prioritariamente em modo de composição / previsão (*IME - Input Method Editor*).
2. Em vez de emitirem `keydown` com o código da tecla, eles emitem `keyCode: 229` e mantêm o texto em um buffer interno de composição até que o usuário pressione Espaço, Enter ou selecione uma palavra sugerida.
3. **Sintoma Relatado (Issue #5108):** No Android, ao digitar caracteres em um xterm.js dentro do WebView, `term.onData()` não é disparado até que a tecla Enter seja pressionada duas vezes.

#### Mitigações Obrigatórias no HTML/WebView:
Para garantir que cada caractere digitado seja enviado instantaneamente ao PTY:
```html
<textarea
  class="xterm-helper-textarea"
  autocorrect="off"
  autocomplete="off"
  autocapitalize="none"
  spellcheck="false"
  inputmode="text"
  tabindex="-1"
  aria-label="Terminal Input"
></textarea>
```
Além disso, no script do xterm, deve-se adicionar um listener para o evento `input` ou `beforeinput`:
```javascript
const helperTextarea = document.querySelector('.xterm-helper-textarea');
if (helperTextarea) {
  helperTextarea.addEventListener('input', (e) => {
    // Intercepta caracteres inseridos que o keydown (229) reteve
    if (e.inputType === 'insertText' && e.data) {
      window.ReactNativeWebView.postMessage(JSON.stringify({
        type: 'input',
        data: e.data
      }));
    }
  });
}
```

---

## 5. Arquitetura da Barra de Teclado Ergonômica (Accessory Bar)

A barra de teclado estendida é um componente visual do React Native posicionado imediatamente acima do teclado virtual. Ela provê acesso rápido a teclas essenciais que não existem ou são inacessíveis nos teclados do iOS/Android.

```
+-------------------------------------------------------------+
|                     Terminal Viewport                       |
|                     (xterm.js Canvas)                       |
+-------------------------------------------------------------+
| [Esc] [Tab] [Ctrl] [Alt] [ / ] [ - ] [ ~ ] [ ↑ ] [ ↓ ] [ ← ] [ → ] | <-- Accessory Bar
+-------------------------------------------------------------+
|                                                             |
|                 Software Keyboard do SO                     |
|                 (iOS Keyboard / GBoard)                     |
|                                                             |
+-------------------------------------------------------------+
```

### 5.1. Sincronização de Altura e Movimento (60/120 FPS)

O componente nativo do React Native `KeyboardAvoidingView` apresenta engasgos, atrasos na animação e quebra de comportamento com telas *Edge-to-Edge* no Android 15+.

**Solução Recomendada:** Utilizar a biblioteca [`react-native-keyboard-controller`](https://github.com/kirillzyusko/react-native-keyboard-controller) (integrada nativamente ao Expo).
- O componente `KeyboardStickyView` vincula a barra diretamente aos eventos de layout de janela do SO, movimentando a barra em perfeita sincronia com a subida/descida do teclado virtual em ambas as plataformas.

### 5.2. Máquina de Estados para Teclas Modificadoras (`Ctrl` e `Alt`)

Como o usuário não pode manter o dedo pressionado em `Ctrl` no celular enquanto digita outra tecla com a mesma mão, adota-se o padrão de modificadores **Sticky / Latched**:

```
           +-----------------------+
           |       INATIVO         |
           +-----------------------+
              |                 ^
  1 toque no  |                 | Caractere digitado
  botão Ctrl  v                 | OU toque para cancelar
           +-----------------------+
           |     LATCHED (1x)      |
           | (Botão destacado azul)|
           +-----------------------+
              |                 ^
  2º toque no |                 | Toque para destravar
  botão Ctrl  v                 |
           +-----------------------+
           |      LOCKED (2x)      |
           | (Ícone fixado/duplo)  |
           +-----------------------+
```

#### Regra de Conversão de Modificadores:
1. **Modo `Ctrl` Ativo:**
   O código ASCII gerado para qualquer tecla alfabética de `A` a `Z` é calculado pela operação de bitwise AND com `0x1F`:
   $$\text{Código ASCII} = \text{charCode}(\text{Char}) \ \& \ 0\text{x}1\text{F}$$
   Exemplos:
   - `Ctrl+C` $\to$ `'C'.charCodeAt(0) & 0x1F = 67 & 31 = 3 \implies \text{'\x03'}` (ETX / Interrupção)
   - `Ctrl+D` $\to$ `'D'.charCodeAt(0) & 0x1F = 68 & 31 = 4 \implies \text{'\x04'}` (EOF)
   - `Ctrl+Z` $\to$ `'Z'.charCodeAt(0) & 0x1F = 90 & 31 = 26 \implies \text{'\x1a'}` (SIGTSTP)
   - `Ctrl+L` $\to$ `'L'.charCodeAt(0) & 0x1F = 76 & 31 = 12 \implies \text{'\x0c'}` (Limpar tela)
   - `Ctrl+[` $\to$ `'\x1b'` (Equivalente ao Esc)

2. **Modo `Alt` (Meta) Ativo:**
   Prefixa o caractere digitado com a sequência Escape (`\x1b`):
   - `Alt+f` $\to$ `\x1bf` (Avançar uma palavra no bash/zsh)
   - `Alt+b` $\to$ `\x1bb` (Recuar uma palavra no bash/zsh)
   - `Alt+d` $\to$ `\x1bd` (Deletar palavra adiante)

3. **Teclas de Ação Direta:**
   - `Esc`: envia imediatamente `\x1b`
   - `Tab`: envia imediatamente `\t` (`\x09`)
   - `Seta Cima`: envia `\x1b[A`
   - `Seta Baixo`: envia `\x1b[B`
   - `Seta Direita`: envia `\x1b[C`
   - `Seta Esquerda`: envia `\x1b[D`

### 5.3. Prevenção de Perda de Foco (*Prevent Blur*)

Ao tocar em qualquer botão fora do WebView no React Native, o sistema operacional tende a retirar o foco do input ativo, fechando o teclado virtual.

**Técnicas de Prevenção:**
1. **Tratamento no Componente React Native:**
   Utilizar `onPressIn` em vez de `onPress`, e configurar `keyboardShouldPersistTaps="always"` no container raiz.
2. **Prevenção de Foco no Toque:**
   No manipulador de toque dos botões da barra acessória, invocar `event.preventDefault()` para impedir a transferência de foco nativo.
3. **Feedback Tátil:**
   Integrar `expo-haptics` para disparar `Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light)` a cada acionamento de tecla, proporcionando sensação tátil física de clique.

---

## 6. Código de Referência e Especificação de Implementação

### 6.1. Template HTML Embutido do xterm.js (`terminal.html.ts`)

```typescript
export const getTerminalHtml = (theme = 'dark') => `
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/css/xterm.css" />
  <script src="https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/lib/xterm.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/@xterm/addon-fit@0.10.0/lib/addon-fit.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/@xterm/addon-canvas@0.7.0/lib/addon-canvas.js"></script>
  <style>
    * { margin: 0; padding: 0; box-sizing: border-box; }
    html, body {
      width: 100%;
      height: 100%;
      background-color: #0d1117;
      overflow: hidden;
    }
    #terminal-container {
      width: 100%;
      height: 100%;
    }
  </style>
</head>
<body>
  <div id="terminal-container"></div>
  <script>
    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#0d1117',
        foreground: '#c9d1d9',
        cursor: '#58a6ff'
      },
      scrollback: 3000,
      convertEol: true
    });

    const fitAddon = new FitAddon.FitAddon();
    term.loadAddon(fitAddon);

    // Carrega o Canvas Addon para renderização estável a 60 FPS
    try {
      const canvasAddon = new CanvasAddon.CanvasAddon();
      term.loadAddon(canvasAddon);
    } catch (e) {
      console.warn("Canvas Addon fallback para DOM:", e);
    }

    term.open(document.getElementById('terminal-container'));
    fitAddon.fit();

    // Captura entrada do usuário no xterm e despacha ao React Native
    term.onData(data => {
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'input', data }));
    });

    // Notifica redimensionamento de cols/rows
    term.onResize(size => {
      window.ReactNativeWebView.postMessage(JSON.stringify({
        type: 'resize',
        cols: size.cols,
        rows: size.rows
      }));
    });

    // Comunicação recebida do React Native (DataChannel -> Terminal)
    window.addEventListener('message', event => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'output') {
          term.write(msg.data);
        } else if (msg.type === 'fit') {
          fitAddon.fit();
        } else if (msg.type === 'clear') {
          term.clear();
        }
      } catch (err) {
        console.error("Erro ao processar mensagem IPC:", err);
      }
    });

    // Ajusta o tamanho quando a janela for redimensionada pelo teclado
    window.addEventListener('resize', () => {
      setTimeout(() => fitAddon.fit(), 50);
    });

    // Sinaliza que o terminal está pronto
    window.ReactNativeWebView.postMessage(JSON.stringify({
      type: 'ready',
      cols: term.cols,
      rows: term.rows
    }));
  </script>
</body>
</html>
`;
```

---

### 6.2. Componente de Teclado Estendido em React Native (`KeyboardAccessoryBar.tsx`)

```tsx
import React, { useState } from 'react';
import { View, Text, TouchableOpacity, ScrollView, StyleSheet } from 'react-native';
import * as Haptics from 'expo-haptics';

interface KeyboardAccessoryBarProps {
  onSendKey: (sequence: string) => void;
  ctrlActive: boolean;
  setCtrlActive: (active: boolean | ((prev: boolean) => boolean)) => void;
  altActive: boolean;
  setAltActive: (active: boolean | ((prev: boolean) => boolean)) => void;
}

export const KeyboardAccessoryBar: React.FC<KeyboardAccessoryBarProps> = ({
  onSendKey,
  ctrlActive,
  setCtrlActive,
  altActive,
  setAltActive,
}) => {
  const handlePress = (keyAction: () => void) => {
    Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    keyAction();
  };

  return (
    <View style={styles.container}>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.scroll}>
        {/* Modificador Esc */}
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\x1b'))}
        >
          <Text style={styles.keyText}>ESC</Text>
        </TouchableOpacity>

        {/* Modificador Tab */}
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\t'))}
        >
          <Text style={styles.keyText}>TAB</Text>
        </TouchableOpacity>

        {/* Modificador Ctrl (Sticky) */}
        <TouchableOpacity
          style={[styles.keyButton, ctrlActive && styles.keyActive]}
          onPressIn={() => handlePress(() => setCtrlActive(prev => !prev))}
        >
          <Text style={[styles.keyText, ctrlActive && styles.textActive]}>CTRL</Text>
        </TouchableOpacity>

        {/* Modificador Alt (Sticky) */}
        <TouchableOpacity
          style={[styles.keyButton, altActive && styles.keyActive]}
          onPressIn={() => handlePress(() => setAltActive(prev => !prev))}
        >
          <Text style={[styles.keyText, altActive && styles.textActive]}>ALT</Text>
        </TouchableOpacity>

        {/* Teclas de Símbolos Frequentes em Shell */}
        {['|', '/', '-', '~', '$', ';'].map(char => (
          <TouchableOpacity
            key={char}
            style={styles.keyButton}
            onPressIn={() => handlePress(() => onSendKey(char))}
          >
            <Text style={styles.keyText}>{char}</Text>
          </TouchableOpacity>
        ))}

        {/* Setas Direcionais */}
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\x1b[A'))}
        >
          <Text style={styles.keyText}>↑</Text>
        </TouchableOpacity>
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\x1b[B'))}
        >
          <Text style={styles.keyText}>↓</Text>
        </TouchableOpacity>
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\x1b[D'))}
        >
          <Text style={styles.keyText}>←</Text>
        </TouchableOpacity>
        <TouchableOpacity
          style={styles.keyButton}
          onPressIn={() => handlePress(() => onSendKey('\x1b[C'))}
        >
          <Text style={styles.keyText}>→</Text>
        </TouchableOpacity>
      </ScrollView>
    </View>
  );
};

const styles = StyleSheet.create({
  container: {
    height: 44,
    backgroundColor: '#161b22',
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: '#30363d',
  },
  scroll: {
    paddingHorizontal: 8,
    alignItems: 'center',
    gap: 6,
  },
  keyButton: {
    paddingHorizontal: 12,
    paddingVertical: 6,
    borderRadius: 6,
    backgroundColor: '#21262d',
    minWidth: 40,
    alignItems: 'center',
    justifyContent: 'center',
  },
  keyActive: {
    backgroundColor: '#1f6feb',
  },
  keyText: {
    color: '#c9d1d9',
    fontSize: 13,
    fontWeight: '600',
    fontFamily: 'monospace',
  },
  textActive: {
    color: '#ffffff',
  },
});
```

---

## 7. Próximos Passos e Impacto nos Demais Tickets

1. **Ticket 003 (Protocolo DataChannel):**
   - O payload de entrada do xterm.js via bridge já é emitido como string UTF-8 ou buffer binário.
   - O envelope de mensagem no DataChannel deve suportar a mensagem `{"type": "resize", "cols": N, "rows": M}` para que o Host repasse imediatamente o novo tamanho ao PTY (`ptyProcess.resize(cols, rows)`).
2. **Refinamento de UX de Gestos:**
   - Adicionar pinça (*pinch-to-zoom*) para alterar dinamicamente o `fontSize` do terminal (10px a 20px) em tempo de execução sem recarregar o WebView.
3. **Desbloqueio de Tarefas de Implementação:**
   - A especificação deste ticket fornece o modelo completo para criação dos componentes de UI do Cliente Mobile quando o desenvolvimento em código for iniciado.

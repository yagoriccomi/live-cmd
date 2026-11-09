# Plano de Implementação em Fatias Verticais: Tickets Bala Traçante (Ticket 009)

**Data:** 2026-11-06  
**Status:** Pronto para Execução  
**Abordagem:** Fatias verticais ponta a ponta (cada marco produz software executável e testável)

---

## Roteiro de Fatias Verticais (Tracer Bullets)

### Fatia 1: Conexão P2P Mínima e Echo de Terminal (O "Hello World" Remoto)
- **Objetivo:** Estabelecer a primeira conexão WebRTC DataChannel e validar que o que for digitado no celular ecoa no PC e vice-versa.
- **Entregáveis:**
  - Worker de sinalização básico implantado na Cloudflare.
  - CLI em Rust instanciando um `webrtc-rs` PeerConnection local.
  - App Expo conectando via `react-native-webrtc` e trocando strings simples por um DataChannel.
- **Critério de Aceite:** Digitar uma mensagem no celular e vê-la impressa no terminal do computador.

### Fatia 2: Terminal PTY Real com xterm.js Canvas
- **Objetivo:** Acoplar o `portable-pty` no Rust e o `xterm.js` na WebView móvel.
- **Entregáveis:**
  - Host spawna sessão real do PowerShell/Bash com ConPTY/POSIX.
  - Cliente Mobile renderiza xterm.js via Canvas com redimensionamento dinâmico (`resize`).
  - Barra auxiliar de teclado com `Ctrl+C`, `Esc` e setas direcionais funcionando.
- **Critério de Aceite:** Executar comandos interativos como `top`/`htop`, navegar no histórico com setas e interromper processos com `Ctrl+C` pelo celular.

### Fatia 3: Pareamento Seguro por QR Code & Chaves Permanentes
- **Objetivo:** Eliminar hardcode de sessões e implementar a autorização por QR Code e chaves Ed25519.
- **Entregáveis:**
  - Host imprime QR Code no terminal com OTK de 5 minutos.
  - App abre câmera, escaneia, valida OTK e troca chaves públicas.
  - Reabertura do app conecta direto sem novo QR Code.
- **Critério de Aceite:** Fechar o app no celular e reconectar ao PC de outra rede sem precisar de novo pareamento.

### Fatia 4: Transferência Resumível de Arquivos (Chunks 64KB & Resume)
- **Objetivo:** Implementar o `file-channel` independente com capacidade de retomar uploads interrompidos.
- **Entregáveis:**
  - Protocolo de fatiamento em chunks de 64 KB com hash SHA-256 no Rust e React Native.
  - Persistência em staging temporário com expurgo automático após 24 horas.
  - Barra de progresso visual de upload e download no celular.
- **Critério de Aceite:** Iniciar o envio de um arquivo de 100 MB, desligar o Wi-Fi no meio, religar e ver a transferência concluir do ponto onde parou.

### Fatia 5: App Desktop Híbrido (Tray, Autostart e Modo Cliente)
- **Objetivo:** Empacotar o Host em Tauri 2.x com ícone na bandeja do sistema e cliente desktop unificado.
- **Entregáveis:**
  - Configuração de bandeja do sistema (System Tray) com inicialização no boot.
  - Modo silencioso total (sem janelas na tela física quando acessado remotamente).
  - Interface desktop para visualizar e conectar a outros terminais remotos.
- **Critério de Aceite:** O computador reinicia, o serviço sobe silenciosamente no tray e fica acessível pelo celular sem nenhuma ação manual no PC.

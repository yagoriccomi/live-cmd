# Handoff: Gerenciador de Terminais Remoto — Concepção e Especificações Completas

**Data:** 06/11/2026  
**Origem:** Sessão de Mapeamento do Caminho (`user:mapear-caminho-mp`)  
**Status do Projeto:** Fase de Concepção e Decisões 100% Concluída → Pronto para Construção  
**Próximo Foco:** Implementação da Fatia 1 (Tracer Bullet 1) — Setup do monorepo, Worker de sinalização Cloudflare e primeiro eco de caracteres via WebRTC DataChannel.

---

## 1. Visão Geral do Projeto

Construção de uma solução completa de acesso remoto a terminais inspirada no [9remote](https://github.com/decolua/9remote), composta por:
1. **Host Daemon & Cliente Desktop:** Aplicativo único em **Rust** (Tauri 2.x + `portable-pty`), com inicialização automática e execução silenciosa na bandeja do sistema (System Tray). Atua servindo terminais locais (com ConPTY nativo no Windows e POSIX no Linux/macOS) e como cliente para acessar outros computadores remotos.
2. **Cliente Mobile:** Aplicativo em **React Native / Expo** integrando emulador `xterm.js` via Canvas 2D de alta performance, barra de teclado acessória ergonômica (`react-native-keyboard-controller`) com estados *Latched/Locked* para `Ctrl`/`Alt`, reconexão silenciosa automática, transferência de arquivos resumível e autenticação biométrica opcional (Face ID / digital).
3. **Sinalização & NAT Traversal:** **Cloudflare Workers com Durable Objects** (WebSocket Hibernation a custo $0,00) + STUN público híbrido e **Cloudflare Realtime TURN** (1 TB/mês gratuito com TLS 443) para contornar CGNAT simétrico de operadoras celulares (4G/5G).
4. **Canais WebRTC Isolados:**
   - `pty-channel`: tráfego interativo de texto do terminal com latência mínima.
   - `file-channel`: envio de arquivos fatiados em blocos indexados de 64 KB com hash SHA-256 e retomada automática em caso de queda de rede (estilo torrent); expurgo de partes incompletas após 24 horas.
5. **Segurança & Pareamento:** Pareamento instantâneo via QR Code efêmero (OTK de 5 minutos) gerando chaves permanentes **Ed25519** ponta a ponta para reconexão remota contínua.

---

## 2. Mapa de Artefatos no Repositório

| Artefato | Caminho | Descrição |
| :--- | :--- | :--- |
| **Glossário Canônico** | `CONTEXT.md` | Termos oficiais do domínio (`Host`, `Cliente Mobile`, `Sessão PTY`, `Canal P2P`, `OTK`, `Transferência Resumível`). |
| **Decisões Tomadas** | `answers/` | 3 registros completos com todas as perguntas e respostas levantadas no processo. |
| **Pesquisa Runtime PTY** | `pesquisas/001-runtime-pty-host.md` | Análise profunda demonstrando a superioridade de Rust + `portable-pty` sobre Go e Node.js. |
| **Pesquisa Sinalização** | `pesquisas/002-sinalizacao-webrtc-nat.md` | Especificação da infraestrutura Cloudflare Workers + TURN 1 TB/mês a custo zero. |
| **Pesquisa Terminal Mobile** | `pesquisas/005-emulador-terminal-react-native.md` | Integração do xterm.js Canvas, correção do bug de IME do Android e componente de teclado. |
| **Mapa do Wayfinder** | `tickets/mapa.md` | Índice canônico de tickets com o destino alcançado. |
| **Spec Host / Desktop** | `tickets/006-spec-arquitetura-host-rust.md` | Módulos `pty_manager`, `webrtc_peer`, `file_transfer`, buffer de 10.000 linhas e tray. |
| **Spec Protocolo** | `tickets/007-spec-protocolo-mensagens.md` | Esquemas de framing binário PTY, sinalização WebSocket e chunks de arquivos. |
| **Spec Mobile** | `tickets/008-spec-app-mobile-react-native.md` | Arquitetura Expo Router, leitor QR, biometria e fila de transferências. |
| **Roteiro em Fatias** | `tickets/009-plano-implementacao-fatias.md` | 5 marcos verticais no padrão bala traçante (tracer bullet) para desenvolvimento guiado. |

---

## 3. Roteiro Imediato para o Próximo Agente (Fatia 1)

O próximo agente (Claude / Codex) deve focar estritamente na **Fatia 1**:
1. **Estruturar o Monorepo:**
   - `desktop/`: Projeto Cargo em Rust com Tauri 2.x e dependências (`webrtc-rs`, `tokio`, `portable-pty`, `serde`).
   - `mobile/`: Projeto Expo SDK 52 com TypeScript (`react-native-webrtc`, `react-native-webview`).
   - `signaling/`: Projeto Cloudflare Worker com Durable Objects e TypeScript (`wrangler`).
2. **Implantar Sinalização Mínima:**
   - Criar o Worker que gerencia salas de sinalização e repassa mensagens de oferta/resposta SDP entre dois WebSockets.
3. **Conexão P2P Mínima de Eco:**
   - Criar script/CLI no Host conectando ao Worker e abrindo um DataChannel.
   - Criar tela básica no app mobile conectando ao mesmo canal e enviando uma mensagem de texto.
   - Validar que o texto digitado no mobile ecoa no terminal do computador.

---

## 4. Skills Sugeridas para a Próxima Sessão

Para conduzir a implementação de código com máxima qualidade, o próximo agente deve utilizar:
- `user:implementar-spec-mp`: Para orientar a construção direta a partir das especificações dos tickets 006 a 008.
- `user:executar-projeto`: Para coordenar o ciclo de vida da tarefa, fila de branches e revisão contínua.
- `user:tdd-mp`: Para guiar o desenvolvimento do protocolo de mensagens e chunking de arquivos orientado a testes.
- `user:design-de-codebase-mp`: Para estruturar os módulos profundos e isolamento de dependências no Rust e React Native.
- `user:criar-projeto`: Caso queira gerar o scaffolding inicial padronizado de scripts e automação local.

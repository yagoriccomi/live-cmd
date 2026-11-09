# Especificação Técnica: Host Daemon & Cliente Desktop em Rust (Ticket 006)

**Data:** 2026-11-06  
**Status:** Especificação Aprovada  
**Stack:** Rust 2024 / Tauri 2.x / `portable-pty` / `webrtc-rs` / `tokio`

---

## 1. Visão Geral e Papel Duplo

O aplicativo de desktop possui papel híbrido e roda de forma silenciosa na bandeja do sistema (System Tray) com inicialização automática opcional:
1. **Host Daemon:** Cria e mantém pseudo-terminais virtuais (`PTY`), escuta conexões WebRTC P2P e gerencia upload/download de arquivos locais.
2. **Cliente Desktop:** Oferece interface gráfica nativa para acessar tanto os terminais locais deste computador quanto os terminais de outros computadores remotos autenticados.

---

## 2. Módulos Internos do Binário Rust

```
┌────────────────────────────────────────────────────────┐
│                   Tauri Desktop GUI                    │
│   (Gerenciador de Abas, Terminal Local/Remoto, Tray)   │
└──────────────────────────┬─────────────────────────────┘
                           │ IPC
┌──────────────────────────▼─────────────────────────────┐
│                      Host Core                         │
│  ┌─────────────────┐ ┌──────────────────────────────┐  │
│  │  SessionManager │ │       WebRtcEngine           │  │
│  │ (portable-pty,  │ │ (webrtc-rs, DataChannels:    │  │
│  │  scrollback RB) │ │  pty-channel / file-channel) │  │
│  └────────┬────────┘ └──────────────┬───────────────┘  │
│           │                         │                  │
│  ┌────────▼────────┐ ┌──────────────▼───────────────┐  │
│  │  FileTransfer   │ │        SecurityStore         │  │
│  │ (Chunked 64KB,  │ │ (Ed25519 Keys, Paired Devices│  │
│  │  TTL 24h Clean) │ │  OTK Generator 5min)         │  │
│  └─────────────────┘ └──────────────────────────────┘  │
└────────────────────────────────────────────────────────┘
```

### 2.1. `SessionManager` (Gerenciador de Sessões PTY)
- **Biblioteca:** `portable-pty` com backend `ConPty` no Windows 10/11 e `UnixMasterPty` no Linux/macOS.
- **Estrutura de Sessão:**
  ```rust
  pub struct PtySession {
      pub id: Uuid,
      pub title: String,
      pub cwd: PathBuf,
      pub master: Box<dyn MasterPty + Send>,
      pub child: Box<dyn Child + Send + Sync>,
      pub scrollback: RingBuffer<u8>, // Mantém 10.000 linhas em memória
      pub last_activity: Instant,
  }
  ```
- **Ciclo de Vida & Expiração (24h):** Uma rotina periódica em segundo plano (`tokio::time::interval`) inspeciona sessões ativas. Se uma sessão passar mais de 24 horas consecutivas sem conexões ativas de clientes e sem I/O, o processo filho é encerrado com `SIGTERM`/`TerminateProcess` e sua memória liberada.
- **Scrollback Replay:** Ao reconectar, o Host envia imediatamente o conteúdo acumulado no buffer circular para o cliente recuperar a tela sem perda de histórico.

### 2.2. `WebRtcEngine` (Comunicação P2P)
- **Implementação:** `webrtc-rs` assíncrono sobre `tokio`.
- **Dois DataChannels Isolados:**
  - `pty-channel`: `ordered = true`, `max_retransmits = None` (SCTP confiável de baixa latência para comandos de teclado, redimensionamento de janela e saída do terminal).
  - `file-channel`: `ordered = true`, `max_packet_life_time = None` (dedicado a blocos binários de arquivos, com leitura assíncrona desacoplada do terminal).

### 2.3. `FileTransferManager` (Transferência Resumível)
- **Diretório Ativo:** O envio de arquivos tem como destino padrão o diretório onde o terminal da sessão ativa está aberto (`cwd`), com permissão para navegar e selecionar qualquer diretório autorizado.
- **Chunking & Hashing:**
  - Fatiamento em chunks fixos de 64 KB (`65.536 bytes`).
  - Hash SHA-256 por chunk e hash total do arquivo no manifesto inicial.
  - O Host grava pedaços parciais em um diretório temporário de staging (`~/.remoteterm/staging/<file_id>/`).
- **Retomada Estilo Torrent:** Se a rede cair, o cliente envia um `ResumeRequest { file_id }`. O Host responde com um `BitField` indicando os blocos já persistidos, e a transmissão recomeça apenas dos blocos ausentes.
- **Limpeza de 24 Horas:** Arquivos parciais em staging que não receberem novos chunks durante 24 horas são expurgados do disco automaticamente.

### 2.4. `SecurityStore` (Segurança & Chaves)
- **Geração de Chaves:** Par de chaves Ed25519 permanente gerado na primeira inicialização e armazenado de forma protegida em `~/.remoteterm/keys/`.
- **One-Time Key (OTK):** Ao solicitar pareamento, o Host gera um token criptográfico de uso único válido por 5 minutos, empacotado no QR Code junto à URL do servidor de sinalização e `sessionId`.
- **Lista de Dispositivos Pareados:** Arquivo JSON protegido (`paired_devices.json`) contendo `device_id`, chave pública do cliente, nome do aparelho e data do pareamento. Permite revogação individual via CLI ou interface gráfica.
- **Modo Silencioso:** Nenhuma notificação ou indicador surge na tela do computador durante o acesso remoto pelo celular.

### 2.5. Integração com a Bandeja do Sistema (Tray) & Autostart
- Ícone nativo na bandeja do sistema com menu de contexto:
  - *Abrir Terminal Desktop*
  - *Novo Pareamento (Exibir QR Code)*
  - *Dispositivos Conectados*
  - *Iniciar com o Sistema (Checkbox)*
  - *Sair*
- Configuração de autostart via registro no Windows (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`), `launchd` no macOS e `.desktop` em `~/.config/autostart/` no Linux.

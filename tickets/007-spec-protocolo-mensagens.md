# Especificação Técnica: Protocolo de Mensagens & Sinalização (Ticket 007)

**Data:** 2026-11-06  
**Status:** Especificação Aprovada  
**Protocolo:** WebRTC DataChannel + WebSocket Signaling

---

## 1. Fluxo de Sinalização (Cloudflare Workers)

A sinalização ocorre exclusivamente durante a fase de negociação de conexão via WebSocket seguro (`wss://signaling.seudominio.com/ws/:sessionId`).

```
Mobile Client                       Signaling Server (Worker)                      Host Daemon
      │                                       │                                         │
      │ ── 1. Connect (sessionId, OTK) ──────>│<── 1. Connect (sessionId) ───────────── │
      │                                       │                                         │
      │ ── 2. SDP Offer ─────────────────────>│ ── Forward Offer ──────────────────────>│
      │                                       │<── 2. SDP Answer ────────────────────── │
      │<── Forward Answer ────────────────────│                                         │
      │                                       │                                         │
      │ ── 3. ICE Candidates ────────────────>│ ── Forward Candidates ─────────────────>│
      │<── Forward Candidates ────────────────│<── 3. ICE Candidates ────────────────── │
      │                                       │                                         │
      │◄═════════════════ 4. Direct WebRTC DataChannels Established ═══════════════════►│
      │                                       │                                         │
      │ (Signaling Server WebSocket Hibernates / Drops)                                 │
```

### 1.1. Carga Útil do QR Code de Pareamento
O QR Code gerado pelo Host codifica uma URI estruturada no padrão:
```
remoteterm://pair?session=<uuid>&otk=<base64_otk>&host_pk=<base64_ed25519_pk>&server=signaling.seudominio.com
```

---

## 2. Estrutura dos WebRTC DataChannels

Uma vez estabelecida a conexão P2P, a comunicação se divide em dois canais estritamente separados:

### 2.1. Canal de Terminal (`pty-channel`)

Utiliza um framing binário leve e eficiente:
- **Byte de Controle (`0x01` a `0x05`):**
  - `0x01`: Entrada do Teclado (`Client -> Host`) — seguido de bytes UTF-8 digitados.
  - `0x02`: Saída do Terminal (`Host -> Client`) — seguido de bytes ANSI/VT100 para renderização.
  - `0x03`: Redimensionamento de Janela (`Client -> Host`) — payload de 4 bytes: `cols: u16`, `rows: u16`.
  - `0x04`: Ping / Heartbeat para verificação de latência.
  - `0x05`: Notificação de Término de Processo (`Host -> Client`) — payload indicando retorno ao shell prompt ou código de saída (`exit_code: i32`).

### 2.2. Canal de Transferência de Arquivos (`file-channel`)

Utiliza mensagens codificadas em JSON para metadados e blocos binários para conteúdo:

#### A. Início de Transferência (`FileTransferInit`)
```json
{
  "type": "file_init",
  "file_id": "f8a9e10c-3b4a-4d32-b2a1-...",
  "filename": "backup.tar.gz",
  "size_bytes": 104857600,
  "chunk_size": 65536,
  "total_chunks": 1600,
  "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "target_dir": null // null = usa o diretório atual do terminal (cwd)
}
```

#### B. Envio de Chunks Binários (`FileChunk`)
Cada mensagem enviada no canal é precedida por um cabeçalho binário de 16 bytes:
- `file_id`: 8 bytes truncados ou índice da sessão.
- `chunk_index`: 4 bytes (`u32` Big Endian).
- `payload_len`: 4 bytes (`u32` Big Endian).
- `data`: até 65.536 bytes binários do arquivo.

#### C. Confirmação e Controle de Fluxo (`FileAck`)
```json
{
  "type": "file_ack",
  "file_id": "f8a9e10c-3b4a-4d32-b2a1-...",
  "received_chunk": 42,
  "buffered_amount": 0
}
```

#### D. Negociação de Retomada (*Resume*)
Quando a conexão for restabelecida após queda:
```json
// Cliente solicita retomar:
{
  "type": "file_resume_request",
  "file_id": "f8a9e10c-3b4a-4d32-b2a1-..."
}

// Host responde com mapa de blocos recebidos:
{
  "type": "file_resume_response",
  "file_id": "f8a9e10c-3b4a-4d32-b2a1-...",
  "missing_chunks": [43, 44, 45, 120, 121]
}
```

---

## 3. Máquina de Estados da Reconexão Silenciosa

1. **Detecção:** O cliente móvel escuta o evento `peerConnection.onconnectionstatechange`. Se o estado transitar para `disconnected` ou `failed`:
2. **Bufferização de Entrada:** O cliente entra em modo `"Reconectando..."`. Teclas digitadas durante a oscilação são retidas em fila local temporária.
3. **Re-sinalização Automática:** O cliente reconecta ao WebSocket com as chaves permanentes Ed25519 já pareadas (sem QR Code).
4. **Re-handshake P2P:** Um novo `RTCPeerConnection` é negociado em menos de 1,5 segundo.
5. **Replay & Flush:** O Host envia o buffer de scrollback das linhas geradas durante a queda e o cliente descarrega as teclas acumuladas.

# SPEC C3 — Canais, framing e sessões PTY

**Status: APROVADA em 2026-11-08 (answers 012) — implementação só com liberação explícita; aceite de M2 exige testes em Android e iOS reais.**
Decisões aplicadas: answers 006, 007 e 010 (desvios D1–D8 aprovados) e 012 (D9–D10 aprovados).

Base: `PLANO-live-cmd.md` seção 7 (C3); `SPEC-C2-identidade.md`. Esta spec detalha C3 sem alterar o PLANO; o que muda está em "Desvios" (seção 10). Vocabulário: `CONTEXT.md`. Arquivos (`file-channel`, chunks, resume) ficam em C4; aqui só a convivência com o terminal.

**Códigos de erro:** a fonte única é a seção 2.2 de `SPEC-C2-identidade.md`. Esta spec não cria códigos.

Convenções:
- Inteiros binários em big-endian.
- `session_id` = UUID v4 gerado pelo host. No JSON vai como texto minúsculo; no frame, como 16 bytes.
- Offsets e sequências u64 vão no JSON como **string decimal** (sem `Number` JS), regex `^(0|[1-9][0-9]{0,19})$`, valor ≤ 2^64−1.

## 1. Canais

| Canal | Criado por | Propriedades | Conteúdo |
|---|---|---|---|
| `pty-channel` | offerer, antes da oferta | confiável, ordenado, sem `maxRetransmits`/`maxPacketLifeTime`, `protocol` vazio | handshake C2 (texto), controles C3 (texto), frames PTY (binário) |
| `file-channel` | offerer, antes da oferta | idem | C4 |

- O host fecha a PeerConnection (CLOSED) se:
  - surgir um canal com outro rótulo;
  - um dos dois vier com propriedades diferentes;
  - um rótulo vier duplicado.
- **Tamanho máximo real de mensagem:**
  - Cada lado calcula `min(max-message-size local, remoto)` a partir do SDP.
  - Se o resultado for menor que 65 536 bytes, fecha a PeerConnection após AUTHENTICATED, com `LIMIT_EXCEEDED` no log local.
  - Nenhuma mensagem C3 passa de 65 536 bytes (seções 2 e 3).
- **Antes de AUTHENTICATED** (C2 seção 2): qualquer controle C3, frame binário ou mensagem no `file-channel` leva a CLOSED.

## 2. Controles (texto JSON)

**Formato.** Mesmo formato da C2: `{v:1, type, connection_id, ...}`, com limite de ≤ 65536 bytes da mensagem UTF-8 e campos desconhecidos → `INVALID_MESSAGE`.

**Correlação.** Pedidos levam `request_id` (UUID v4 novo por pedido). A resposta repete o `request_id`.

**Erros fatais e não fatais:**
- `error` **com** `request_id` é não fatal: o pedido falhou e a conexão continua.
- `error` **sem** `request_id` é fatal: o emissor fecha a PeerConnection.
- `INVALID_MESSAGE` é sempre fatal.
- No handshake C2 todo erro é fatal; isso não muda.

### 2.1 Pedidos do cliente → respostas do host

| Pedido | Campos | Resposta OK | Erros não fatais |
|---|---|---|---|
| `session_create` | `profile_id` (string, ID de perfil definido no host; o cliente nunca envia linha de comando), `cols`, `rows` (1–1000), `label?` (≤ 64 chars) | `session_create_ok`: `session_id` | `NOT_FOUND` (perfil desconhecido), `LIMIT_EXCEEDED`, `IO_ERROR` |
| `session_list` | — | `session_list_ok`: `sessions[]` (seção 2.3), `profiles[]` `{profile_id, label}` | — |
| `session_attach` | `session_id`, `from_offset` (string \| `null`), `cols?`, `rows?` | `session_attach_ok`: `session_id`, `state`, `buffer_start`, `output_end`, `last_input_seq`, `exit_code?` | `NOT_FOUND`, `LIMIT_EXCEEDED` |
| `session_detach` | `session_id` | `session_detach_ok` | `NOT_FOUND` |
| `session_close` | `session_id` | `session_close_ok` (o processo foi encerrado; segue o frame `05`) | `NOT_FOUND`, `IO_ERROR` |
| `client_focus` | `state` (`"foreground"`\|`"background"`) | `client_focus_ok` | — (D10; seção 9 item 8) |

### 2.2 Mensagens espontâneas do host

| type | Campos | Quando |
|---|---|---|
| `input_ack` | `session_id`, `seq` | Cumulativo: tudo até `seq` foi aceito pelo PTY. Pode ser agrupado, com atraso máximo de 100 ms. |
| `replay_begin` | `session_id`, `from_offset`, `to_offset` | Antes dos frames `02` de replay. |
| `replay_gap` | `session_id`, `requested`, `available_from` | `from_offset < buffer_start`. O replay começa em `available_from`. |
| `replay_end` | `session_id`, `offset` | Fim do replay; a partir daqui a saída é ao vivo e contínua. |
| `session_exited` | `session_id`, `exit_code`, `final_offset` | O processo do PTY terminou. É o espelho em texto do frame `05`. |
| `screen_snapshot` | `session_id`, `offset`, `cols`, `rows`, `data` (formato a definir, seção 9 item 3) | Antes do replay, quando houver gap (D9). |
| `command_completed` | `session_id`, `exit_code?` | Via integração de shell OSC 133 (seção 9 item 6). Nunca deduzido por regex. |
| `cwd_changed` | `session_id`, `cwd` (\| `null` = desconhecido) | Via integração de shell OSC 7 (seção 9 item 6). |
| `session_revoked` | — | Não existe. A revogação fecha a PeerConnection (seção 7). |

**Respostas a pedidos sem contexto.** Pedido com `session_id` que este dispositivo não está autorizado a ver: `NOT_FOUND`, igual a um ID inexistente, para não revelar que a sessão existe.

### 2.3 Entrada de `session_list`

`{session_id, label, state:"running"|"exited", created_at_ms, last_activity_ms, cols, rows, attached_count}`

- Tempos em ms Unix como string decimal.
- Sem paginação: o limite de sessões (seção 6) garante que cabe em ≤ 65536 bytes da mensagem UTF-8.

## 3. Frames PTY (binário)

```
version:u8 (=1) | opcode:u8 | session_id:16 | sequence:u64 | payload_len:u32 | payload
```

O cabeçalho tem 30 bytes. `payload_len` precisa ser igual ao tamanho do frame menos 30; senão `INVALID_MESSAGE`. Versão desconhecida → `VERSION_UNSUPPORTED`.

| Op | Nome | Direção | `sequence` | Payload |
|---|---|---|---|---|
| 01 | input | cliente → host | seq de input (seção 4) | 1–16 384 bytes brutos |
| 02 | output | host → cliente | offset do 1º byte do payload no fluxo de saída da sessão | 1–16 384 bytes brutos |
| 03 | resize | cliente → host | 0 | `cols:u16`, `rows:u16`, ambos 1–1000 |
| 04 | heartbeat | ambos | id do ping; o eco repete o id | 0 bytes; `session_id` = 16 zeros (o único opcode que aceita zeros) |
| 05 | session_exited | host → cliente | `final_offset` | `exit_code:i32` (no Windows, o u32 reinterpretado) |

- Os bytes do PTY não são decodificados como UTF-8 em nenhum ponto do transporte. Ctrl+C é o byte `0x03` no opcode 01.
- Opcode desconhecido, `session_id` não anexado por esta conexão ou tamanho fora da faixa → `INVALID_MESSAGE` (fatal).
- **Heartbeat:**
  - Cada lado envia um a cada 10 s.
  - Sem nenhum frame ou mensagem recebida em 30 s, fecha a PeerConnection e o cliente reconecta (C2 `reconnect`).
  - Só com o app em primeiro plano (seção 9 item 9); em segundo plano não há heartbeat.
- Um frame tem no máximo 30 + 16 384 = 16 414 bytes.

## 4. Entrada

- `seq` é contado por `(session_id, client_pk)`, começa em 1 e cresce de 1 em 1. O host guarda `last_input_seq` em memória enquanto a sessão existir; a contagem sobrevive à reconexão.
- No host:
  - `seq ≤ last_input_seq`: descarta (duplicata) e reenvia o `input_ack` com o valor atual.
  - `seq = last + 1`: escreve no PTY e depois confirma.
  - `seq > last + 1`: `INVALID_MESSAGE`.
- O ACK significa aceito pelo PTY, **não** que o comando foi executado.
- Janela: o cliente mantém no máximo 64 KiB ou 256 frames sem ACK. Além disso, retém localmente.
- Na reconexão, `session_attach_ok.last_input_seq` diz o que foi aceito. O cliente pode reenviar apenas o que tiver `seq > last_input_seq` e tiver sido digitado **com a conexão ativa** antes da queda. O mesmo `seq` mais a deduplicação torna esse reenvio seguro.
- Se a sessão não existir mais (`NOT_FOUND`, reinício do host, TTL), a entrada pendente é descartada e o usuário é avisado. Nunca é reenviada para outra sessão.
- O que foi digitado **durante** a desconexão só é enviado após confirmação do usuário (seção 9 item 2).
- Input para uma sessão `exited` é descartado sem ACK; o cliente já recebeu o `05`.

## 5. Saída, buffer e replay

- O offset de saída é a contagem de bytes desde a criação da sessão. É monotônico e nunca reinicia.
- O ring buffer guarda até **10 000 linhas** (contando `\n`) **ou 8 MiB** por sessão, o que vier primeiro. `buffer_start` é o offset do byte mais antigo retido.
- O host lê o PTY sempre, mesmo com cliente lento ou ausente. Quem ficar para trás perde o que saiu do buffer e recebe `replay_gap`. O PTY nunca é bloqueado por causa da rede.
- **Attach:**
  - Com `from_offset = null`: replay de todo o buffer.
  - Com `from_offset` entre `buffer_start` e `output_end`: replay a partir dele.
  - Com `from_offset > output_end`: `INVALID_MESSAGE`.
  - A ordem é sempre `session_attach_ok` → (`replay_gap` → `screen_snapshot`) → `replay_begin` → frames `02` → `replay_end` → saída ao vivo, sem buraco de offset. O `offset` do snapshot é igual ao `available_from` do replay.
- Depois de um gap, o cliente limpa o emulador, aplica o `screen_snapshot` (D9, seção 9 item 3) e segue com o replay. A recuperação total não é alegada até ser demonstrada com `vim`/`htop`.
- **TTL:**
  - A sessão é encerrada após 24 h **sem cliente anexado e sem I/O**. O relógio é injetável nos testes.
  - Desconectar não encerra o PTY.
  - As sessões não sobrevivem a reinício do host.

## 6. Limites e backpressure

| Limite | Valor proposto | Excesso |
|---|---|---|
| Sessões por host | 16 | `LIMIT_EXCEEDED` no `session_create` |
| Anexos simultâneos por sessão | 4 conexões | `LIMIT_EXCEEDED` no `session_attach` |
| Controles por conexão | 50/s, rajada de 100 | `error` `LIMIT_EXCEEDED` sem `request_id` (fatal) |
| Fila de envio do host por (conexão, sessão) | 1 MiB | para de enfileirar; quem ficar para trás recebe `replay_gap` ao recuperar |
| `bufferedAmount` do `pty-channel` | pausa acima de 1 MiB; retoma em `bufferedamountlow` (256 KiB) | — |
| Input sem ACK | 64 KiB / 256 frames | retido no cliente |

**Convivência com C4:** o remetente do `file-channel` deve:
- limitar o próprio `bufferedAmount`, com o valor definido em C4;
- ceder a vez sempre que houver frame PTY pendente no mesmo lado.

Canais separados não garantem prioridade na rede (PLANO C3); o teste N9 mede isso.

## 7. Autorização e revogação

- Todo pedido checa se o `client_pk` autenticado na conexão continua na trust store e não foi revogado, (todo dispositivo autorizado vê todas as sessões, seção 9 item 1).
- **Revogar (C2 seção 6)** faz, de forma síncrona:
  1. Fecha todas as PeerConnections do `client_pk`.
  2. Remove os anexos dele de todas as sessões.
  3. Descarta as filas de envio e o estado de input dele.
  4. Cancela as transferências (C4).

  As sessões PTY continuam vivas: elas pertencem ao host. Uma reconexão desse dispositivo recebe `REVOKED` no handshake.

## 8. Matriz de aceite

| # | Caso | Esperado |
|---|---|---|
| A1 | Criar → anexar → digitar `echo` → ver a saída | bytes idênticos; `input_ack` recebido |
| A2 | Queda de rede → reconexão → attach com `from_offset` | replay sem buraco nem duplicata de offset; `last_input_seq` correto |
| A3 | Input reenviado após reconexão (`seq` já aceito) | descartado e re-ACK; o PTY recebe uma vez só |
| A4 | Saída maior que o buffer com cliente ausente | `replay_gap` com `available_from = buffer_start` |
| A5 | Dois clientes anexados | os dois recebem a mesma saída e os mesmos offsets |
| A6 | `session_close` | o processo some; `session_close_ok` e `05` com `final_offset` |
| A7 | TTL com relógio controlado | sessão encerrada aos 24 h, não antes; I/O ou attach zera a contagem |
| A8 | Bytes não UTF-8 e Ctrl+C (`0x03`) | entregues intactos; Ctrl+C interrompe o processo no Windows (ConPTY) e no Unix |
| N1 | Controle ou frame antes de AUTHENTICATED | CLOSED |
| N2 | Canal com outro rótulo ou parcialmente confiável | CLOSED |
| N3 | Frame com `payload_len` divergente, opcode desconhecido ou resize 0/1001 | `INVALID_MESSAGE`, fatal |
| N4 | `seq` pulando (`last + 2`) | `INVALID_MESSAGE` |
| N5 | Attach a sessão inexistente ou não autorizada | `NOT_FOUND` igual nos dois casos, não fatal |
| N6 | 17ª sessão; 5º anexo; rajada de controles | `LIMIT_EXCEEDED` |
| N7 | Revogação com sessão anexada e saída fluindo | conexão fechada em até 1 s; sessão viva; reconexão `REVOKED` |
| N8 | Cliente lento (sem ler) com saída de 50 MiB | host não bloqueia o PTY; memória limitada; `replay_gap` ao retomar |
| N9 | Upload C4 a toda velocidade e digitação simultânea | eco do input com p95 < 150 ms em rede local (seção 9 item 8) |
| N10 | Offset/seq JSON como número ou acima de 2^64−1 | `INVALID_MESSAGE` |
| N11 | Heartbeat ausente por 30 s | PeerConnection fechada; o cliente reconecta |

## 9. Itens em aberto (decisão do usuário)

1. ~~**Visibilidade das sessões.**~~ **Decidido** (answers 006): todos os dispositivos pareados e não revogados veem todas; o host checa dispositivo e revogação em todo pedido.
2. ~~**Teclas digitadas com a conexão caída.**~~ **Decidido** (answers 006): não enviar sozinho; mostrar o texto retido e enviar só com toque de confirmação.
3. **Restaurar após `replay_gap`.** **Decidido** (answers 007, resposta livre "guardar e reexibir"): o host guarda o estado e reexibe; interpretado como (b) emulador de terminal no host gerando snapshot da tela, enviado antes do replay. Aberto (técnico): escolher a crate de emulação (ex. `vt100`/`alacritty_terminal`) e o formato do snapshot.
4. **Tamanho com vários clientes.** Decidido pelo usuário (answers 006): PTY em segundo plano no host (sem janela); cada cliente com tamanho próprio e texto sincronizado.
   **Aberto (técnico):** um PTY tem um só `cols`/`rows`. Texto corrido (shell, logs) pode ser refluído no cliente; apps de tela cheia (`vim`, `htop`) não. Opções: (a) PTY no tamanho do último cliente ativo, demais refluem/escalam; (b) emulador no host renderizando por cliente. **Recomendação:** (a) em M3; validar com `vim`/`htop` antes de considerar (b).
5. ~~**Shell da sessão.**~~ **Decidido** (answers 006): perfis definidos só no host, escolhidos por ID; o cliente nunca envia linha de comando.
6. **`command_completed` / `cwd_changed`.** **Decidido** (answers 007): já na primeira versão (M3), via integração de shell OSC 133 / OSC 7 nos perfis do host (pwsh, cmd, WSL). Aberto (técnico): `cmd` não emite OSC nativamente; definir fallback (ex. `PROMPT` customizado) ou marcar sem suporte.
7. ~~**Sessão `exited`.**~~ **Decidido** (answers 007): buffer disponível até o TTL de 24 h ou até `session_close`.
8. **Atualização da tela.** **Decidido** (answers 007, resposta livre): celular com tela ligada/em foco e janela Windows em foco = ao vivo; celular em segundo plano = sem tráfego, sincroniza (replay/snapshot) ao voltar; cliente PC fora de foco = atualiza a cada 5 s. Meta p95 < 150 ms em rede local para o modo ao vivo (recomendação, não confirmada).
9. **Heartbeat.** Consequência do item 8: só com app em primeiro plano (10 s / queda após 30 s, recomendação); em segundo plano o celular não mantém heartbeat e a reconexão C2 ocorre ao reabrir.
10. **Tamanho real de mensagem.** Pesquisado (2026-11-08): desktop anuncia `max-message-size` 65536 por padrão (`rtc-0.21.1/.../setting_engine.rs:281`), teto 262144; envio usa o menor entre local e remoto; acima disso `send` falha com `ErrOutboundPacketTooLarge` (sem fragmentar). Consequência: limite efetivo 65536 bytes nos dois sentidos. Frames de 16 KiB + cabeçalho: seguros. JSON: o limite "64 KiB" significa **mensagem UTF-8 inteira ≤ 65536 bytes**. Backpressure: `set_buffered_amount_low_threshold` + `OnBufferedAmountLow` existem no Rust; no RN 155.0.1 o evento usa `<` estrito (limiar 0 nunca dispara) e `bufferedAmount` atrasa após `send` — fixar limiar > 0 (ex. 256 KiB) e checar `bufferedAmount` após cada `send`. Pendente em aparelho: `max-message-size` anunciado pelo mobile e código nativo Android/iOS.

## 10. Desvios

**D1–D8 aprovados em 2026-11-08 (answers 010)** — D1 individualmente, D2–D8 em bloco. **D9–D10 aprovados em 2026-11-08 (answers 012).**

- **D1:** novo controle `session_detach`. O PLANO lista só create/list/attach/close; sem ele, trocar de sessão exigiria fechar o PTY ou a conexão.
- **D2:** respostas nomeadas `<pedido>_ok`, correlacionadas por `request_id`. O PLANO não define a forma das respostas.
- **D3:** `sequence` tem significado por opcode: offset de bytes na saída, seq de input na entrada, 0 no resize, id no heartbeat. Isso unifica o "offset monotônico" do PLANO com o campo do frame.
- **D4:** opcode `05` significa só `session_exited`. `command_completed` vira controle de texto. Novos controles: `session_exited` e `command_completed` (o PLANO pede a distinção, mas não lista os tipos).
- **D5:** o heartbeat é por conexão, com `session_id` zerado.
- **D6:** `error` com `request_id` não fatal e sem `request_id` fatal. A C2 continua com todo erro fatal no handshake.
- **D7:** offsets, seq e tempos no JSON como string decimal (o PLANO aplica isso explicitamente só a C4).
- **D8:** controles anteriores aos frames ganham `connection_id`, mesmo formato da C2.
- **D9:** nova mensagem `screen_snapshot`, enviada após `replay_gap` e antes de `replay_begin` (answers 007, "guardar e reexibir").
- **D10:** novo pedido `client_focus` `{state:"foreground"|"background"}` para o host ajustar o envio ao vivo / a cada 5 s / nenhum (answers 007, seção 9 item 8).

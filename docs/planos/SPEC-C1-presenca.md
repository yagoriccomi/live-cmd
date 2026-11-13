# SPEC C1 — Adendo: presença do host para reconexão

**Status: PROPOSTA (2026-11-13) — adendo à C1, local-only; deploy segue vetado; aprovação do usuário pendente.**

Base: `PLANO-live-cmd.md` §7 C1 ("host mantém registro de presença para reconexão", "Prever reconexão/presença após hibernação") e `SPEC-C2-identidade.md` §1 e §5. Implementação: `signaling/worker.ts`, `signaling/room.ts`; testes em `signaling/signaling.test.ts`.

## 1. Problema

Hoje a sala de sinalização serve uma vez só. O Worker grava `consumed` no Durable Object e responde 409 a qualquer reuso. `room_id` e `connection_id` vêm fixos das variáveis `ROOM_ID`/`CONNECTION_ID`. O host faz uma conexão e encerra. Um celular já pareado não tem como reencontrar o PC sem novo QR, e C2 §5 exige `connection_id` novo a cada reconexão.

## 2. Decisão

- O `session` do QR de pareamento (C2 §1) vira a **sala do host**: permanente, fixa por host. O celular a guarda junto com `host_pk` e `server` depois do pareamento.
- A sala pode ser reutilizada: uma tentativa por vez, no máximo um host e um cliente.
- O cliente (offerer) sorteia um `connection_id` novo (UUID v4) a cada tentativa e o anuncia no seu `hello`. O host o recebe no `hello {status:"ready"}`.
- Local-only: o Worker só aceita a sala configurada em `ROOM_ID` (var do `wrangler dev`). `CONNECTION_ID` deixa de existir. Uma sala por instância local basta para um host e mantém os testes exatos. Em produção futura, escolher outra regra (fora deste adendo).

## 3. Fluxo

1. Host abre `ws://127.0.0.1:{port}/v1/ws/{room_id}` e envia `hello` com `connection_id: null`.
2. Worker responde `hello {status:"waiting"}` com `connection_id: null`. Host ocioso espera sem prazo.
3. Cliente abre o mesmo WS e envia `hello` com o seu `connection_id`.
4. Worker fixa esse `connection_id` para a tentativa e envia `hello {status:"ready"}` aos dois, com o `connection_id` do cliente.
5. Daí em diante, todo envelope (offer, answer, ice, close) precisa trazer esse `connection_id`; caso contrário, `protocol_error`.
6. A tentativa termina com `close` (`done`, `timeout`, `peer_left`, `protocol_error`). O Worker fecha os dois sockets e zera o `connection_id`.
7. O host abre um WS novo na mesma sala e volta ao passo 1.

A ordem host/cliente pode se inverter: cliente primeiro recebe `waiting`; o `ready` só sai quando os dois papéis estão presentes.

## 4. Mensagens alteradas (JSON exato)

Host → Worker:
```json
{"v":1,"type":"hello","connection_id":null,"request_id":"host","payload":{"role":"host"}}
```
Worker → host (aguardando):
```json
{"v":1,"type":"hello","connection_id":null,"request_id":"host","payload":{"status":"waiting"}}
```
Cliente → Worker (`connection_id` = UUID v4 minúsculo, novo a cada tentativa):
```json
{"v":1,"type":"hello","connection_id":"<uuid v4>","request_id":"<id>","payload":{"role":"mobile"}}
```
Worker → ambos (o `request_id` é o do `hello` que completou o par):
```json
{"v":1,"type":"hello","connection_id":"<uuid do cliente>","request_id":"<id>","payload":{"status":"ready"}}
```
Papel já ocupado (segundo host ou segundo cliente), só para o recém-chegado; WS fechado com código 1013, motivo `busy`:
```json
{"v":1,"type":"error","connection_id":null,"request_id":"<id>","payload":{"code":"LIMIT_EXCEEDED","message":"Room busy"}}
{"v":1,"type":"close","connection_id":null,"request_id":"<id>","payload":{"reason":"busy"}}
```
Terceiro WS com a sala cheia (dois sockets): HTTP 409 `Room busy`, sem upgrade.

`busy` só existe no sentido Worker → peer. Peers continuam enviando `close` apenas com `done|timeout|peer_left|protocol_error`.

## 5. Limites

- Mantidos: envelope ≤ 64 KiB, 64 mensagens/s por peer, ICE ≤ 4 KiB, 256 candidatos por tentativa, 20 conexões/IP/min, negociação 30 s e teto de 300 s por tentativa.
- O relógio da tentativa reinicia quando entra o segundo socket. Assim, a espera do host ocioso não consome os 30 s do cliente.
- Socket sozinho que não seja host identificado expira em 30 s.
- Estado só em memória. Se o DO for despejado ou reiniciado, a tentativa cai e os peers reconectam. Hibernação de WebSocket fica para quando houver exposição pública.

## 6. Segurança

- Reuso é aceitável porque `room_id` não autoriza nada (C2 §1). A autoridade é o handshake C2 fim a fim: `mode="pair"` exige OTK ativa; `mode="reconnect"` exige `client_pk` na trust store do host e `host_pk` na do cliente.
- O uso único da OTK é garantido pelo host (CAS em memória, C2 §5), não pela sala. Por isso o marcador `consumed` sai sem perda de garantia.
- Replay de offer antiga: o `connection_id` antigo não bate com a tentativa nova. A mensagem vira `protocol_error` e nunca chega ao novo peer.
- O `connection_id` é escolhido pelo cliente e não tem valor de segredo. Entra no transcript C2 (16 bytes), e o host compara com o que recebeu no `ready`.
- Quem conhece a sala pode ocupar o lugar do cliente e causar negação de serviço por até 30 s por tentativa. Mitigação: rate limit por IP. Risco aceito em local-only.
- Continuam: só hostname de loopback, sem header `Origin`, sem query string, sala UUID v4 minúscula, e o Worker nunca recebe nem registra chaves, OTK ou URI.

## 7. Casos negativos (cobertos por testes)

| Caso | Resultado |
|---|---|
| `hello` de host com `connection_id` não nulo | `protocol_error` |
| `hello` de cliente com `connection_id` nulo ou fora do formato UUID v4 minúsculo | `protocol_error` |
| Envelope pós-`hello` com `connection_id` diferente | `protocol_error`, fecha os dois |
| Offer da tentativa anterior reenviada | `protocol_error`, não é repassada |
| Segundo host ou segundo cliente | `busy` só para ele; tentativa atual segue |
| Terceiro socket | HTTP 409 |
| Outra sala, host não loopback, `Origin`, query, sem `ROOM_ID` | 404 / 403 / 403 / 404 / 503 |

## 8. O que o host (D5a) implementa

- Sala do host = `session` do QR, persistida junto com a identidade do host (o QR de pareamento reutiliza essa sala).
- Laço: conectar, enviar `hello` com `connection_id: null`, aguardar `ready`, ler `connection_id` do `ready`, negociar e autenticar (C2) com esse id; ao fim, reconectar à mesma sala.
- Validar o envelope do `ready`: UUID v4 minúsculo. A partir daí, exigir esse id em todo envelope recebido.
- Ao receber `close` (qualquer motivo) ou perder o WS, reconectar com backoff (sugestão: 1 s, dobrando até 30 s; zera após `ready`). Ao receber `busy`, também esperar com backoff antes de voltar.
- Uma PeerConnection por tentativa (C2 §5). Nada de estado C2 sobrevive entre tentativas.
- Testes com Wrangler: remover `CONNECTION_ID` de `desktop/scripts/d1-integration.cjs` e dos testes de `main.rs`; o cliente de teste gera o id.

## 9. Desvios do PLANO

- P1: "Sala aleatória, efêmera" passa a ser "sala aleatória, permanente por host; tentativas efêmeras". O efêmero passa a ser a tentativa (`connection_id`).
- P2: "Registro de presença" é o próprio host conectado na sua sala. O Worker não guarda registro persistente.
- P3: o motivo `busy` e o código 1013 são novos e só saem do Worker para o recém-chegado. `LIMIT_EXCEEDED` reusa a lista de C2 §2.2.
- P4: presença após hibernação fica fora. Despejo do DO derruba a tentativa e o host reconecta (§5).

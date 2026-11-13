# SPEC C1 — Adendo: presença do host para reconexão

**Status: PROPOSTA (2026-11-13) — adendo à C1, local-only; deploy segue vetado; aprovação do usuário pendente. Revisão S2 (2026-11-13): clientes simultâneos (answers 018 item 2, P14).**

Base: `PLANO-live-cmd.md` §7 C1 ("host mantém registro de presença para reconexão", "Prever reconexão/presença após hibernação") e `SPEC-C2-identidade.md` §1 e §5. Implementação: `signaling/worker.ts`, `signaling/room.ts`; testes em `signaling/signaling.test.ts`.

## 1. Problema

Hoje a sala de sinalização serve uma vez só. O Worker grava `consumed` no Durable Object e responde 409 a qualquer reuso. `room_id` e `connection_id` vêm fixos das variáveis `ROOM_ID`/`CONNECTION_ID`. O host faz uma conexão e encerra. Um celular já pareado não tem como reencontrar o PC sem novo QR, e C2 §5 exige `connection_id` novo a cada reconexão.

## 2. Decisão

- O `session` do QR de pareamento (C2 §1) vira a **sala do host**: permanente, fixa por host. O celular a guarda junto com `host_pk` e `server` depois do pareamento.
- A sala tem **um host** e até **4 clientes simultâneos** (`MAX_CLIENTS`, igual ao limite de anexos da C3 §6). Cada cliente negocia sua própria tentativa, em paralelo com as outras. Só os comandos do terminal são serializados, pelo `seq` da C3 §4; a sinalização não serializa nada.
- O cliente (offerer) sorteia um `connection_id` novo (UUID v4) a cada tentativa e o anuncia no seu `hello`. O host recebe um `hello {status:"ready"}` por cliente, cada um com o `connection_id` daquele cliente.
- O `connection_id` é a chave de roteamento: cada envelope do host vai só para o cliente dono daquele id; cada envelope de cliente chega ao host com o id do cliente.
- O WS do host **permanece** entre tentativas. O fim de uma tentativa fecha só o socket daquele cliente.
- Local-only: o Worker só aceita a sala configurada em `ROOM_ID` (var do `wrangler dev`). `CONNECTION_ID` deixa de existir. Em produção futura, escolher outra regra (fora deste adendo).

### 2.1 O que mudou na revisão S2

| Antes (D5a/S1) | Agora (S2) |
|---|---|
| Uma tentativa por vez: um host e um cliente | Um host e até 4 clientes negociando ao mesmo tempo |
| Segundo cliente recebia `busy` | Segundo, terceiro e quarto cliente recebem `ready`; só o quinto recebe `busy` |
| Fim da tentativa fechava os dois sockets; o host reabria o WS | Fim da tentativa fecha só o cliente; o host recebe `close` com o id dele e o WS do host continua |
| Erro de protocolo fechava os dois | Erro fecha só a tentativa culpada; envelope inválido do host para um cliente derruba só aquele cliente |
| `connection_id` único na sala, zerado no fim | `connection_id` por cliente; a sala não guarda id global |
| Terceiro socket: HTTP 409 | Socket acima de 6 (host + 4 clientes + 1 de folga para ouvir `busy`): HTTP 409 |

`busy`/1013 deixa de ser o caminho normal: só sai para um segundo host ou para um quinto cliente.

## 3. Fluxo

1. Host abre `ws://127.0.0.1:{port}/v1/ws/{room_id}` e envia `hello` com `connection_id: null`.
2. Sem cliente, o Worker responde `hello {status:"waiting"}` com `connection_id: null`. Host ocioso espera sem prazo.
3. Cliente abre o mesmo WS e envia `hello` com o seu `connection_id`.
4. Com host presente, o Worker envia `hello {status:"ready"}` ao cliente e ao host, com o `connection_id` do cliente. Sem host, o cliente recebe `waiting`; quando o host chega, cada cliente à espera recebe o seu `ready`, e o relógio de 30 s desse cliente reinicia.
5. Daí em diante, todo envelope (offer, answer, ice, close) do cliente traz o seu `connection_id`. Todo envelope do host traz o `connection_id` do cliente a que se destina. Cliente com id diferente do seu: `protocol_error` só para ele.
6. A tentativa termina com `close` (`done`, `timeout`, `peer_left`, `protocol_error`), enviado pelo cliente, pelo host (com o id do cliente) ou pelo Worker. O Worker fecha só o socket daquele cliente e avisa o host com `close` e o id do cliente.
7. Envelope do host para um id que já terminou é descartado em silêncio. Nunca é repassado a outro cliente.
8. Se o WS do host cair, todos os clientes recebem `close {reason:"peer_left"}` e são fechados. Conexões WebRTC já estabelecidas não dependem do WS e continuam. O host reabre o WS e volta ao passo 1.

## 4. Mensagens alteradas (JSON exato)

Host → Worker:
```json
{"v":1,"type":"hello","connection_id":null,"request_id":"host","payload":{"role":"host"}}
```
Worker → host (aguardando):
```json
{"v":1,"type":"hello","connection_id":null,"request_id":"host","payload":{"status":"waiting"}}
```
O `waiting` para um cliente sem host leva o `connection_id` dele.

Cliente → Worker (`connection_id` = UUID v4 minúsculo, novo a cada tentativa):
```json
{"v":1,"type":"hello","connection_id":"<uuid v4>","request_id":"<id>","payload":{"role":"mobile"}}
```
Worker → cliente e host (o `request_id` é o do `hello` que completou o par):
```json
{"v":1,"type":"hello","connection_id":"<uuid do cliente>","request_id":"<id>","payload":{"status":"ready"}}
```
Fim de uma tentativa, Worker → host:
```json
{"v":1,"type":"close","connection_id":"<uuid do cliente>","request_id":"<id>","payload":{"reason":"done"}}
```
Segundo host, ou cliente acima de 4, só para o recém-chegado. WS fechado com código 1013, motivo `busy`:
```json
{"v":1,"type":"error","connection_id":null,"request_id":"<id>","payload":{"code":"LIMIT_EXCEEDED","message":"Room busy"}}
{"v":1,"type":"close","connection_id":null,"request_id":"<id>","payload":{"reason":"busy"}}
```
Socket acima de 6 com a sala cheia: HTTP 409 `Room busy`, sem upgrade.

`busy` só existe no sentido Worker → peer. Peers continuam enviando `close` apenas com `done|timeout|peer_left|protocol_error`.

## 5. Limites

- Mantidos por tentativa: envelope ≤ 64 KiB, ICE ≤ 4 KiB, 256 candidatos, negociação 30 s e teto de 300 s. 20 conexões/IP/min.
- Taxa: 64 mensagens/s por cliente. O host, que negocia para todos, tem 64 × 4 = 256/s.
- O relógio de cada tentativa começa no socket do cliente e reinicia quando o host chega. Host ocioso espera sem prazo.
- Socket que não se identifica expira em 30 s.
- Estado só em memória. Se o DO for despejado ou reiniciado, as tentativas caem e os peers reconectam. Hibernação de WebSocket fica para quando houver exposição pública.

## 6. Segurança

- Reuso é aceitável porque `room_id` não autoriza nada (C2 §1). A autoridade é o handshake C2 fim a fim, um por tentativa: `mode="pair"` exige OTK ativa; `mode="reconnect"` exige `client_pk` na trust store do host e `host_pk` na do cliente.
- O uso único da OTK é garantido pelo host (CAS em memória, C2 §5), não pela sala. Dois clientes simultâneos com a mesma OTK: só um pareia.
- Replay de offer antiga: o `connection_id` antigo não pertence ao socket novo. A mensagem vira `protocol_error` e nunca chega ao host.
- `connection_id` repetido de um cliente vivo: `protocol_error` para o recém-chegado; o dono segue.
- O `connection_id` é escolhido pelo cliente e não tem valor de segredo. Entra no transcript C2 (16 bytes), e o host compara com o que recebeu no `ready`.
- Isolamento: o host nunca recebe envelope de cliente sem o id daquele cliente; um cliente nunca recebe envelope de outro id.
- Quem conhece a sala pode ocupar as 4 vagas por até 30 s por tentativa (negação de serviço). Mitigação: rate limit por IP. Risco aceito em local-only.
- Continuam: só hostname de loopback, sem header `Origin`, sem query string, sala UUID v4 minúscula, e o Worker nunca recebe nem registra chaves, OTK ou URI.

## 7. Casos negativos (cobertos por testes)

| Caso | Resultado |
|---|---|
| `hello` de host com `connection_id` não nulo | `protocol_error` para o host |
| `hello` de cliente com `connection_id` nulo ou fora do formato UUID v4 minúsculo | `protocol_error`, só ele |
| Envelope de cliente com `connection_id` diferente do seu | `protocol_error`, só ele; host avisado com `close` |
| Envelope inválido do host para um cliente | `protocol_error` só na tentativa daquele cliente |
| Envelope do host para id já encerrado | descartado |
| Offer da tentativa anterior reenviada | `protocol_error`, não é repassada |
| `connection_id` de cliente vivo reusado | `protocol_error` para o recém-chegado |
| Segundo host, ou quinto cliente | `busy` só para ele; tentativas atuais seguem |
| Sétimo socket | HTTP 409 |
| Queda do WS do host | todos os clientes recebem `peer_left` |
| Outra sala, host não loopback, `Origin`, query, sem `ROOM_ID` | 404 / 403 / 403 / 404 / 503 |

## 8. O que o host implementa

- Sala do host = `session` do QR, persistida junto com a identidade do host (o QR de pareamento reutiliza essa sala).
- Laço: conectar, enviar `hello` com `connection_id: null` e ficar no WS. Cada `ready` válido (UUID v4 minúsculo, id ainda não em uso) abre uma tarefa própria: PeerConnection nova e handshake C2 independente (C2 §5). Nenhum estado C2 sobrevive entre tentativas nem é compartilhado entre elas.
- Envelopes recebidos vão só para a tarefa do seu `connection_id`. Id desconhecido ou encerrado é descartado. A saída de todas as tarefas passa por um único escritor no WS.
- Ao terminar, a tarefa envia `close` com o seu id (`done` ou `protocol_error`) para o Worker liberar a vaga.
- Uma tarefa que cai não derruba as outras nem o WS. Se o WS cair, as conexões WebRTC autenticadas continuam. O host reconecta com backoff (1 s, dobrando até 30 s; zera após `ready`). Ao receber `busy` (outro host na sala), também espera com backoff.
- Testes com Wrangler: o cliente de teste gera o id.

## 9. Desvios do PLANO

- P1: "Sala aleatória, efêmera" passa a ser "sala aleatória, permanente por host; tentativas efêmeras". O efêmero passa a ser a tentativa (`connection_id`).
- P2: "Registro de presença" é o próprio host conectado na sua sala. O Worker não guarda registro persistente.
- P3: o motivo `busy` e o código 1013 são novos e só saem do Worker para o recém-chegado. `LIMIT_EXCEEDED` reusa a lista de C2 §2.2.
- P4: presença após hibernação fica fora. Despejo do DO derruba as tentativas e o host reconecta (§5).
- P5 (S2): clientes simultâneos (answers 018 item 2). O limite de 4 vem da C3 §6; mudar um exige mudar o outro.

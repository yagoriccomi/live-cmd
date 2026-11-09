# SPEC C4 — Arquivos e retomada

**Status: APROVADA em 2026-11-08 (answers 012) — implementação só com liberação explícita; aceite de M2 exige testes em Android e iOS reais.**
Decisões aplicadas: answers 008, 009 e 011 (desvios D1–D16 aprovados).

Base: `PLANO-live-cmd.md` seção 7 (C4); `SPEC-C2-identidade.md`; `SPEC-C3-controle.md`; tickets 003, 006, 007, 008 e 009; answers 001, 002 e 003. Esta spec detalha C4 sem alterar o PLANO; o que muda está em "Desvios" (seção 12). Vocabulário: `CONTEXT.md`.

Decisões de produto já tomadas e usadas aqui:
- Upload e download de arquivos entram no MVP (answers 001).
- Chunks de 64 KiB, retomada automática e expurgo de parciais após 24 h (answers 002).
- O destino padrão é o cwd da sessão, com navegação livre pelo disco (answers 003).

**Códigos de erro:** a fonte única é a seção 2.2 de `SPEC-C2-identidade.md`. O único código novo, `ALREADY_EXISTS` (D4), já está lá.

**Convenções.** Valem as da C3:
- Inteiros binários em big-endian.
- UUID v4 minúsculo no JSON e 16 bytes no frame.
- u64 no JSON como string decimal, regex `^(0|[1-9][0-9]{0,19})$`.
- Exceção: índices de chunk são u32 e vão no JSON como número inteiro (cabem em 2^53; desvio D8). Fora de `0..2^32−1` ou não inteiro → `INVALID_MESSAGE`.

Termos:
- **Remetente:** quem tem os bytes. No upload é o cliente; no download, o host.
- **Destinatário:** o outro lado.
- **Intervalo:** `[início, fim)`, fim exclusivo, em índices de chunk.

## 1. Canal e pré-condições

- `file-channel`: confiável, ordenado, sem `maxRetransmits`/`maxPacketLifeTime`, criado pelo offerer (C3 seção 1).
- Texto = controles JSON C4 (formato C3 seção 2, limite ≤ 65536 bytes da mensagem UTF-8, com `v`, `type` e `connection_id`). Binário = frames de chunk (seção 3).
- Qualquer mensagem no `file-channel` antes de AUTHENTICATED (C2) → CLOSED.
- Regra de erro fatal ou não fatal: a da C3 seção 2, com mais dois casos: `error` com `file_id` e sem `request_id` é não fatal e afeta só aquela transferência (D3); `error` com `batch_id` (sem `file_id` e sem `request_id`) é não fatal e afeta só aquele lote (D16). `INVALID_MESSAGE` continua sempre fatal.

## 2. Controles

### 2.1 Pedidos do cliente → respostas do host

| Pedido | Campos | Resposta OK | Erros não fatais |
|---|---|---|---|
| `fs_list` | `path` (string \| `null`), `cursor?` | `fs_list_ok`: `path`, `entries[]`, `next_cursor` (\| `null`) | `NOT_FOUND`, `UNAUTHORIZED`, `IO_ERROR` |
| `file_upload_init` | `file_id`, `session_id` (\| `null`), `filename`, `target_dir` (\| `null`), `size_bytes`, `chunk_size` (=65536), `total_chunks`, `sha256`, `overwrite` (bool) | `file_upload_init_ok`: `file_id`, `missing` (seção 2.4) | `ALREADY_EXISTS`, `NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`, `UNAUTHORIZED` |
| `file_download_init` | `file_id`, `path` | `file_download_init_ok`: `file_id`, `filename`, `size_bytes`, `chunk_size`, `total_chunks`, `sha256` | `NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`, `UNAUTHORIZED` |
| `file_get` | `file_id`, `ranges` (≤ 64) | — (o host passa a enviar os chunks pedidos) | `NOT_FOUND`, `LIMIT_EXCEEDED` |
| `file_resume` | `file_id`, `cursor?` | `file_resume_ok`: `file_id`, `direction`, `filename`, `size_bytes`, `total_chunks`, `sha256`, `missing` | `NOT_FOUND` |
| `file_cancel` | `file_id` | `file_cancel_ok` | `NOT_FOUND` |

- `file_id` é gerado pelo cliente (UUID v4). Se já existir no host → `INVALID_MESSAGE`, fatal.
- `fs_list` com `path=null`:
  - Com `session_id` e cwd conhecido: lista o cwd.
  - Sem isso: lista as raízes (unidades locais no Windows; `/` e `$HOME` no Unix).
- `entries[]`: `{name, kind:"file"|"dir"|"link"|"other", size_bytes?, mtime_ms, hidden: bool}`. Ocultos são sempre listados; o filtro é do app (seção 9 item 5).
- Paginação: até 500 entradas ou 48 KiB por página. `cursor` é opaco, emitido pelo host e válido por 60 s.

### 2.2 Mensagens espontâneas

| type | Remetente | Campos | Quando |
|---|---|---|---|
| `file_ack` | destinatário | `file_id`, `ranges` (≤ 64) | Chunks gravados de forma durável (seção 5). É idempotente. |
| `file_complete` | quem finalizou | `file_id` | Hash total conferido e arquivo no destino final (upload: host; download: cliente). |
| `error` | qualquer | `code`, `file_id` | Falha só desta transferência (desvio D3). |

### 2.3 Validação de `file_upload_init`

- `size_bytes`: string decimal, menor ou igual ao limite da seção 7.
- `total_chunks = ceil(size_bytes / 65536)`. Divergente → `INVALID_MESSAGE`.
- `chunk_size` ≠ 65536 → `INVALID_MESSAGE`.
- Arquivo vazio: `total_chunks = 0` e `sha256 = e3b0c442…b855`. O host finaliza já no init: envia `file_upload_init_ok` com `missing=[]` e, em seguida, `file_complete`.
- `sha256`: 64 caracteres hexadecimais minúsculos.
- `filename` e `target_dir`: seção 6.
- `target_dir = null`: usa o cwd da sessão `session_id`. Se o cwd for desconhecido (sem OSC 7, C3 seção 9 item 6) ou `session_id = null` → `NOT_FOUND`. O app pede a pasta e lembra a última por host (seção 9 item 8).
- Se o destino já existir e `overwrite = false` → `ALREADY_EXISTS` já no init. A verificação se repete na finalização, por causa de corridas.
- Espaço livre (seção 7): o destino precisa ter `size_bytes` + 1 GiB; o staging precisa continuar com pelo menos 5 GiB livres após reservar `size_bytes`. Senão → `LIMIT_EXCEEDED`.

### 2.4 `missing` e paginação

- `missing`: `{ranges: [[início, fim), …], next_cursor}`. No máximo 1024 intervalos por mensagem.
- Se houver mais, `next_cursor` não é nulo e o cliente pede o resto com `file_resume` e `cursor`.
- Nunca vai uma lista sem limite (PLANO).

## 3. Frame de chunk (binário)

```
version:u8 (=1) | file_id:16 | chunk_index:u32 | frag_offset:u32 | chunk_len:u32 | chunk_sha256:32 | payload_len:u32 | data
```

O cabeçalho tem 65 bytes. O chunk lógico continua com 64 KiB; no transporte ele **sempre** vai em fragmentos de até 16 384 bytes (desvio D1):
- O maior frame tem 16 449 bytes, então cabe em qualquer `max-message-size` ≥ 64 KiB exigido pela C3.
- O transporte nunca assume que 65 536 bytes mais o cabeçalho cabem numa mensagem.

Regras:
- `chunk_len = 65536`, exceto no último chunk, que tem o tamanho exato do resto (`size − 65536 × (total − 1)`).
- `frag_offset` é múltiplo de 16 384 e menor que `chunk_len`.
- `payload_len = min(16384, chunk_len − frag_offset)` e precisa ser igual ao tamanho do frame menos 65.
- Os fragmentos de um chunk chegam em sequência e em ordem, sem fragmento de outro chunk do mesmo `file_id` no meio.
- Fragmentos de arquivos diferentes podem se intercalar.
- O reassembly fica limitado a um chunk (64 KiB) por transferência ativa.
- Violação de qualquer regra acima, `chunk_index ≥ total_chunks`, `file_id` desconhecido nesta conexão ou chunk fora do que foi pedido ou da janela → `INVALID_MESSAGE` (fatal).
- `version` desconhecida → `VERSION_UNSUPPORTED` (fatal).
- `chunk_sha256` se repete em todos os fragmentos; se divergir entre fragmentos → `INVALID_MESSAGE`.
- Após o reassembly, o destinatário calcula o SHA-256 do chunk:
  - Divergente: descarta o chunk, não confirma e envia `error HASH_MISMATCH` com `file_id`. O remetente pode reenviar **uma** vez; a segunda falha no mesmo índice cancela a transferência (staging preservado; seção 8).
- Chunk já confirmado chegando de novo:
  - O destinatário relê o trecho gravado e compara o hash.
  - Igual: reenvia o ACK (idempotente).
  - Diferente: `HASH_MISMATCH` e a transferência é cancelada (PLANO: "duplicata com conteúdo divergente falha").

## 4. Fluxos

**Upload (push):**
1. `file_upload_init` → `file_upload_init_ok` com `missing`.
2. O cliente envia os chunks de `missing` dentro da janela (seção 7).
3. O host confirma com `file_ack`.
4. Quando não falta nenhum chunk, o host finaliza (seção 5) e envia `file_complete` ou `error` (`HASH_MISMATCH`, `ALREADY_EXISTS`, `IO_ERROR`) com `file_id`.

**Download (pull; desvio D2):**
1. `file_download_init`:
   - O host abre o arquivo e exige arquivo regular (`kind=file`; diretório ou dispositivo → `NOT_FOUND`).
   - Calcula `sha256` e grava um manifesto de download `{file_id, owner, path, size, mtime, identidade do arquivo, sha256}`.
2. O cliente pede intervalos com `file_get`, dentro da janela. O host envia os chunks.
3. O cliente grava, confirma, verifica o hash total, move para o destino local e envia `file_complete`. O host então apaga o manifesto.
4. Se, antes de enviar um chunk, o host perceber que o arquivo mudou (tamanho, mtime ou identidade) → `error NOT_FOUND` com `file_id`. A versão iniciada não existe mais; o cliente descarta o parcial e avisa o usuário.

**Resume após reconexão:**
- Nova PeerConnection e handshake C2 `reconnect`. O cliente envia `file_resume` para cada transferência pendente.
- No upload, o host responde com o que falta, lido do mapa persistido.
- No download, o cliente sabe o que tem e só faz `file_get` dos intervalos que faltam.
- Chunks enviados e não confirmados antes da queda são tratados como ausentes.
- O resume exige o mesmo `client_pk` dono da transferência. Outro dispositivo, `file_id` desconhecido ou transferência expirada → `NOT_FOUND` igual nos três casos (desvio D5; não revela existência).
- Reinício do host não perde uploads: manifesto, mapa e dados ficam em disco (PLANO). Downloads também retomam se o arquivo de origem não mudou.

## 5. Durabilidade, staging e finalização (host)

**Staging:** `<data_dir>/staging/<file_id>/`, onde `data_dir` é o diretório de dados do app, nunca o cwd. Contém:
- `manifest.json`: `schema_version`, `file_id`, `owner_client_pk`, `direction`, `filename`, `target_dir` canônico, `overwrite`, `size_bytes`, `total_chunks`, `sha256`, `created_at`, `last_valid_chunk_at` (relógio de parede, que sobrevive a reinício).
- `bitmap`: um bit por chunk.
- `data.part`: pré-alocado com `size_bytes`; cada chunk é gravado no offset `índice × 65536`.

**ACK (group commit):**
1. O host grava os chunks recebidos.
2. Faz `fsync` de `data.part`.
3. Atualiza o `bitmap` de forma atômica (temporário + `fsync` + rename).
4. Só então envia `file_ack` com os intervalos.

Um lote junta no máximo 16 chunks ou 100 ms (desvio D9). O ACK nunca vem antes da gravação durável (PLANO).

**Finalização do upload:**
1. Recalcula o SHA-256 de `data.part` inteiro. Divergente → `HASH_MISMATCH` e o staging é descartado: os chunks bateram, então o hash declarado ou a origem estavam errados e retomar não resolve.
2. Abre `target_dir` pelo handle guardado (seção 6) e confere de novo que é o mesmo diretório.
3. Se o staging estiver em outro volume, copia para `<target_dir>/.<filename>.livecmd-<file_id>.part`, criado com criação exclusiva, e faz `fsync`. No mesmo volume, usa `data.part` direto.
4. Rename:
   - `overwrite = false`: rename **sem substituir** (Windows: `MoveFileExW` sem `REPLACE_EXISTING`; Unix: `renameat2(RENAME_NOREPLACE)` ou `link` + `unlink`). Se o destino existir → `ALREADY_EXISTS`; o parcial fica preservado e pode ser retomado (seção 9, item 2).
   - `overwrite = true`: substituição atômica (`MoveFileExW` com `REPLACE_EXISTING` ou `rename`). O original fica intacto até o rename.
5. `fsync` do diretório (Unix), apaga o staging e envia `file_complete`.

- Disco cheio ou falha de I/O em qualquer passo → `IO_ERROR`. O original é preservado, o staging continua retomável e o chunk em questão não é confirmado.
- Na inicialização, o host varre o staging:
  - `manifest.json` ilegível ou com `schema_version` desconhecida → pasta descartada (log saneado).
  - Temporários `.livecmd-*.part` órfãos dentro de `target_dir` são removidos só se estiverem listados num manifesto.

**Expurgo:**
- Ocorre 24 h após `last_valid_chunk_at`, com relógio injetável nos testes.
- Restrito a `<data_dir>/staging/`.
- A remoção nunca segue links nem junctions; um link encontrado é removido como link.
- Manifestos de download expiram no mesmo prazo.

## 6. Segurança de caminhos (host)

"Navegação livre" (answers 003) significa as permissões da conta do usuário do host, nunca privilégio administrativo (PLANO). O host roda sem elevação. Regras:

**`filename`** — um único componente. Rejeitar (`INVALID_MESSAGE`):
- vazio, `.` ou `..`;
- `/`, `\`, `:` (inclusive stream alternativo `x:stream`), NUL e caracteres de controle `0x00–0x1F`;
- no Windows também: `< > " | ? *`, ponto ou espaço no final;
- nome reservado do Windows, sem diferenciar maiúsculas e com ou sem extensão: `CON`, `PRN`, `AUX`, `NUL`, `COM1–COM9`, `LPT1–LPT9`, `COM¹²³`/`LPT¹²³`, `CONIN$`, `CONOUT$` (vale também no Unix, para o arquivo ser portável);
- mais de 255 unidades UTF-16;
- UTF-8 inválido.

O nome nunca é corrigido em silêncio.

**`target_dir`/`path`** — absoluto, até 4096 bytes. Rejeitar:
- caminho relativo;
- componentes `.` e `..`;
- namespaces de dispositivo `\\.\` e `\\?\` (inclusive `\\?\UNC\`).

Caminhos de rede (UNC `\\servidor\compartilhamento\…` e unidades mapeadas) são aceitos com as regras extras da seção 8B (answers 008).

Canonicalização:
- O host abre o diretório **uma vez** e guarda o handle.
- Todo acesso posterior é relativo a esse handle (`openat` / `NtCreateFile` com `RootDirectory`), sem concatenar strings.
- Links e junctions nos ancestrais escolhidos pelo usuário são seguidos (navegação livre); o caminho canônico resolvido vai para o manifesto.

Arquivo final e temporários:
- Criados com criação exclusiva (`O_CREAT|O_EXCL|O_NOFOLLOW`; `CREATE_NEW` + `FILE_FLAG_OPEN_REPARSE_POINT`).
- Se o nome final já existir como link ou junction: tratado como existente (`ALREADY_EXISTS`); com `overwrite`, substitui-se o link, nunca o alvo.

Corridas:
- Se o diretório for trocado entre o init e a finalização (identidade de arquivo/volume diferente) → `IO_ERROR`, staging preservado.
- `fs_list` também abre por handle e reporta links como `kind:"link"`, sem segui-los para tamanho.

**Download** — a origem deve ser um arquivo regular após resolver links. Dispositivos, pipes e diretórios → `NOT_FOUND`. A leitura usa o handle aberto no init.

Logs: nunca registrar caminhos, nomes de arquivo nem conteúdo (PLANO, "Dados, configuração e privacidade"); só `file_id`, estado e código.

## 7. Limites e backpressure

| Limite | Valor proposto | Excesso |
|---|---|---|
| Tamanho por arquivo | 64 GiB, configurável (seção 9 item 1) | `LIMIT_EXCEEDED` no init |
| Transferências ativas por conexão / por host | 4 / 8 | `LIMIT_EXCEEDED` no init |
| Staging total no host | 100 GiB, configurável (seção 9 item 1) | `LIMIT_EXCEEDED` no init |
| Margem de espaço livre no destino | 1 GiB além de `size_bytes` | `LIMIT_EXCEEDED` |
| Piso de espaço livre no volume do staging | nunca menos de 5 GiB livres após reservar `size_bytes` | `LIMIT_EXCEEDED` |
| Janela por transferência | 32 chunks (2 MiB) enviados e não confirmados | chunk além da janela → `INVALID_MESSAGE` |
| `file_get` pendente por transferência | 32 chunks | `LIMIT_EXCEEDED` não fatal |
| Controles por conexão | os da C3 (50/s, rajada de 100), contados juntos com o `pty-channel` | fatal |
| `bufferedAmount` do `file-channel` (remetente) | pausa acima de 512 KiB; retoma no evento `bufferedamountlow` com `bufferedAmountLowThreshold = 128 KiB` | — |
| Fila de gravação no host | 8 chunks por transferência | o host para de ler o canal; o SCTP aplica backpressure |

**Convivência com o `pty-channel`:**
- O terminal tem prioridade.
- Antes de cada fragmento, o remetente verifica se há frame PTY pendente do mesmo lado; se houver, cede a vez.
- O `bufferedAmount` do `file-channel` fica abaixo de metade do limite do PTY (1 MiB na C3).
- No host, a gravação em disco roda em pool separado e nunca bloqueia a leitura do PTY nem a do `pty-channel`.
- O teste C3 N9 mede o resultado.

**Mobile:**
- O upload calcula o SHA-256 do arquivo inteiro antes do init. O arquivo de origem é reaberto a cada retomada e o tamanho e o mtime são conferidos; se mudarem, a transferência é cancelada localmente e o usuário é avisado.
- O download grava em staging privado do app e expõe o arquivo ao usuário só após o hash conferir, na pasta do app e depois via "Compartilhar/Salvar em…" (seção 9 item 6).

## 8. Cancelamento e revogação

- `file_cancel`, pelo dono ou pelo host: apaga o staging ou o manifesto e descarta a reassembly. Chunks em trânsito daquele `file_id` passam a ser ignorados, e depois de 5 s sem tráfego viram `INVALID_MESSAGE`.
- Duas falhas de hash no mesmo chunk (seção 3) cancelam a transferência, mas preservam o staging para resume.
- Revogação (C2 seção 6, C3 seção 7), de forma síncrona com o fechamento das conexões:
  1. Cancela as transferências ativas do `client_pk`.
  2. Impede `file_resume`; o dispositivo revogado nem chega a AUTHENTICATED (`REVOKED`).
  3. Apaga na hora o staging e os manifestos do dispositivo revogado (seção 9 item 4).
- Fechar a PeerConnection não cancela: a transferência fica pendente para resume até o expurgo.

## 8A. Pastas inteiras (answers 008)

**Modelo:** um **lote** (`batch_id`, UUID v4 do cliente) com manifesto de entradas por caminho relativo. Cada arquivo do lote é uma transferência comum das seções 2–5 (mesmo chunk, frame, hash, ACK, staging e resume), com `file_id` próprio. Nada é compactado (seção 11). Pastas vazias são criadas explicitamente.

**Manifesto** (`entries[]`, paginado como `missing`, até 1000 entradas ou 48 KiB por mensagem):
`{rel_path, kind:"file"|"dir", size_bytes?, sha256?, file_id?}` — `file_id`/`size_bytes`/`sha256` só em `kind:"file"`.

**Controles novos (desvio D12):**

| Pedido | Campos | Resposta OK | Erros não fatais |
|---|---|---|---|
| `batch_upload_init` | `batch_id`, `session_id` (\| `null`), `root_name`, `target_dir` (\| `null`), `total_files`, `total_bytes`, `entries[]`, `more` (bool) | `batch_upload_init_ok`: `batch_id`, `conflicts[]` | `ALREADY_EXISTS`, `NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`, `UNAUTHORIZED` |
| `batch_entries` | `batch_id`, `entries[]`, `more` | `batch_entries_ok` | `LIMIT_EXCEEDED` (entrada inválida → `INVALID_MESSAGE`, fatal) |
| `batch_download_init` | `batch_id`, `path` (diretório) | `batch_download_init_ok`: `batch_id`, `root_name`, `total_files`, `total_bytes`, `entries[]`, `next_cursor`, `skipped` | `NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`, `UNAUTHORIZED` |
| `batch_resolve` | `batch_id`, `root?` (`"merge"`\|`"rename"`, obrigatório se houve conflito de raiz), `decisions[]`: `{rel_path, action:"overwrite"\|"rename"\|"skip"}` ou `apply_all` (`"overwrite"`\|`"rename"`\|`"skip"`) | `batch_resolve_ok` | `NOT_FOUND` |
| `batch_status` | `batch_id`, `cursor?` | `batch_status_ok`: `done`, `pending[]` (`file_id`s), `skipped[]`, `failed[]`, `next_cursor` | `NOT_FOUND` |
| `batch_cancel` | `batch_id` | `batch_cancel_ok` | `NOT_FOUND` |

Upload: após `batch_upload_init_ok` (e `batch_resolve`, se houver conflitos), o cliente usa `file_upload_init` por arquivo com o campo extra `batch_id` + `rel_path` (sem `filename`/`target_dir`). Download: o cliente usa `file_download_init` com `batch_id` + `rel_path`. Fim do lote: `batch_complete` espontâneo (`batch_id`, contagens de concluídos/pulados/falhos) de quem finalizou o último arquivo.

**Validação de `rel_path`** (no host, upload e download; no cliente, download):
- separador só `/`; cada componente segue todas as regras de `filename` (seção 6) — portanto sem `..`, `.`, vazio, `\`, `:`, reservados Windows, ponto/espaço final;
- nunca absoluto, sem letra de unidade, sem `//` inicial; até 4096 bytes no total;
- colisão que difere só em maiúsculas/minúsculas ou normalização Unicode (NFC) dentro do lote → `INVALID_MESSAGE` no init (o destino Windows não distingue);
- todo diretório-pai de um `file` precisa existir no manifesto como `dir`; `rel_path` duplicado → `INVALID_MESSAGE`.

**Links, junctions e especiais dentro da pasta:** não seguidos. No download, o host percorre com handles relativos sem seguir reparse points/symlinks; links, dispositivos, pipes e entradas sem permissão de leitura vão para `skipped` (contagem e `rel_path`, mostrado ao usuário), nunca são transferidos. No upload, o cliente não envia links (o app lista os pulados). A raiz escolhida pelo usuário pode ser link (como na seção 6).

**Limites** (configuráveis no host; excesso → `LIMIT_EXCEEDED` no init, antes de transferir qualquer byte):

| Limite | Valor proposto |
|---|---|
| Entradas por lote | 100 000 |
| Profundidade | 64 níveis |
| Tamanho total | 64 GiB (mesmo teto do arquivo) e dentro da cota de staging de 100 GiB |
| Lotes ativos por conexão / por host | 1 / 2 (contam também nas transferências da seção 7) |
| Arquivos simultâneos dentro do lote | 4 |

**Conflitos** (decisão: perguntar; answers 008):
- No upload, o host compara o manifesto com o destino no init e devolve `conflicts[]` (arquivo existente, ou `dir` sobre arquivo e vice-versa). O app pergunta por arquivo, com **"aplicar a todos"** (sobrescrever / renomear / pular; seção 9 item 10). `rename` usa o sufixo "nome (1).ext" decidido no host.
- Se o próprio diretório raiz já existir: conflito de raiz em `conflicts[]`; o app pergunta entre mesclar (pré-selecionado; depois pergunta por arquivo) e criar "nome (1)", enviado em `batch_resolve.root` (seção 9 item 11).
- No download, o mesmo vale no aparelho, dentro da pasta do app.
- Conflito que surge depois do init (corrida) é tratado na finalização daquele arquivo, como na seção 5.

**Atomicidade:** **por arquivo, nunca da pasta inteira.** Cada arquivo segue temp + rename da seção 5; em cancelamento, falha ou queda, os arquivos já concluídos ficam no destino e nenhum parcial aparece com o nome final. Não há rollback do lote. O app mostra "pasta incompleta" com a lista de pendentes/falhos.

**Retomada parcial:** o host persiste `<data_dir>/staging/batch-<batch_id>/manifest.json` (entradas, decisões de conflito, estado por arquivo). Após reconexão, o cliente pede `batch_status` e continua só os pendentes (cada um com `file_resume`). Falha de um arquivo (`HASH_MISMATCH`, `IO_ERROR`) não para os outros; vai para `failed[]` e o usuário pode tentar de novo só esses. Expurgo do lote: 24 h sem chunk válido em nenhum arquivo dele. Revogação apaga o lote inteiro na hora (seção 9 item 4).

**Progresso:** bytes e arquivos concluídos sobre `total_bytes`/`total_files`, calculados no cliente a partir de `file_ack`/`file_complete`; sem mensagem extra de progresso.

**Hash no mobile (upload):** o SHA-256 de cada arquivo é calculado antes do `file_upload_init` daquele arquivo, não do lote inteiro de uma vez (evita minutos de espera antes do primeiro byte). O manifesto inicial pode vir sem `sha256`; ele é obrigatório no `file_upload_init` (desvio D13).

## 8B. Pastas de rede (answers 008)

UNC (`\\servidor\compartilhamento\…`) e unidades mapeadas são aceitas em `fs_list`, `target_dir`, `path` e lotes.

- **Credenciais:** as da conta do usuário do host, na sessão em que o host roda. O live-cmd não pede, guarda nem transmite senhas de compartilhamento. Sem acesso → `UNAUTHORIZED` não fatal. Unidade mapeada só existe na sessão do usuário: se o host rodar como serviço, ela não aparece (documentar; host roda na sessão do usuário — PLANO).
- **Staging sempre em volume local:** `<data_dir>/staging/` nunca fica em rede. O upload sempre usa o caminho "outro volume" da seção 5.
- **Finalização no destino de rede:**
  1. Copia para `<target_dir>/.<filename>.livecmd-<file_id>.part` com criação exclusiva.
  2. `FlushFileBuffers`; fecha.
  3. **Relê o temporário pela rede e confere o SHA-256** (o `fsync` remoto não é garantido pelo SMB).
  4. Rename no mesmo compartilhamento (`MoveFileExW`, sem ou com `REPLACE_EXISTING`, como na seção 5). A atomicidade depende do servidor; o host não promete mais que o servidor oferece.
  5. Divergência no passo 3 → `HASH_MISMATCH`, temporário removido, staging local preservado para nova tentativa.
- **Download de rede:** o host lê para o envio direto do compartilhamento, mas confere tamanho/mtime/identidade antes de cada chunk (seção 4). Mudou → `NOT_FOUND` como hoje.
- **Timeouts:** cada operação de rede (abrir, ler um chunk, gravar, listar uma página, rename) tem prazo de 30 s (seção 9 item 12). Estourou → `EXPIRED` não fatal com `file_id` (ou `request_id` no `fs_list`); a transferência fica pausada e retomável. A operação bloqueada roda no pool de disco e nunca bloqueia o PTY (seção 7).
- **Compartilhamento desconectado** no meio: a transferência pausa (`IO_ERROR` não fatal), o staging local é preservado; o usuário retoma depois. Upload continua recebendo chunks no staging local mesmo com o destino fora; só a finalização espera.
- **Canonicalização:** `\\servidor\compartilhamento` vira a raiz do handle; `..` acima dela continua proibido. Unidade mapeada é resolvida para o UNC no manifesto (para retomada mesmo se a letra mudar). Ataque de troca de diretório por link no servidor: mesma regra de identidade da seção 6 (identidade = volume serial + file ID quando o servidor fornecer; sem identidade estável → aceito, conferindo caminho + tamanho + mtime; limitação registrada, seção 9 item 13).
- **Espaço livre:** `GetDiskFreeSpaceExW` no compartilhamento; se o servidor não informar, segue sem a checagem prévia e depende de `IO_ERROR` na cópia.
- **Logs:** nunca registrar nomes de servidor/compartilhamento (são caminhos).

## 9. Itens em aberto (decisão do usuário)

Itens 1–8 decididos em answers 008 (2026-11-08):

1. ~~Tamanho máximo e cota.~~ **Decidido:** 64 GiB por arquivo; staging total até 100 GiB, nunca deixando menos de 5 GiB livres; configuráveis no host (`LIMIT_EXCEEDED`).
2. ~~Nome já existe.~~ **Decidido:** perguntar (sobrescrever, renomear ou cancelar); parcial preservado enquanto decide. Usa `ALREADY_EXISTS` (D4, aprovado em answers 011).
3. ~~Pastas de rede.~~ **Decidido:** entram na v1 (UNC e unidades mapeadas). Aberto (técnico): timeouts de rede, `fsync`/rename atômico em SMB não garantidos — staging sempre em volume local e cópia final verificada por hash; lista de bloqueio opcional e vazia por padrão.
4. ~~Staging de revogado.~~ **Decidido:** apagar na hora.
5. ~~Ocultos.~~ **Decidido:** mostrar, com campo `hidden` e filtro no app.
6. ~~Destino no celular.~~ **Decidido:** pasta do app, depois "Compartilhar/Salvar em…".
7. ~~Pastas inteiras.~~ **Decidido:** entram na v1 (upload e download de diretório). Detalhado na seção 8A (manifesto sem compactar, atomicidade por arquivo, links não seguidos).
8. ~~cwd desconhecido.~~ **Decidido:** o app pede a pasta e lembra a última por host.
9. **Pesquisa técnica (não é decisão de produto):** confirmar `set_buffered_amount_low_threshold`/`on_buffered_amount_low` em `webrtc =0.21.1` e o evento `bufferedamountlow` no `react-native-webrtc 155.0.1`; e o custo do `fsync` por lote em disco real (meta: 100 MB retomável, PLANO M4.2).
Itens 10–14 decididos em answers 009 (2026-11-08):

10. ~~"Aplicar a todos".~~ **Decidido:** oferecido (sobrescrever / renomear / pular todos).
11. ~~Pasta raiz já existe.~~ **Decidido:** perguntar entre mesclar (pré-selecionado) e criar "nome (1)".
12. ~~Prazo em rede.~~ **Decidido:** 30 s por operação; pausa retomável.
13. ~~NAS sem file ID.~~ **Decidido:** aceitar, conferindo caminho + tamanho + mtime; limitação registrada.
14. ~~Limites de pasta.~~ **Decidido:** 100 000 entradas, 64 níveis, 64 GiB, 1 lote ativo por conexão; configuráveis no host.

## 10. Matriz de aceite

| # | Caso | Esperado |
|---|---|---|
| A1 | Upload de 100 MB; Wi-Fi desligado no meio e religado | retoma do primeiro chunk ausente; hash confere; `file_complete` |
| A2 | Download de 100 MB com a mesma queda | idem, pelo cliente |
| A3 | Reinício do host no meio do upload | `file_resume` devolve o que falta segundo o bitmap persistido |
| A4 | Arquivo vazio; arquivo de 65 536 bytes; arquivo de 65 537 bytes | 0 / 1 / 2 chunks; último chunk com o tamanho exato |
| A5 | Staging em volume diferente do destino | temporário no volume de destino; rename atômico |
| A6 | Overwrite com consentimento | original intacto até o rename; substituído depois |
| A7 | Digitação durante upload a toda velocidade | critério C3 N9 |
| A8 | Expurgo com relógio controlado | parcial removido aos 24 h, não antes; chunk válido zera a contagem |
| A9 | `missing` com mais de 1024 intervalos | paginado por `cursor` |
| N1 | Mensagem no `file-channel` antes de AUTHENTICATED | CLOSED |
| N2 | `filename` com `..`, `a/b`, `a\b`, `C:x`, `x:stream`, `CON`, `con.txt`, `COM1`, `LPT¹`, espaço ou ponto final, `\x01`, 256 caracteres | `INVALID_MESSAGE` |
| N3 | `target_dir` relativo, com `..`, `\\.\`, `\\?\` ou `\\?\UNC\` | rejeitado (UNC comum é aceito; seção 8B) |
| N4 | Destino final pré-existente como symlink/junction apontando para fora | `ALREADY_EXISTS`; o alvo não é tocado |
| N5 | Diretório de destino trocado por junction entre o init e a finalização | `IO_ERROR`; nada gravado fora |
| N6 | `chunk_sha256` errado | `HASH_MISMATCH`, sem ACK; a segunda falha cancela |
| N7 | Duplicata de chunk confirmado com conteúdo diferente | `HASH_MISMATCH`, transferência cancelada |
| N8 | Hash total errado com chunks corretos | `HASH_MISMATCH`; nada no destino |
| N9 | `total_chunks` inconsistente; `chunk_size` ≠ 65536; `size_bytes` como número JSON | `INVALID_MESSAGE` |
| N10 | `chunk_index ≥ total`; fragmento fora de ordem; chunk além da janela | `INVALID_MESSAGE`, fatal |
| N11 | Resume por outro dispositivo; `file_id` inexistente; transferência expirada | `NOT_FOUND`, igual nos três |
| N12 | Revogação durante upload | conexão fechada; transferência cancelada; resume impossível |
| N13 | Disco cheio no meio | `IO_ERROR`; original preservado; parcial retomável após liberar espaço |
| N14 | 5ª transferência na conexão; arquivo acima do limite | `LIMIT_EXCEEDED` |
| N15 | Arquivo de origem alterado durante o download | `NOT_FOUND` com `file_id`; o cliente descarta o parcial |
| N16 | Logs após A1–N15 e A10–N30 | nenhum caminho, nome, servidor, compartilhamento ou conteúdo de arquivo |
| A10 | Upload de pasta com 3 níveis, 200 arquivos e 2 pastas vazias | estrutura idêntica no destino; hashes conferem; `batch_complete` |
| A11 | Download de pasta para o aparelho | idem, na pasta do app |
| A12 | Queda de rede no meio do lote; reconexão | `batch_status` lista pendentes; só eles são retomados; concluídos intactos |
| A13 | Reinício do host no meio do lote | manifesto do lote persistido; retomada como A12 |
| A14 | Cancelamento do lote no meio | concluídos ficam no destino; nenhum `.livecmd-*.part` nem nome final parcial; app mostra "pasta incompleta" |
| A15 | 10 conflitos com "aplicar a todos: renomear" | 10 arquivos "nome (1).ext"; originais intactos |
| A16 | Upload para `\\servidor\compartilhamento\x` e para unidade mapeada | staging local; temporário relido e hash confere; rename; `file_complete` |
| A17 | Download de arquivo em compartilhamento | igual ao local |
| N17 | `rel_path` com `..`, `/abs`, `C:/x`, `//x`, `a\b`, `a//b`, componente `CON` ou com espaço final | `INVALID_MESSAGE`; nenhum byte gravado |
| N18 | Lote com `A.txt` e `a.txt`; ou `é` NFC e NFD | `INVALID_MESSAGE` no init |
| N19 | Arquivo sem `dir` pai no manifesto; `rel_path` duplicado | `INVALID_MESSAGE` |
| N20 | Download de pasta com symlink/junction apontando para fora, pipe e arquivo sem permissão | não seguidos nem transferidos; listados em `skipped` |
| N21 | Lote com 100 001 entradas; profundidade 65; total acima de 64 GiB | `LIMIT_EXCEEDED` antes de qualquer chunk |
| N22 | Falha de hash em 1 arquivo do lote | só ele em `failed[]`; os demais concluem |
| N23 | Revogação durante lote | lote inteiro cancelado e staging apagado |
| N24 | Compartilhamento desconectado durante upload | pausa com `IO_ERROR` não fatal; staging local preservado; retomada após reconectar conclui |
| N25 | Servidor de rede lento além do prazo | `EXPIRED` não fatal; PTY continua responsivo (C3 N9) |
| N26 | Conteúdo do temporário de rede alterado antes da conferência | `HASH_MISMATCH`; nada com o nome final |
| N27 | Compartilhamento sem acesso para a conta do host | `UNAUTHORIZED` não fatal; nenhuma credencial pedida ou registrada |
| N28 | Upload de pasta cuja raiz já existe no destino | conflito de raiz em `conflicts[]`; `root:"merge"` mescla e pergunta por arquivo; `root:"rename"` cria "nome (1)"; `batch_resolve` sem `root` → `INVALID_MESSAGE` |
| N29 | Destino em NAS sem file ID; arquivo de origem trocado (mesmo caminho, tamanho ou mtime diferente) | troca detectada por caminho + tamanho + mtime; `NOT_FOUND`/`IO_ERROR` como na seção 4/6 |
| N30 | `fs_list` em pasta com arquivos ocultos | entradas com `hidden:true` listadas; o filtro do app as esconde/mostra |

## 11. Fora do escopo

Sincronização de pastas, deduplicação entre arquivos, compressão e transferência entre dois clientes sem passar pelo host.

## 12. Desvios

**D1–D16 aprovados em 2026-11-08 (answers 011)** — D4 e D5 individualmente (já aplicados na C2 seção 2.2), demais em bloco.

- **D1:** o chunk lógico de 64 KiB vai **sempre** em fragmentos de 16 KiB, com os campos `frag_offset` e `chunk_len` no frame (cabeçalho de 65 bytes). O PLANO só fragmenta se não couber; fragmentar sempre deixa o caminho único e testável.
- **D2:** `file_init` foi dividido em `file_upload_init` (push) e `file_download_init` + `file_get` (pull). O PLANO tem um `file_init` com `direction`, mas no download o cliente não sabe tamanho nem hash.
- **D3:** `error` com `file_id` e sem `request_id` é não fatal e afeta só aquela transferência. Estende a regra da C3.
- **D4:** novo código `ALREADY_EXISTS` na C2 seção 2.2: "destino já existe e não houve consentimento para sobrescrever"; emissor host; C4; mensagem "Já existe um arquivo com esse nome.". Aplicado na C2.
- **D5:** arquivo de outro dispositivo → `NOT_FOUND`, como as sessões na C3. Texto de `UNAUTHORIZED`/`NOT_FOUND` ajustado na C2 seção 2.2.
- **D6:** o hash por chunk não é persistido. Duplicatas são conferidas relendo o trecho do disco, o que evita 32 MiB de manifesto por arquivo de 64 GiB.
- **D7:** novo controle `fs_list` para o seletor de navegação livre (answers 003). O PLANO não lista controles de navegação.
- **D8:** índices e intervalos de chunk como número JSON (u32 cabe em 2^53); `size_bytes` como string decimal (PLANO).
- **D9:** ACK por intervalos com group commit (até 16 chunks ou 100 ms), em vez do ACK por chunk do ticket 007. Continua valendo "ACK só após gravação durável".
- **D10:** novo controle `file_cancel`.
- **D11:** o `buffered_amount` do `file_ack` do ticket 007 foi removido. O backpressure é local (`bufferedAmount`) mais a janela.
- **D12:** novos controles de lote: `batch_upload_init`, `batch_entries`, `batch_download_init`, `batch_resolve`, `batch_status`, `batch_cancel` e o espontâneo `batch_complete` (seção 8A). O PLANO e o ticket 007 só tratam arquivo único.
- **D13:** `file_upload_init` e `file_download_init` aceitam `batch_id` + `rel_path` no lugar de `filename`/`target_dir`/`path`, quando o arquivo pertence a um lote.
- **D14:** caminhos de rede (UNC e unidades mapeadas) passam a ser aceitos (seção 8B); a versão anterior desta spec os rejeitava.
- **D15:** `EXPIRED` passa a ser usado como erro não fatal de prazo de operação de rede, com `file_id` ou `request_id`. A tabela da C2 seção 2.2 já lista "outros prazos de C3/C4"; só o caso fica registrado.
- **D16:** `error` com `batch_id` (sem `file_id` e sem `request_id`) é não fatal e afeta só aquele lote. Estende D3.

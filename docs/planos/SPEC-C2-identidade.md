# SPEC C2 — Identidade vinculada ao transporte

**Status: APROVADA em 2026-11-08 (answers 012) — implementação só com liberação explícita; aceite de M2 exige testes em Android e iOS reais.**
Decisões aplicadas: `answers/2026_11_07 gerenciador-terminais-remoto 004.md`.

Base: `PLANO-live-cmd.md` seção 7 (C1, C2, C3). Esta spec detalha C2 sem alterá-lo; mudanças ficam em "Desvios" (seção 9). Vocabulário: `CONTEXT.md`.

Convenções: `LP(x)` = `u32 BE len(x) || x`. `||` = concatenação. Strings em UTF-8 sem terminador. Base64url = RFC 4648 §5, sem padding. Papéis fixos: **offerer = Cliente Mobile** (ou cliente desktop), **answerer = Host**.

## 1. URI do QR

```
remoteterm://pair?v=1&session=<uuid>&otk=<b64url 32 bytes>&host_pk=<b64url 32 bytes>&server=<host DNS>
```

| Campo | Regra |
|---|---|
| `v` | `1`. Outro valor: cliente recusa, não conecta e avisa `VERSION_UNSUPPORTED` (seção 2.2). |
| `session` | UUID v4 minúsculo; vira o `room_id` de C1. Não autoriza nada. |
| `otk` | 32 bytes CSPRNG; base64url, exatamente 43 chars. |
| `host_pk` | Chave pública Ed25519 do host, 32 bytes, 43 chars. Cliente fixa (pin) este valor. |
| `server` | Apenas hostname[:porta]; cliente só aceita se estiver na allowlist fixa embutida no app (não configurável em tempo de execução; trocar de servidor exige atualizar o app). Fora da lista: recusa, sem conectar. WSS obrigatório fora de loopback. |

- Parâmetro desconhecido, duplicado ou ausente: recusa. Tamanho máximo do URI: 512 bytes.
- Validade: 300 s medidos pelo host com relógio monotônico desde a emissão. O QR não carrega timestamp; o cliente não decide expiração.
- Gerar um novo QR invalida a OTK anterior (no máximo uma OTK ativa por host).
- **Nunca** vão para logs, analytics, crash reports, Worker ou SDP: o URI inteiro, `otk`, chaves privadas, nonces, MACs, assinaturas. Para o Worker vão apenas `session` (como room_id) e o envelope C1. Logs podem conter `connection_id`, estado e código de erro.

## 2. Handshake de controle no `pty-channel`

Mensagens de texto JSON `{v:1, type, connection_id, ...}` (C3), limite ≤ 65536 bytes da mensagem UTF-8. Binários de 32 bytes em base64url. `connection_id` = o da tentativa C1, no mesmo formato UUID do envelope C1; no transcript entra como 16 bytes binários. Divergente do observado localmente → fecha.

Pré-condição de ambos os lados: `pty-channel` aberto **e** DTLS `connected`, fingerprints obtidos conforme seção 4. Qualquer mensagem no `file-channel`, frame binário ou controle não-auth antes de AUTHENTICATED → CLOSED.

| # | type | Remetente | Campos além de v/type/connection_id |
|---|---|---|---|
| 1 | `auth_hello` | offerer | `mode` (`"pair"`\|`"reconnect"`), `role:"offerer"`, `client_pk`, `nonce_o` |
| 2 | `auth_challenge` | answerer | `role:"answerer"`, `host_pk`, `nonce_a`, `sig_a`, `mac_a` (só em `pair`) |
| 3 | `auth_proof` | offerer | `sig_o`, `mac_o` (só em `pair`) |
| 4 | `auth_ok` | answerer | — |
| * | `error` | qualquer | `code` (seção 2.2); emissor fecha em seguida |

Fluxo:
1. Offerer envia `auth_hello`. Host valida formato, versão, papel, `mode`; em `reconnect` exige `client_pk` na trust store e não revogado; em `pair` exige OTK ativa e não expirada.
2. Host gera `nonce_a`, monta o transcript `T` (seção 3), assina/MACa, envia `auth_challenge`. Estado → CHALLENGED.
3. Offerer verifica `host_pk == pin` (QR em `pair`, trust store em `reconnect`), `role`, `sig_a` e, em `pair`, `mac_a`. Falha → `error` + fecha. Sucesso → envia `auth_proof`.
4. Host verifica `sig_o` e `mac_o`; em `pair` consome a OTK atomicamente (seção 5) e grava `client_pk` na trust store; envia `auth_ok`. Estado → AUTHENTICATED. Offerer só passa a AUTHENTICATED ao receber `auth_ok`.

Fingerprints **não** trafegam nas mensagens: cada lado coloca no `T` o que observou no próprio transporte. Divergência aparece como falha de assinatura.

### 2.1 Estados (por PeerConnection, nos dois lados)

`NEW` → (`auth_challenge` enviado/recebido) → `CHALLENGED` → (`auth_ok`) → `AUTHENTICATED`; qualquer erro, timeout ou mensagem fora de ordem → `CLOSED` (fecha PeerConnection inteira, não só o canal). Timeout: 30 s monotônicos desde DTLS `connected` até AUTHENTICATED. Sem retransmissão nem segundo desafio na mesma conexão; reinício do host descarta tentativas pendentes. Toda reconexão = nova PeerConnection, novo `connection_id`, novos nonces.

### 2.2 Erros estáveis do app (lista única)

**Esta tabela é a fonte única de códigos de erro do live-cmd inteiro.** C3 e C4 (e qualquer spec futura) devem referenciá-la, não redefinir códigos; código novo só entra aqui, com aprovação. Inclui os 8 códigos de C3 (`PLANO-live-cmd.md` seção 7), mais 2 necessários para autenticação e `ALREADY_EXISTS` de C4.

| code | Significado | Emissor | Módulos | Exibido ao usuário | Mensagem sugerida |
|---|---|---|---|---|---|
| `INVALID_MESSAGE` | JSON/campo/tamanho inválido, ordem errada, `role`/`mode` inválido, `connection_id` divergente, comprimento inesperado no transcript; também QR malformado (parâmetro desconhecido/duplicado/ausente, URI > 512 bytes) | ambos | C2, C3, C4 | sim | "Mensagem inválida. Tente conectar de novo." (QR: "QR Code inválido.") |
| `VERSION_UNSUPPORTED` | `v` desconhecido no QR ou nas mensagens de controle | ambos | C2, C3, C4 | sim | "Versão incompatível. Atualize o app no celular e no computador." |
| `UNAUTHORIZED` | assinatura/MAC inválido, chave desconhecida, `host_pk` ≠ pin, OTK consumida ou desconhecida, `nonce_o` repetido, fingerprint indisponível ou divergente, CAS da OTK perdido | ambos | C2, C3, C4 | sim | "Não autorizado. Pareie novamente lendo um novo QR Code." (C4, pasta sem permissão: "Sem permissão para acessar esta pasta.") |
| `REVOKED` | `client_pk` revogado (tombstone) | host | C2, C3 | sim | "Este aparelho foi removido pelo computador. Pareie novamente." |
| `EXPIRED` | OTK expirada (300 s); timeout de autenticação (30 s); outros prazos de C3/C4 | ambos | C2, C3, C4 | sim | "Tempo esgotado. Gere um novo QR Code ou tente de novo." |
| `LIMIT_EXCEEDED` | excesso de handshakes simultâneos/rate limit; limites de C3/C4 (tamanho, sessões, buffer) | host | C2, C3, C4 | sim | "Limite atingido. Aguarde e tente novamente." |
| `NOT_FOUND` | sessão, arquivo ou recurso inexistente; também transferência de outro dispositivo (não revela existência; C4 D5) | host | C3, C4 | sim | "Não encontrado." |
| `ALREADY_EXISTS` | destino já existe e não houve consentimento para sobrescrever (C4 D4, aprovado em answers 011) | host | C4 | sim | "Já existe um arquivo com esse nome." |
| `HASH_MISMATCH` | hash do arquivo transferido não confere | ambos | C4 | sim | "Arquivo corrompido na transferência. Tente de novo." |
| `IO_ERROR` | falha de disco/PTY/cofre no lado local, inclusive gravação da trust store após o CAS (seção 5) | ambos | C2, C3, C4 | sim | "Erro ao gravar ou ler dados no aparelho." |
| `SERVER_NOT_ALLOWED` | `server` do QR fora da allowlist embutida (seção 1); só local, nunca trafega | cliente | C2 | sim | "Servidor não reconhecido. Atualize o app ou use um QR válido." |

Separações avaliadas e recusadas (ficam em `UNAUTHORIZED`): fingerprint indisponível × divergente, OTK consumida × desconhecida, nonce repetido, pin divergente. Distingui-las no fio diria a um atacante qual verificação falhou ou se uma OTK existe; nenhuma muda a ação do usuário (parear de novo). `VERSION_UNSUPPORTED` foi separado porque a ação é outra (atualizar) e a versão não é segredo.

O motivo detalhado fica só em log local saneado; o peer recebe apenas o código.

## 3. Transcript canônico

```
T = LP("live-cmd/auth/v1")
 || LP(0x01)                    versão, 1 byte
 || LP(mode)                    "pair" | "reconnect"
 || LP("offerer")               papel do offerer
 || LP("answerer")              papel do answerer
 || LP(connection_id)           16 bytes
 || LP(room_id)                 16 bytes (UUID binário da sala C1)
 || LP(host_pk)                 32 bytes
 || LP(client_pk)               32 bytes
 || LP(nonce_o)                 32 bytes CSPRNG do offerer
 || LP(nonce_a)                 32 bytes CSPRNG do answerer
 || LP(fp_offerer)              32 bytes SHA-256 do DER do cert DTLS do offerer
 || LP(fp_answerer)             32 bytes SHA-256 do DER do cert DTLS do answerer
```

Ordem dos fingerprints é **por papel**, nunca local/remoto: o host usa `fp_offerer = remoto`, `fp_answerer = local`; o mobile usa `fp_offerer = local`, `fp_answerer = remoto`. Comprimento inesperado em qualquer campo de tamanho fixo → `INVALID_MESSAGE`.

Provas (rótulo separado por papel evita reflexão):

```
sig_a = Ed25519.sign(host_sk,   LP("sig/answerer") || T)
sig_o = Ed25519.sign(client_sk, LP("sig/offerer")  || T)
mac_a = HMAC-SHA256(OTK, LP("mac/answerer") || T)      só em pair
mac_o = HMAC-SHA256(OTK, LP("mac/offerer")  || T)      só em pair
```

Comparação de MAC em tempo constante. Ed25519 estrito (RFC 8032, rejeitar pontos não canônicos/pequena ordem).

### 3.1 Vetor de teste (estrutural)

Entradas: `mode="pair"`, `connection_id=00 01 … 0f`, `room_id=10 11 … 1f`, `host_pk=20×32`, `client_pk=21×32`, `nonce_o=30×32`, `nonce_a=31×32`, `fp_offerer=40×32`, `fp_answerer=41×32`, `OTK=50×32`, chaves Ed25519 de semente fixa `60×32` (host) e `61×32` (cliente).

Bytes esperados de `T` (312 bytes):
```
00000010 6c6976652d636d642f617574682f7631     "live-cmd/auth/v1"
00000001 01
00000004 70616972                              "pair"
00000007 6f666665726572                        "offerer"
00000008 616e737765726572                      "answerer"
00000010 000102…0f
00000010 101112…1f
00000020 2020…20
00000020 2121…21
00000020 3030…30
00000020 3131…31
00000020 4040…40
00000020 4141…41
```
(20+5+8+11+12+20+20+6×36 = 312 bytes.) Saídas `sig_*`/`mac_*` serão geradas uma vez pela implementação Rust, gravadas como fixture e verificadas pelo mobile. Negativos derivados: trocar `fp_offerer`↔`fp_answerer`, trocar rótulos `sig/`, `mode="reconnect"` com MAC presente — todos devem falhar.

## 4. Obtenção dos fingerprints

Só após DTLS `connected`; antes disso → aguardar até o timeout de 30 s, nunca usar valores do SDP.

**Desktop (`webrtc =0.21.1`, fixado):**
- Remoto: `DtlsTransport::get_remote_certificates()`; `fp = SHA-256(certs[0])`. Vazio → `UNAUTHORIZED`.
- Local: certificado gerado pelo host e passado na configuração; `RTCCertificate::get_fingerprints`, entrada `sha-256`, hex → 32 bytes.
- `with_disable_certificate_fingerprint_verification` proibido (teste de grep no CI). Upgrade da crate exige nova revisão desta seção.

**Mobile (`react-native-webrtc 155.0.1`, fixado):**
- Gerar certificado com `generateCertificate` e passá-lo em `certificates`; guardar o fingerprint esperado.
- Após `connectionState==='connected'`: `getStats()`, localizar a entrada `transport` ativa, seguir `localCertificateId` e `remoteCertificateId`. Usar a entrada `certificate` referenciada (folha); ignorar `issuerCertificateId`.
- Para cada uma: `fingerprintAlgorithm` deve ser `sha-256`; decodificar `base64Certificate` (DER), calcular SHA-256 localmente e exigir igualdade com o campo `fingerprint`. Usar o valor recalculado.
- Local deve ser igual ao fingerprint do certificado gerado (detecta fallback silencioso do Android para o certificado padrão). Divergência, IDs ausentes ou mais de um transport ativo → `UNAUTHORIZED`.
- Upgrade de `react-native-webrtc` exige nova revisão desta seção.

## 5. OTK, concorrência, nonces, reconexão

- OTK em memória do host apenas (nunca em disco): `{otk, issued_at_mono, state: active|consumed}`. Expira quando `now_mono - issued_at_mono ≥ 300 s`; checagem no `auth_hello` e de novo no `auth_proof`.
- Consumo: compare-and-swap `active → consumed` sob lock, **depois** de `sig_o` e `mac_o` válidos e **antes** de gravar a trust store e enviar `auth_ok`. Perdeu o CAS → `UNAUTHORIZED`. Prova inválida não consome a OTK.
- Concorrência: até 4 handshakes `pair` simultâneos (excedente → `LIMIT_EXCEEDED`); só um completa. Falha na gravação da trust store após o CAS: OTK permanece consumida, usuário gera novo QR.
- Nonces: cada lado gera o seu por tentativa e nunca reutiliza. Host mantém cache limitado (ex. 4096 entradas, LRU, vida do processo) de `nonce_o` vistos; repetição → `UNAUTHORIZED`.
- Reconexão (`mode="reconnect"`): sem OTK nem MAC; exige `client_pk` na trust store do host e `host_pk` na do cliente. Mesmo transcript, novos nonces e `connection_id`.
- Rejeições sempre fecham a PeerConnection; não há "retry" dentro dela.

## 6. Trust store e revogação

- Host: chave privada Ed25519 no cofre nativo (Windows DPAPI/Credential Manager, macOS Keychain, Linux Secret Service). Arquivo da trust store com `schema_version`, entradas `{client_pk, label, paired_at, revoked_at?}`, escrita atômica (temp + rename). Revogado vira tombstone para responder `REVOKED`.
- Mobile: semente de `client_sk` em `expo-secure-store` (Keychain iOS / cifrada por chave AES do Keystore Android); lista de hosts `{host_pk, server, label}`. Nunca chave privada em JSON em claro. Risco aceito (answers 005): Keystore/Secure Enclave não suportam Ed25519, então a semente é decifrada para a memória ao assinar; root/jailbreak pode extraí-la, mitigado por revogação no host. Desinstalar o app perde a chave (parear de novo).
- Revogar (host): marca tombstone, fecha PeerConnections daquele `client_pk`, cancela PTY attach e transferências, impede resume. Biometria só desbloqueia localmente.

## 7. Matriz de aceite

| # | Caso | Esperado | Dispositivo real |
|---|---|---|---|
| A1 | Pareamento feliz | AUTHENTICATED, trust store gravada | Android + iOS |
| A2 | Reconexão feliz após troca de rede | AUTHENTICATED, novo connection_id | Android + iOS |
| A3 | Vetor 3.1 Rust ↔ JS | bytes e provas idênticos | não |
| N1 | Troca local/remoto dos fingerprints num lado | `UNAUTHORIZED` | Android + iOS |
| N2 | SDP com `a=fingerprint` divergente do certificado | DTLS falha, sem `connected` | Android + iOS |
| N3 | Certificado gerado ignorado (fallback Android) | `UNAUTHORIZED` | Android |
| N4 | OTK expirada (relógio monotônico controlado) | `EXPIRED` | não |
| N5 | OTK reutilizada após pareamento | `UNAUTHORIZED` | não |
| N6 | `nonce_o` repetido | `UNAUTHORIZED` | não |
| N7 | `role` ou `mode` divergentes | `INVALID_MESSAGE` | não |
| N7b | `v` desconhecido no QR ou na mensagem | `VERSION_UNSUPPORTED` | não |
| N8 | `host_pk` ≠ pin | cliente fecha com `UNAUTHORIZED` | não |
| N9 | Dois clientes com o mesmo QR, provas simultâneas | exatamente um AUTHENTICATED | não |
| N10 | Revogação durante sessão ativa | conexão fechada, PTY/transfer cancelados, reconexão `REVOKED` | Android ou iOS |
| N11 | Mensagem no `file-channel` ou frame PTY antes de auth | CLOSED | não |
| N12 | Sem `auth_proof` em 30 s | `EXPIRED`, CLOSED | não |
| N13 | Assinatura com rótulo do outro papel (reflexão) | `UNAUTHORIZED` | não |
| N14 | Reinício do host com handshake pendente | tentativa morre; nova tentativa necessária | não |
| N15 | Grep: nenhum uso de disable fingerprint verification; logs sem OTK/URI | passa | não |
| N16 | QR com `server` fora da allowlist embutida | `SERVER_NOT_ALLOWED`, nenhuma conexão aberta | não |

Aceite de M2 exige A1, A2, N1, N2, N3 e N10 em Android **e** iOS reais.

## 8. Itens em aberto

1. Confirmar em dispositivo que `transport.localCertificateId`/`remoteCertificateId` são preenchidos no Android e iOS.
2. Confirmar em dispositivo que o libwebrtc mobile rejeita certificado ≠ `a=fingerprint` (N2).
3. Confirmar se `generateCertificate` no RN expõe o fingerprint esperado; senão, derivá-lo de outro modo.
4. Fallback (assinar os `a=fingerprint` dos dois SDPs no transcript) só se 1 falhar **e** 2 estiver provado; exige nova aprovação.
5. ~~Escolher bibliotecas do mobile.~~ **Resolvido** (answers 005): `@noble/curves` + `@noble/hashes` (Ed25519 com `zip215:false` sempre explícito, HMAC-SHA256, SHA-256, comparação constante), `expo-crypto` (`getRandomValues`, nunca `getRandomBytes`; polyfill carregado antes do noble), `expo-secure-store` (semente cifrada por chave do Keystore/Keychain, sem biometria; backup Android exclui o SecureStore). Host usa `ed25519-dalek` `verify_strict`. Nada instalado até a implementação ser liberada.
6. ~~Fixar versão de `react-native-webrtc`.~~ **Resolvido** (answers 004): `react-native-webrtc 155.0.1`; upgrade exige nova revisão da seção 4.
7. ~~Allowlist de `server`: onde configurar e como atualizar.~~ **Resolvido** (answers 004): lista fixa embutida no app; trocar de servidor exige atualizar o app (seção 1).

## 9. Desvios

- D1: `otk`/`host_pk` em base64url sem padding (ticket 007 dizia base64) — evita `+/=` no URI. **Aprovado em 2026-11-07 (answers 004).**
- D2: novo parâmetro `v=1` no URI (C2 exige versão; o URI do ticket não tinha). **Aprovado em 2026-11-07 (answers 004).**
- D3: campos extras no transcript: `mode` e `room_id`, para separar pareamento de reconexão e amarrar a sala. **Aprovado em 2026-11-07 (answers 004).**
- D4: lista única e completa de erros (aprovado 2026-11-07) — seção 2.2; acrescenta `VERSION_UNSUPPORTED` e `SERVER_NOT_ALLOWED` aos 8 de C3; `ALREADY_EXISTS` entrou depois (C4 D4, answers 011).
- D5: rótulos de papel neutros `"offerer"`/`"answerer"`, válidos para qualquer cliente. **Aprovado em 2026-11-07 (answers 004).**

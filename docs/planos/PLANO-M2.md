# PLANO M2 — Identidade e pareamento

**Status: APROVADO em 2026-11-08 (answers 014) — execução liberada: D2a, M2a, D2b, S2 (instalação das deps listadas e testes). Sem commit, push ou deploy. M2b e aceite final exigem Android e iOS reais.**

**Andamento (2026-11-08):** D2a, S2, M2a e D2b implementadas e aprovadas em verificação independente no PC (D2b após correção de timeout e N15). Pendentes: M2b, E1–E3, aceite em aparelho, gate M1. Diretórios temporários dos testes D2b agora apagados ao fim de cada teste. Observação menor: N15 não pega escrita via `CONOUT$`.

Base: `SPEC-C2-identidade.md` (aprovada, answers 012); `PLANO-live-cmd.md` seções 6 (M2.1–M2.3) e 8 (D2, S2, M2). Nada aqui libera instalação, compilação pesada, deploy ou commit.

## 1. Estado atual

- `desktop/src/main.rs` (462 linhas, crate `live-cmd-echo`): host de eco M1, `webrtc =0.21.1`, envelope C1, sem identidade.
- `signaling/` (`room.ts`, `worker.ts`): salas C1 com até dois peers; sem admission, rate limit nem TURN.
- `mobile/`: não existe; só design.
- Gate M1 **não** aprovado (PLANO linha 282): falta Ctrl+C físico no Windows, transporte dispositivo/ICE e E2E mobile.

## 2. Bloqueios

| Bloqueio | Trava | Não trava |
|---|---|---|
| Gate M1 (Ctrl+C físico, E2E mobile/ICE) | D2b, M2b, S2, A1/A2/N1–N3/N10 | D2a, M2a (puros, sem rede) |
| Liberação de dependências | todas as fatias | — |
| Projeto mobile inexistente (M1.3) | M2a/M2b | D2a |
| Experimentos E1–E3 (seção 5) | M2b (fingerprints no mobile) | D2a, D2b, M2a |

## 3. Fatias (ordem)

Cada fatia: um módulo, um agente, verificação própria.

| Fatia | Módulo | Conteúdo | Deps novas (instalar só com liberação) | Casos C2 §7 | Onde testa |
|---|---|---|---|---|---|
| D2a | desktop/ | `auth.rs`: transcript T (§3), `sig_*`/`mac_*`, parse/validação de URI (§1) e de mensagens (§2), máquina de estados (§2.1) com relógio injetável, OTK + CAS + cache de nonces (§5). Gera fixture do vetor 3.1 | `ed25519-dalek` (`verify_strict`), `hmac`, `sha2`, `rand_core`/`getrandom`, `base64` | A3 (lado Rust), N4–N9, N7b, N12–N14, N16 (parse) | PC, `cargo test` (liberação) |
| M2a | mobile/ | `auth.ts`: mesmo transcript/provas, parse do QR + allowlist; verifica a fixture Rust | `@noble/curves`, `@noble/hashes`, `expo-crypto` | A3 (lado JS), N8, N7b, N16 | PC, `node --test` |
| D2b | desktop/ | Ligar D2a ao `pty-channel`: fingerprints do transporte (§4), timeout 30 s, gate "nada antes de AUTHENTICATED", trust store atômica + chave no cofre (DPAPI/Credential Manager), revogação, QR no terminal | `keyring` (ou `windows` DPAPI), `qrcode` | N11, N12, N14, N15, N10 (host) | PC loopback (peer Rust no papel offerer) |
| M2b | mobile/ | Câmera/QR, `generateCertificate` + fingerprints via `getStats()` (§4), `expo-secure-store`, lista de hosts, reconexão | `expo-secure-store`, `expo-camera`, `react-native-webrtc 155.0.1` | A1, A2, N1, N2, N3, N10 | **Android + iOS reais** |
| S2 | signaling/ | Admission (limite de salas/conexões por IP, tamanho/taxa de mensagens), sem logar segredos (§1); TURN fica para depois do M2 (answers 013) | nenhuma (TURN: credencial externa) | N15 (signaling) | PC, `npm test` |

Grep de CI (N15): `with_disable_certificate_fingerprint_verification` ausente; nenhum log com `otk=`/URI `remoteterm://`.

## 4. Testes em aparelho real (aceite M2)

A1, A2, N1, N2, N3 (só Android) e N10 em Android **e** iOS. Peers no PC não contam. Registrar modelo, versão do SO e build.

## 5. Experimentos em aparelho (C2 §8 itens 1–4)

Rodar antes de M2b, num build mínimo descartável:

| # | Experimento | Sucesso |
|---|---|---|
| E1 | Após `connected`, `getStats()` tem um `transport` ativo com `localCertificateId` e `remoteCertificateId` preenchidos | Ambos presentes no Android e no iOS |
| E2 | SDP com `a=fingerprint` adulterado | DTLS nunca chega a `connected` nos dois SOs |
| E3 | Fingerprint do certificado gerado por `generateCertificate` | Igual ao `fingerprint` local dos stats e ao SHA-256 recalculado do DER |
| E4 | Fallback (assinar `a=fingerprint` dos SDPs) | Só se E1 falhar **e** E2 passar; exige nova aprovação |

## 6. Decisões (answers 013)

1. TURN: **depois do M2**. S2 sem emissão TURN; redes que exigem TURN ficam fora do aceite de M2.
2. Biometria: **opcional, desligada por padrão**; só trava a UI local. Chave segue sem biometria (C2 seção 6).

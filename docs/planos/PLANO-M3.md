# PLANO M3 — Terminal autenticado

**Status: APROVADO (answers 017, 2026-11-09). Deps `windows` e `vt100` liberadas (D3c, D3d). Deploy segue vetado. Aceite de M3 exige Android e iOS reais.**

Base: `SPEC-C3-controle.md` (aprovada, answers 012; D1–D10 aprovados); `PLANO-live-cmd.md` seção 6 (M3.1–M3.3), gate M3 ("terminal autenticado com replay e não duplicação") e fila D3/M3; `PLANO-M2.md`.

## 1. Estado atual

- `desktop/src/main.rs` (784 linhas) + `auth.rs` (705): handshake C2 completo no `pty-channel`, checagem de canais (rótulo, ordenado, sem retransmit/lifetime, `protocol` vazio), trust store + revogação relida a cada 1 s. Após AUTHENTICATED o host só **ecoa texto**; mensagem binária antes da auth fecha; qualquer mensagem no `file-channel` fecha (C4 ausente).
- Sem PTY, sessões, frames binários, ring buffer, heartbeat nem perfis.
- `auth::Error` já tem os códigos usados pela C3 (`NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`, `INVALID_MESSAGE`, `VERSION_UNSUPPORTED`); a C3 não cria códigos.
- `mobile/`: não existe.
- **Progresso (answers 016):** D3a feita e verificada (PASS, 2026-11-08): `desktop/src/proto.rs` + fixtures `desktop/tests/fixtures/c3-frames.json`/`c3-controls.json`. M3a feita e verificada (PASS): `mobile/src/pty.ts` + `pty.test.ts` no `npm test`. Pós-verificação: `attached()` renumera o nunca enviado em vez de descartar; fixtures ganharam label 64/65 emoji e mensagem com exatamente 65536 B. Para M3b: validar mensagens host→cliente (`*_ok`, `input_ack`, `replay_*`, `error` etc.), que M3a não lê. **D3b, D3c, D3d feitas e verificadas (PASS, 2026-11-13, commit `2565d12`)**: `session.rs`, `conpty.rs`, `screen.rs`, `pty-channel` ligado em `main.rs`; `pty.ts` valida controles host→cliente (fixture `c3-host-controls.json`).
- Gate M1 **não** aprovado (Ctrl+C físico no Windows, E2E mobile/ICE). Gate M2 **não** aprovado (M2b, E1–E3 e aceite em aparelho pendentes).

## 2. Bloqueios

| Bloqueio | Trava | Não trava |
|---|---|---|
| Gate M1 (Ctrl+C físico, E2E mobile/ICE) | D3c, M3b, M3c, aceite M3 | D3a, D3b, D3d, M3a (puros, sem rede) |
| Gate M2 em aparelho (M2b, E1–E3, A1/A2/N1–N3/N10 C2) | **todo o mobile de M3** (M3b, M3c) e o aceite M3; D3c (fila: "Gate autenticação") | D3a, D3b, D3d, M3a |
| Liberação de dependências | fatias com deps novas | D3a, M3a |
| Perguntas abertas (seção 6) | D3b (resize), D3d (emulador/snapshot, `cmd`) | D3a, M3a |

## 3. Fatias (ordem)

Cada fatia: um módulo, um agente, verificação própria.

| Fatia | Módulo | Conteúdo | Deps novas (instalar só com liberação) | Casos C3 §8 | Onde testa |
|---|---|---|---|---|---|
| D3a | desktop/ | `proto.rs`: parse/serialização do frame (§3: cabeçalho 30 B, opcodes 01–05, faixas de payload, resize 1–1000, heartbeat com `session_id` zerado) e dos controles (§2.1–2.3: `request_id`, u64 como string decimal, campos desconhecidos, ≤ 65536 B). Gera fixtures binárias/JSON | nenhuma | N3, N10 | PC, `cargo test` |
| D3b | desktop/ | `session.rs`: tabela de sessões com PTY falso injetável e relógio injetável: `seq` por `(session_id, client_pk)` e dedup (§4), ring buffer 10 000 linhas/8 MiB e offsets (§5), ordem do attach com `replay_gap`/`replay_begin`/`replay_end` (§5), TTL 24 h (§5), limites 16 sessões/4 anexos/fila 1 MiB (§6), `NOT_FOUND` uniforme (§2.2), revogação passos 1–3 (§7), regra de resize (seção 6 item 1) | nenhuma | A3, A4, A5 (lógica), A7, N4, N5, N6 (sessões/anexos), N8 (lógica) | PC, `cargo test` |
| D3c | desktop/ | Ligar D3a/D3b ao `pty-channel` no lugar do eco: ConPTY real, perfis do host por `profile_id` (§2.1, item 5), `max-message-size` < 65536 fecha (§1), taxa de controles 50/s rajada 100 (§6), `bufferedAmount` 1 MiB / retoma 256 KiB (§6), heartbeat 10 s / queda 30 s (§3), `client_focus` ao vivo / 5 s / nenhum (D10), revogação fecha em ≤ 1 s | crate de PTY (pergunta 3) | A1, A2, A6, A8, N1, N2, N6 (rajada), N7, N8, N11 | PC loopback (peer Rust offerer) + ConPTY real |
| D3d | desktop/ | M3.3 lado host + D9: emulador no host gera `screen_snapshot` após gap (§5, item 3); parse OSC 133 → `command_completed` e OSC 7 → `cwd_changed` (item 6), nunca por regex; integração nos perfis pwsh/WSL; `cmd` conforme pergunta 2 | crate de emulação (pergunta 4) | A4 (com snapshot), fim de shell ≠ fim de comando (M3.2 PLANO) | PC, `cargo test` + `vim`/`htop` reais |
| M3a | mobile/ | `pty.ts`: mesmo frame/controles de D3a, u64 como string/`BigInt`, janela de input 64 KiB/256 frames, retenção do que foi digitado offline (§4, item 2); verifica as fixtures de D3a | nenhuma | N3, N10 (lado JS), A3 (lado cliente) | PC, `node --test` |
| M3b | mobile/ | xterm local, abas por sessão, teclado (Ctrl+C = `0x03`, IME), resize, reconexão com `from_offset` + reenvio só de `seq` pendente digitado online, confirmação do texto offline, aplicar `screen_snapshot`, `client_focus` e heartbeat só em primeiro plano (itens 8–9), `bufferedAmount` checado após cada `send` (item 10) | xterm + container (pergunta 5) | A1, A2, A5, A8, N11 | **Android + iOS reais** |
| M3c | mobile/ | M3.3 lado app: mostrar `command_completed` e `cwd_changed`; sem suporte, cwd "desconhecido" | nenhuma além de M3b | — (PLANO M3.3) | **Android + iOS reais** |

N9 (upload C4 + digitação, p95 < 150 ms) fica para M4: depende do `file-channel`.

## 4. Testes em aparelho real (aceite M3)

A1, A2, A5, A8 e N11 em Android **e** iOS, contra host Windows com ConPTY. A8 inclui Ctrl+C interrompendo processo real. Medir eco ao vivo (meta p95 < 150 ms em rede local; medição, não garantia). Validar `vim`/`htop` após `replay_gap` antes de alegar recuperação total (§5). Pendente da C3 §9 item 10: `max-message-size` anunciado pelo mobile. Peers no PC não contam. Registrar modelo, versão do SO e build.

## 5. Experimentos

| # | Experimento | Sucesso |
|---|---|---|
| E5 | `max-message-size` anunciado pelo `react-native-webrtc 155.0.1` (C3 §9 item 10) | ≥ 65536 no Android e no iOS; senão volta à spec |
| E6 | `bufferedamountlow` com limiar 256 KiB e leitura de `bufferedAmount` após `send` (item 10) | evento dispara e a pausa/retomada funciona nos dois SOs |
| E7 | `vim`/`htop` com o snapshot escolhido (D3d) após gap forçado | tela legível e utilizável após o replay |

E5/E6 rodam no mesmo build descartável de E1–E3 (PLANO-M2 §5), antes de M3b.

## 6. Perguntas abertas (decisão do usuário)

**Decididas (answers 015):** 1 — tamanho independente por aparelho: o PC envia bytes idênticos a todos e cada app monta a tela no próprio tamanho PTY com um tamanho por vez, editável pelo usuário e acompanhando redimensionamento/divisão de janelas (referência "9remote", a confirmar); 2 — `cmd` sem suporte no M3; 3 — ConPTY direto (crate `windows`); 4 — `vt100` no host; 5 — terminal nativo em React Native, sem WebView; 6 — perfis em arquivo de configuração. **Decididas (answers 017):** terminal invisível no PC (segundo plano, sem janela, não atrapalha quem usa o PC); com vários aparelhos ativos vence o resize do último que digitou ou redimensionou. **Referência 9remote confirmada** (github.com/decolua/9remote, `ptyDaemon.js`/`useXTerm.js`): também "último resize vence". Para M3b copiar: tamanho editável via fonte/pinch (cols/rows = espaço ÷ fonte), fit + resize antes de pedir replay, reenviar tamanho ao focar/girar, debounce 100 ms, mínimo 30×2, não reenviar tamanho igual.

1. **Tela com vários celulares de tamanhos diferentes** (C3 item 4). O terminal só tem um tamanho por vez. **Recomendação:** usar o tamanho do último aparelho que digitou ou redimensionou; os outros ajustam a exibição. Testar com `vim`/`htop` antes de algo mais complexo.
2. **`cmd` não avisa quando um comando termina nem a pasta atual** (C3 item 6). **Recomendação:** em M3, marcar `cmd` como "sem suporte" (pasta desconhecida, sem aviso de fim); pwsh e WSL com suporte completo. Um `PROMPT` customizado pode vir depois, mas não dá o código de saída.
3. **Biblioteca para abrir o terminal no Windows.** **Recomendação:** `portable-pty` (usada pelo WezTerm, cobre ConPTY e Unix, o que A8 pede). Alternativa: chamar ConPTY direto via crate `windows`, só Windows e mais código.
4. **Como guardar e reenviar a tela após perda de dados** (C3 item 3). **Recomendação:** crate `vt100` (pequena) gerando a tela como sequência de escape ANSI, enviada em base64 no `data` do `screen_snapshot`. `alacritty_terminal` só se `vt100` falhar em E7.
5. **Terminal no celular.** **Recomendação:** `@xterm/xterm` dentro de `react-native-webview` (o xterm é feito para navegador; RN não tem equivalente maduro). Custo: uma ponte WebView para bytes e teclado, a validar com IME em aparelho.
6. **Onde ficam os perfis (pwsh, cmd, WSL).** A C3 diz só "definidos no host". **Recomendação:** lista fixa no código em M3 (pwsh, cmd e WSL se instalado); arquivo de configuração só quando alguém precisar de um perfil novo.

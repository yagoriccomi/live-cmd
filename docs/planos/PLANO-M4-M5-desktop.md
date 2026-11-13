# PLANO M4+M5 — Desktop: arquivos resumíveis e app de bandeja

| Campo | Valor |
|---|---|
| **Tarefa** | `M4-M5-desktop` |
| **Origem** | descrição livre ("/loop pode fazer m4 e m5 inteiros", 2026-11-13) |
| **Tipo** | História |
| **Modo de execução** | 🔁 Loop (sem confirmação) |
| **Data** | 2026-11-13 |
| **Branch** | uma por fatia, `feat/<fatia>` em worktree irmã `../live-cmd-<fatia>` |

## 1. Enunciado Canônico

- **Problema:** o PC só tem um programa de linha de comando (`live-cmd-echo`): sem arquivos (C4), sem janela, sem bandeja, sem instalador, uma conexão por execução.
- **Resultado esperado:** instalador `.exe` que coloca no Windows um app de bandeja (Tauri 2) que mantém o host rodando, mostra o QR de pareamento, lista/revoga aparelhos, liga "Iniciar com o Windows", abre terminal local e conecta como cliente a outro PC; host com `file-channel` C4 completo (arquivo, pasta, rede, retomada).
- **Como validar:** testes `cargo test` cobrindo a matriz C4 §10 que cabe no PC (peer Rust no loopback); app instalado, reinício do Windows com autostart ligado sobe o host sem janela; revogação pela janela derruba conexão. Aceite com celular continua pendente (Android/iOS).

## 2. Escopo

**Vou fazer:**
- D4 (PLANO M4.1): C4 inteira no host — §2–8 arquivo único, §8A pastas, §8B rede, staging durável, expurgo, revogação.
- Presença para reconexão (PLANO §146 "host mantém registro de presença"): adendo de spec + sinalização local + laço do host aceitando conexões repetidas.
- D5 (PLANO M5.1): Tauri, bandeja, janela de pareamento, aparelhos, autostart opt-in (HKCU Run), cliente local (terminal desta máquina) e cliente remoto (este PC como offerer de outro host), instalador NSIS.

**NÃO vou fazer** [#8]:
- Nada em `mobile/` (M4.2, M3b/c, M5 mobile) — o celular vem depois.
- Deploy do Worker (vetado). Sinalização segue local (`wrangler dev`).
- Assinatura digital do instalador (sem certificado); serviço do Windows antes do login (P6 do PLANO).
- macOS/Linux.

## 3. Premissas Assumidas

| # | Premissa | Por quê | Se estiver errada… |
|---|---|---|---|
| P1 | "M4 e M5 inteiros" libera C4 e a dependência Tauri 2 (+ plugins oficiais) | PLANO M5.1 nomeia Tauri; C4 exige liberação explícita, dada nesta mensagem [#98] | remover `desktop/app/` e o `file-channel` volta a fechar |
| P2 | Ordem M4 → M5 (PLANO); presença em paralelo | dependência do PLANO D5 "Gate arquivos" [#13] | fatias independentes, reordenáveis |
| P3 | Instalador NSIS `.exe`, sem assinatura (aviso "editor desconhecido") | padrão Tauri; sem certificado na máquina [#8] | trocar `bundle.targets` para `msi` |
| P4 | Host vira biblioteca (`desktop/src/lib.rs`), CLI e app Tauri usam a mesma | evita duplicar o host [#6] | — |
| P5 | Cliente local fala com o hub em processo (sem WebRTC); cliente remoto usa WebRTC como offerer + C2 | local não precisa de rede; remoto é o mesmo protocolo do celular [#6] | — |
| P6 | Presença: sala fixa por host derivada de `host_pk`, reaberta pelo host após cada conexão; spec em `SPEC-C1-presenca.md` | PLANO §146/§149 prevê, nenhuma spec define [#8] | revisar com o usuário antes do deploy |
| P7 | UI desenhada pela `design-de-interface-projeto` antes do código (Regra 4) | superfície visual nova | — |
| P8 | Agentes nível `alto` (Loop) | regra da skill | — |
| P9 | Fluxo git do projeto: fatia em branch, revisão do coordenador, fast-forward em `main` e push (autorizado: "pode dar push"); sem PR | histórico do repo vai direto a `main` | abrir PR por branch |
| P10 | `get_usage` indisponível neste ambiente; checkpoints de uso não aplicáveis | ferramenta ausente | — |
| P11 | Pareamento PC-a-PC por "Copiar link para outro PC" (design §8 item 1a): clipboard fora do histórico `Win+V`, limpo ao expirar/usar. **Desvio de C2 §1, pendente de aprovação do usuário** | recomendada no design; PCs costumam estar longe um do outro | remover o botão e o campo de colar link; PC-a-PC fica sem pareamento |
| P12 | Parear, Aparelhos e Configurações em uma janela com 3 abas (design §8 item 2a) | menos código | três janelas |
| P14 | Presença (SPEC-C1-presenca P3/P4): uma sala por instância local; motivo `busy` + WS 1013 novos; presença pós-hibernação fora. **Pendente de aprovação do usuário** | local-only, deploy vetado | revisar a spec antes do deploy |
| P13 | Fechar a janela invalida a OTK; trocar de aba não (design §8 item 3b) | OTK sem ninguém olhando é só risco | manter a OTK até 300 s |

## 4. Decisão Visual

- **Tem superfície visual?** Sim (bandeja, janela de QR, aparelhos, configurações, terminal local/remoto).
- **Precisa de mockup?** Sim — telas novas.
- **`design-de-interface-projeto`:** acionada na fatia D5-design; resultado em `docs/design/desktop-app.md`.

## 5. Terreno

| Arquivo | Papel hoje | O que muda |
|---|---|---|
| `desktop/src/main.rs` (1121) | CLI + `run` de uma conexão + testes loopback | vira `lib.rs` + `main.rs` fino; `file-channel` ligado; laço de presença |
| `desktop/src/auth.rs` | C2, trust store, revogação | lista de aparelhos (nome/data) para a janela |
| `desktop/src/session.rs`, `screen.rs`, `conpty.rs`, `proto.rs` | C3 | consumidos pelo cliente local |
| `desktop/src/files.rs` | — | novo: C4 |
| `desktop/app/` | — | novo: crate Tauri |
| `signaling/` | sala efêmera local | presença |

## 6. Fatias

| Fatia | Módulo | Conteúdo | Nível | Depende de | Branch | Estado |
|---|---|---|---|---|---|---|
| D4a | desktop/ | `files.rs`: C4 §2–8 arquivo único (frame, controles, caminhos §6, staging/bitmap/ACK em lote, finalização, resume, cancel, revogação, expurgo, limites §7), relógio e disco injetáveis, testes A3–A6, A8, A9, N2–N11, N13–N15 | alto | — | `feat/d4a-files-core` | em `main` (42c85c1; 85 ok, clippy limpo) |
| S1 | signaling/ + docs | `SPEC-C1-presenca.md` + presença no Worker local + testes | alto | — | `feat/s1-presence` | revisada (32/32, tsc ok); entra em `main` junto com D5a (quebra o host atual: `CONNECTION_ID` removido) |
| D5-design | docs | mockup das telas (bandeja, QR, aparelhos, config, terminal) | alto | — | `feat/d5-design` | em `main` (b19990e) |
| D4b | desktop/ | §8A lotes + §8B rede em `files.rs` (A10–A17, N17–N30 no PC) | alto | D4a | `feat/d4b-files-batch` | rodando |
| D4c | desktop/ | `file-channel` em `main.rs`: controles/frames, backpressure 512/128 KiB, prioridade do PTY, taxa conjunta; loopback A1, A2, A7 (N9), N1, N12 | alto | D4b | `feat/d4c-file-channel` | na fila |
| D5a | desktop/ | `lib.rs` + laço de presença do host (aceita conexões em sequência e reconexão) | alto | D4c, S1 | `feat/d5a-host-lib` | rodando (antecipada: contrato `Service` fixo; D4c liga arquivos depois) |
| D5b | desktop/app | Tauri: bandeja, QR, aparelhos/revogar, autostart HKCU Run, Sair; instalador NSIS | alto | D5a, D5-design | `feat/d5b-tray` | rodando (adaptador `host.rs` com stub até D5a) |
| D5c | desktop/app | cliente local (terminal desta máquina) e remoto (offerer + C2 + C3) | alto | D5b | `feat/d5c-client` | na fila |

## 7. Impacto em Dados e Contratos

- **Banco:** nenhum. Disco: `<data_dir>/staging/` (C4 §5).
- **Contratos:** C4 já aprovada; novo adendo C1 presença (P6).
- **Configuração:** `profiles.json` existente; limites C4 configuráveis no host; servidor de sinalização configurável.
- **LGPD:** nunca logar caminhos, nomes, servidores (C4 §6/§8B); teste de allowlist de logs continua.

## 8. Plano de Testes

| Nível | O que cobre | Caminho de falha |
|---|---|---|
| Unidade [#41] | `files.rs` (caminhos, frame, bitmap, expurgo com relógio) | N2, N3, N6–N11, N17–N21 |
| Integração [#42] | loopback Rust offerer no `file-channel` | N1, N12, queda/retomada |
| E2E [#43] | instalar, reiniciar, parear via janela, revogar | revogação fecha conexão |

**Comando:** `cd desktop && cargo test && cargo clippy --all-targets -- -D warnings`; `cd signaling && npm test`.

## 9. Riscos e Rollback

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Tauri/WebView2 incompatível com tokio do host | Média | app não sobe | host em runtime próprio dentro do processo |
| `MoveFileExW`/handles relativos complexos no Windows | Média | corrida de caminho | crate `windows` já liberada; testes N4/N5 |
| Presença sem spec aprovada | Alta | refazer depois | adendo isolado, local-only, deploy vetado |

**Rollback** [#84]: uma branch por fatia; `git revert` do merge da fatia.

## 10. Definição de Pronto

- [ ] Fatias revisadas e em `main`
- [ ] `cargo test`, `clippy -D warnings`, `npm test` (signaling) verdes
- [ ] App instalado e conferido (bandeja, QR, autostart, revogação)
- [ ] `docs/planos/ENTREGA-M4-M5-desktop.md`

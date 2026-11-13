# ENTREGA — M4-M5-desktop

| Campo | Valor |
|---|---|
| **Tarefa** | `M4-M5-desktop` |
| **Plano** | [`PLANO-M4-M5-desktop.md`](PLANO-M4-M5-desktop.md) |
| **Modo** | 🔁 Loop |
| **Data** | 2026-11-13 |
| **Branch / Commits** | fatias `feat/*` (12), todas em `main` por fast-forward/merge; `main` em `1b5af93` + este relatório |
| **Status** | ⚠️ Entregue com pendências (conferência manual do app instalado; aceite com celular) |

## 1. O que foi feito

O PC agora tem um app de bandeja instalável (`live-cmd_0.1.0_x64-setup.exe`) que mantém o host rodando, mostra o QR de pareamento, lista e remove aparelhos, liga "Iniciar com o Windows", abre um terminal desta máquina e conecta como cliente a outro PC. O host aceita vários aparelhos ao mesmo tempo e reconexões, e transfere arquivos e pastas (inclusive em pasta de rede) com retomada após queda.

## 2. Arquivos alterados

| Arquivo | Mudança |
|---|---|
| `desktop/src/lib.rs` | criado: host como biblioteca, presença (vários clientes por `connection_id`), `file-channel` com backpressure e prioridade do PTY |
| `desktop/src/main.rs` | alterado: CLI fina sobre `lib.rs` |
| `desktop/src/files.rs` | criado: C4 inteira (arquivo único, lotes §8A, rede §8B com prazo, staging durável, expurgo, revogação) |
| `desktop/src/client.rs` | criado: cliente local (hub em processo) e remoto (offerer WebRTC + C2 + C3), `seq` de input dado no envio |
| `desktop/src/session.rs`, `screen.rs`, `conpty.rs`, `proto.rs`, `auth.rs` | C3 e lista de aparelhos |
| `desktop/app/` | criado: app Tauri 2 (bandeja, 3 abas, terminal, configurações, instalador NSIS) |
| `signaling/` | presença com sala fixa por host e até 4 clientes simultâneos |
| `desktop/scripts/d1-integration.cjs` | testes contra o Worker local falando C2/C3 |
| `docs/planos/SPEC-C1-presenca.md`, `docs/design/desktop-app.md` | adendo de presença e desenho das telas |

## 3. Como validar

1. Rodar `desktop/app/target/release/bundle/nsis/live-cmd_0.1.0_x64-setup.exe` (aviso "editor desconhecido" é esperado, P3).
2. Com `wrangler dev` em `signaling/`, abrir o app pela bandeja: aba Parear mostra o QR e o contador de 5 min.
3. Aba Aparelhos: parear um cliente (outro PC via "Copiar link"), depois Remover: a conexão cai.
4. Configurações → "Iniciar com o Windows" ligado, reiniciar: host sobe sem janela.
5. Bandeja → terminal local: abre o prompt desta máquina.

```bash
cd desktop && cargo clippy --all-targets -- -D warnings && cargo test
cd desktop/app && cargo clippy --all-targets -- -D warnings && cargo test && cargo tauri build
cd signaling && npm test
```

## 4. Verificações executadas

- [x] Build — clippy `-D warnings` limpo em `desktop/` e `desktop/app/`; instalador NSIS gerado a partir de `main` (4,15 MiB)
- [x] Testes — desktop 116 ok (11 ignorados: bash/vim/Wrangler), app 13 ok, signaling 36/36; testes de rede rodaram contra `\\localhost\C$` (sem pulo)
- [x] Verificação independente por fatia — D4b, D5c e D5d com PASS do verificador (após 2 FAIL corrigidos em D4b e 2 em D5c)
- [ ] App instalado e conferido (bandeja, QR, autostart, revogação) — **não feito por mim**: exige olhar a tela e reiniciar o Windows
- [ ] Aceite com celular — fora de escopo (Android/iOS depois)

Testes que oscilam sob carga da máquina (passam isolados): `loopback_backpressure`, `s2_*`, `c3_n7_*`, `d4c_n12_*`, `n25*`.

## 5. ⚠️ Premissas assumidas (revisar)

Ver seção 3 do plano (P1–P17). Decididas pelo usuário em `answers/2026_11_13 gerenciador-terminais-remoto 018.md`: P3, P11, P13, P14. As demais foram assumidas no Loop:

| # | Premissa | Como mudar |
|---|---|---|
| P4 | host virou biblioteca usada pela CLI e pelo app | — |
| P5 | cliente local sem WebRTC; remoto como offerer | — |
| P6 | presença em sala fixa derivada de `host_pk` (spec local, sem deploy) | revisar `SPEC-C1-presenca.md` antes do deploy |
| P15 | Sair com `tauri-plugin-dialog`; servidor padrão `ws://127.0.0.1:8787` | trocar o padrão no deploy |
| P17 | janelas 512/128 KiB da C4; N9 só medido no aparelho | ajustar na spec |

## 6. Decisão visual

- **Mockup:** sim — telas novas.
- **`design-de-interface-projeto`:** acionada (D5-design); resultado em `docs/design/desktop-app.md` (janela única com 3 abas, P12).

## 7. Pendências e fora de escopo

- Conferência manual do app instalado (item 4 acima).
- Simplificações marcadas `ponytail:` no código, principais:
  - `lib.rs:249` — `error` do cliente com `file_id` não pausa a transferência no host (o cliente só para de pedir).
  - `files.rs:995` — hash do `file_download_init` síncrono; trava os controles da conexão em arquivos grandes.
  - `client.rs` — fila `Outbound` do cliente remoto sem limite; `peer()` escuta só em 127.0.0.1 até o deploy.
  - `files.rs:397` — thread presa numa chamada de rede do SO não é cancelada, só desiste no próximo `beat()`.
  - `Files::open` na partida do host roda sem prazo de rede: compartilhamento fora do ar atrasa o início do serviço.
- Fora de escopo: `mobile/`, deploy do Worker, assinatura do instalador, serviço antes do login, macOS/Linux.

## 8. Próximo passo natural

Conferir o app instalado (seção 3); depois M4.2/M5 mobile (Android) sobre o mesmo host.

## 9. Rastreabilidade

- **Jira:** não há.
- **`CLAUDE.md`:** nada novo.

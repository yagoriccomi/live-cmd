# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** desvios D9–D10 da seção 10 de `docs/planos/SPEC-C3-controle.md`; aprovação final de C2/C3/C4.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | C3 D9: `screen_snapshot` antes do replay após gap | Aprovar | Emulador no host; crate e formato ainda abertos (C3 seção 9 item 3) |
| 2 | Importante | C3 D10: pedido `client_focus` | Aprovar | Host ajusta envio: ao vivo / 5 s / nenhum |
| 3 | Crítico | Aprovação final das specs C2, C3 e C4 | Aprovar | Libera o plano de implementação M2; instalar/implementar exige liberação explícita |

**Confirmações:** testes em Android e iOS reais continuam obrigatórios para o aceite de M2.
**Premissas:** nenhuma.
**Ainda em aberto (técnico):** crate de emulação e formato do snapshot; fallback OSC no `cmd`; tamanho único do PTY com vários clientes.

# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** itens 1, 2, 4 e 5 da seção 9 de `docs/planos/SPEC-C3-controle.md`.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | Visibilidade das sessões | Todos os aparelhos pareados veem todas | Host checa dispositivo/revogação em todo pedido |
| 2 | Importante | Texto digitado com conexão caída | Confirmar antes de enviar | Sem reenvio automático de entrada incerta |
| 3 | Importante | Tamanho com vários clientes | Resposta livre: "o terminal que vejo no celular deve estar em segundo plano no pc; se eu abrir em outro pc simultaneamente cada terminal tem seu tamanho, mas devem ter os textos sincronizados, como o 9router" | PTY roda sem janela no host; cada cliente com tamanho próprio e texto sincronizado (ver item aberto 4 da spec) |
| 4 | Importante | Shell da sessão | Perfis definidos no host, escolhidos por ID | Cliente nunca envia linha de comando |

**Confirmações:** nenhuma.
**Premissas:** "9router" citado como referência de comportamento; não verificado.
**Ainda em aberto:** como conciliar tamanho por cliente com um único PTY (item 4 da spec); itens 3, 6–10 da spec.

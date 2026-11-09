# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** perguntas do rascunho `docs/planos/PLANO-M2.md`.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | TURN já no M2? | Depois do M2 | S2 sem emissão TURN; M2 testado em redes com conexão direta/STUN; sem custo agora |
| 2 | Importante | Biometria ao abrir o app | Opcional, desligada por padrão | Só trava a UI local; chave segue em `expo-secure-store` sem biometria (C2 seção 6) |

**Confirmações:** nenhuma.
**Premissas:** redes que exigem TURN ficam fora do aceite de M2.
**Ainda em aberto:** aprovação do `PLANO-M2.md`; liberação para instalar dependências e implementar D2a/M2a.

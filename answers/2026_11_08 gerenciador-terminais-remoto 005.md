# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** escolha das bibliotecas de cripto e cofre do mobile (item 5 da seção 8 de `docs/planos/SPEC-C2-identidade.md`).
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | Bibliotecas de cripto/cofre do mobile | `@noble/curves` + `@noble/hashes` + `expo-crypto` + `expo-secure-store` | Só 1 módulo nativo novo; Ed25519 estrito configurável (`zip215:false`) |
| 2 | Importante | Chave Ed25519 fora do chip seguro (cifrada por chave de hardware, lida para memória ao assinar) | Aceitar, sem biometria | Root/jailbreak pode extrair; mitigação = revogação no host |

**Confirmações:** nada instalado até a implementação ser liberada.
**Premissas:** nenhuma.
**Ainda em aberto:** confirmação em aparelho Android/iOS dos IDs de certificado e da rejeição de fingerprint divergente; aprovação final da spec C2.

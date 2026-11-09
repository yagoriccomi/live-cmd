# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** itens 10–14 da seção 9 de `docs/planos/SPEC-C4-arquivos.md` (pastas inteiras e pastas de rede).
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | "Aplicar a todos" nos conflitos de pasta | Sim | Sobrescrever/renomear/pular todos de uma vez |
| 2 | Importante | Pasta enviada já existe no destino | Perguntar (mesclar pré-selecionado ou "nome (1)") | Nunca mistura sem confirmação |
| 3 | Detalhe | NAS sem identificação estável de arquivo | Aceitar | Confere caminho + tamanho + data; proteção menor registrada |
| 4 | Importante | Limites de pasta e prazo de rede | Aceitar | 100 000 arquivos, 64 níveis, 64 GB, 1 pasta por vez; 30 s por operação de rede, pausa retomável |

**Confirmações:** nenhuma.
**Premissas:** nenhuma.
**Ainda em aberto:** desvios D1–D16 da C4 e D1–D8 da C3; aprovação final das specs C2/C3/C4; testes em aparelho.

# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** itens 1–8 da seção 9 de `docs/planos/SPEC-C4-arquivos.md`.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | Tamanho máximo e espaço de incompletos | 64 GB por arquivo / 100 GB de staging, mínimo 5 GB livres | Configurável no host; excesso = `LIMIT_EXCEEDED` |
| 2 | Importante | Nome já existe no destino | Perguntar | Precisa de `ALREADY_EXISTS` (D4) |
| 3 | Importante | Extras na v1 | Pastas de rede + enviar pastas inteiras (ocultos visíveis com filtro) | Mais escopo em M4; regras de SMB e de diretório a especificar |
| 4 | Detalhe | Destino do download no celular | Pasta do app + "Salvar em…" | Incompleto de aparelho removido apagado na hora; sem cwd, app pede e lembra a última pasta |

**Confirmações:** nenhuma.
**Premissas:** itens 4 e 8 da spec (revogado, cwd desconhecido) aceitos pela recomendação descrita na opção escolhida.
**Ainda em aberto:** modelo de transferência de pastas; comportamento em SMB; desvios D1–D11 da C4; pesquisa de `bufferedamountlow`.

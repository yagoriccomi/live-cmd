# Decisões — gerenciador-terminais-remoto — 2026-11-08

**Contexto:** itens 3, 6, 7, 8 e 9 da seção 9 de `docs/planos/SPEC-C3-controle.md`.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | Restaurar tela após perda de saída | Resposta livre: "guardar e reexibir" | Host mantém emulador de terminal e envia snapshot da tela |
| 2 | Importante | Aviso de fim de comando e pasta atual | Já na primeira versão | Integração de shell (OSC 133/OSC 7) entra em M3 |
| 3 | Detalhe | Tempo do histórico de terminal encerrado | 24 h ou até fechar | Buffer até TTL ou `session_close` |
| 4 | Importante | Metas de atualização/conexão | Resposta livre: "com a tela do celular ligada/em foco no Windows a atualização deve ser ao vivo; em segundo plano o celular pode só checar quando abrir de volta; no PC pode atualizar a tela a cada 5 segundos" | Ao vivo em foco; celular em segundo plano sem tráfego; PC fora de foco a cada 5 s |

**Confirmações:** nenhuma.
**Premissas:** "guardar e reexibir" interpretado como snapshot gerado no host; "no PC a cada 5 s" interpretado como cliente PC fora de foco.
**Ainda em aberto:** crate de emulação no host; fallback de OSC para `cmd`; tamanho máximo real de mensagem WebRTC; desvios D1–D8 da C3.

# Decisões — gerenciador-terminais-remoto — 2026-11-13

**Contexto:** premissas pendentes do PLANO-M4-M5-desktop (P3, P11, P13, P14) antes do fechamento da versão PC.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Importante | Pareamento PC-a-PC | Botão "Copiar link para outro PC" | P11 aprovada: desvio de C2 §1 aceito (clipboard fora do histórico, limpo ao expirar/usar); commit 8371f85 fica |
| 2 | Importante | Segundo aparelho chegando durante conexão de outro | Conexões simultâneas: vários aparelhos se conectam ao mesmo tempo; só os comandos são serializados (um por vez) | P14 muda: presença precisa aceitar vários clientes negociando em paralelo; `busy`/1013 deixa de ser o caminho normal. Comandos já são ordenados por `seq` (C3 §4) |
| 3 | Detalhe | Fechar a janela do QR invalida a OTK | Não: vale até 300 s ou até o uso | P13 invertida; app não chama `cancel_pair` ao fechar a janela (QR continua oculto com janela minimizada/fechada) |
| 4 | Detalhe | Instalador sem assinatura | Sim, sem assinatura | P3 confirmada |

**Confirmações:** P3 (PLANO-M4-M5-desktop, 2026-11-13).
**Premissas:** nenhuma.
**Ainda em aberto:** nenhuma.

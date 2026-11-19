# Respostas 022 — pendências do desktop pós v0.1.20

Contexto: levantamento das pendências do app desktop após a release v0.1.20 (handoffs 002 e 003).

Premissas tomadas como certas (sem pergunta):
- Testes manuais (trava periódica, retomada da aba ativa, botões de recolher, Copiar no histórico, itens antigos do handoff 002) ficam com o usuário.
- Limites conhecidos (colar imagem só 24/32 bits, resume do codex, programas não sobrevivem a reboot) continuam como estão.

## Decisões

1. **Pasta da aba cmd**: ajustar o prompt. O perfil cmd abre com `PROMPT $E]7;file://localhost/$P$E\$P$G` para anunciar a pasta (OSC 7); prints, uploads e retomada passam a usar a pasta atual. (recomendada)
2. **Acesso a Dispositivos/Sair sem o menu ☰**: "Dispositivos conectados" vira item da barra lateral, junto de Pareamento e Configurações; "Sair" continua na bandeja. (recomendada)
3. **Limpeza Git**: apagar só branches/worktrees já incorporados ao `main` (inclui `feat/desktop-ui-ajustes`); os que têm trabalho fora do `main` ficam para o usuário revisar. (recomendada)
4. **Testes instáveis**: corrigir os seis agora, antes de novas funções (`d4c_a7_typing_during_upload`, `loopback_backpressure`, `n25_slow_network_expires`, `s2_two_clients_simultaneous`, `d4c_n12_revoke_during_upload`, `remote_upload_file`). (recomendada)

# Decisões — gerenciador-terminais-remoto — 2026-11-05

**Contexto:** Definição inicial de escopo, arquitetura de rede e stack para aplicativo gerenciador de terminais remotos inspirado no 9remote.
**Modo:** reformular

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Até onde queremos ir neste planejamento antes de colocar a mão no código? | **Opção A**: Criar a especificação técnica completa (arquitetura, tecnologias escolhidas e lista de tarefas) | Evita retrabalho; estabelece contratos claros de dados e protocolo antes da implementação de código. |
| 2 | Bloqueante | O que o aplicativo precisa fazer logo na primeira versão utilizável? | **Opção A + Envio de Arquivos**: Terminal de comandos com abas, pareamento por QR Code + transferência básica de arquivos | O escopo do MVP inclui sessões de PTY persistentes, aprovação rápida por QR code e canal de upload/download de arquivos entre celular e PC. Visualização de tela (desktop remoto) fica para o futuro. |
| 3 | Importante | Como o celular deve encontrar o computador quando você estiver na rua? | **Opção A**: Conexão direta P2P (WebRTC DataChannel) com servidor de sinalização leve e STUN/TURN | Tráfego direto e criptografado de ponta a ponta sem tráfego de dados por servidor central; menor latência ao digitar. |
| 4 | Importante | Como você prefere acessar o aplicativo no celular? | **Opção A (React Native)**: Aplicativo instalado desenvolvido em React Native / Expo | Suporte nativo multiplataforma (Android e iOS), permitindo teclado virtual customizado com teclas de controle (Ctrl, Esc, setas). |

**Confirmações** (decisões antigas que continuam valendo): nenhuma
**Premissas** (recomendadas aceitas por "não sei" ou não perguntadas no modo só bloqueantes): nenhuma
**Ainda em aberto** (dependiam de algo fora desta sessão):
- Definição da stack exata do Host PC (Node.js/TypeScript com `node-pty` vs Rust/Go).
- Infraestrutura do servidor de sinalização (Cloudflare Workers / VPS leve).
- Protocolo e limites para transferência de arquivos via DataChannel vs WebSocket HTTP stream.

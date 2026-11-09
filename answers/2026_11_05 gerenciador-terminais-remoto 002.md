# Decisões — gerenciador-terminais-remoto — 2026-11-05

**Contexto:** Definição do protocolo de comunicação de dados, transferência resiliente de arquivos e modelo de segurança/pareamento persistente com biometria opcional.
**Modo:** reformular

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Como separar o tráfego do terminal do tráfego de envio de arquivos? | **Opção A**: DataChannels WebRTC independentes (`pty` e `files`) | O terminal opera em canal com baixa latência e prioridade máxima, enquanto arquivos trafegam em canal separado sem engasgar a digitação. |
| 2 | Importante | Como o aplicativo deve lidar com o envio de arquivos e limites de tamanho? | **Opção A (Resumível estilo torrent + expiração de 24h)**: Chunks indexados de 64 KB com retomada automática | Arquivos de qualquer tamanho podem ser enviados; interrupções de rede não perdem o progresso já enviado; fatias incompletas há mais de 24 horas são expurgadas pelo Host. |
| 3 | Bloqueante | O que deve acontecer logo após escanear o QR Code pela primeira vez? | **Opção A (Pareamento instantâneo com chaves ponta a ponta permanentes)**: Conexão automática imediata | O escaneamento da chave efêmera autoriza o aparelho e realiza a troca de chaves criptográficas permanentes. A relação de confiança só se perde se um dos lados deletar a chave. |
| 4 | Importante | Como o celular deve se reconectar ao PC nos próximos acessos? | **Opção C**: Lembrar o aparelho com biometria (Face ID / digital) configurável no app | O usuário conecta remotamente de qualquer lugar com 1 toque, podendo ativar opcionalmente o bloqueio biométrico para impedir acessos acidentais. |

**Confirmações** (decisões antigas que continuam valendo):
- Host em Rust com `portable-pty` para ConPTY nativo no Windows e POSIX no Linux/macOS.
- Servidor de Sinalização em Cloudflare Workers + Durable Objects com WebSocket Hibernation.
- NAT Traversal com STUN público híbrido e Cloudflare Realtime TURN (1 TB/mês grátis com TLS 443).
- Cliente Mobile em React Native/Expo com `xterm.js` acelerado via Canvas e `react-native-keyboard-controller`.

**Premissas**: nenhuma.
**Ainda em aberto**: nenhuma decisão conceitual em aberto; avanço para consolidação das especificações arquiteturais.

# Decisões — gerenciador-terminais-remoto — 2026-11-06

**Contexto:** Definição completa do comportamento operacional, ciclo de vida de sessões, diretório de arquivos, aplicativo desktop híbrido (Host + Cliente com Tray/Autostart), notificações e privacidade.
**Modo:** reformular

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Para onde devem ir os arquivos enviados e de onde baixar? | **Opção 1B com navegação livre**: Destino padrão é a pasta atual onde o terminal está aberto, com seletor para navegar livremente pelo disco | Agilidade no fluxo de trabalho (arquivos caem direto no projeto atual), mantendo a flexibilidade de enviar ou puxar arquivos de qualquer pasta do PC. |
| 2 | Bloqueante | O que acontece quando a internet cai temporariamente no celular? | **Opção 2A**: Reconexão silenciosa com recuperação total do histórico da tela | O aplicativo tenta restabelecer o canal WebRTC em segundo plano e, ao reconectar, recupera as últimas linhas do ring buffer sem perda de contexto e sem travar a interface. |
| 3 | Importante | Como se comportam as abas de terminal ao fechar o app no celular? | **Opção 3B**: Fechamento automático de sessões inativas após 24 horas | Permite rodar tarefas de longa duração sem receio de desconexão, mas evita vazamento de processos órfãos esquecidos consumindo memória e CPU do computador indefinidamente. |
| 4 | Importante | Como o programa se apresenta e roda no computador? | **Opções 4B + 4C (Híbrido Host + Cliente Desktop)**: Ícone na bandeja do sistema (Tray), serviço em segundo plano com autostart, funcionando tanto como Host quanto como Cliente para outros PCs | O aplicativo do PC roda discretamente perto do relógio com inicialização automática; além de compartilhar o terminal local, sua interface permite gerenciar e conectar a terminais deste PC ou de outras máquinas remotas. |
| 5 | Importante | O app mobile deve avisar quando um comando longo terminar? | **Opção 5A**: Notificação push local no celular quando o comando finalizar | Maior conveniência: o usuário pode disparar compilações, backups ou scripts pesados, bloquear a tela do celular e receber um alerta sonoro/banner quando o prompt de comando for liberado. |
| 6 | Detalhe | A tela física do computador deve exibir algum aviso durante o acesso remoto? | **Opção 6B**: Modo silencioso total | Nenhuma janela, banner ou texto surge no monitor físico do PC enquanto o celular digita comandos, garantindo discrição e privacidade completa. |

**Confirmações** (decisões anteriores consolidadas):
- Host/Desktop em Rust (Tauri + `portable-pty` para ConPTY nativo no Windows, Linux e macOS).
- Sinalização em Cloudflare Workers + Durable Objects com WebSocket Hibernation ($0,00/mês).
- NAT Traversal com STUN público híbrido e Cloudflare Realtime TURN (1 TB/mês grátis com TLS 443).
- Canais WebRTC isolados (`pty-channel` para texto e `file-channel` para arquivos em chunks de 64 KB resumíveis estilo torrent com expiração de 24h).
- Pareamento inicial via QR Code efêmero com troca de chaves Ed25519 permanentes e biometria opcional no app mobile.

**Premissas**: nenhuma. Todas as decisões de arquitetura e produto foram resolvidas.
**Ainda em aberto**: nenhuma incógnita conceitual; o caminho está 100% livre para consolidação das especificações técnicas.

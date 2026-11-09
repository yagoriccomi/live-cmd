# Glossário de Domínio — Gerenciador de Terminais Remoto

Este documento define a linguagem canônica do projeto. Termos aqui definidos devem ser usados de forma consistente em especificações, discussões e código.

## Termos

### App Desktop Unificado (Host + Cliente Desktop)
O aplicativo executado no computador do usuário. Possui papel duplo e opera em segundo plano a partir da bandeja do sistema (System Tray) com inicialização automática (autostart) no boot do sistema operacional:
1. **Papel de Host:** Gerencia sessões de terminal locais (`PTY`), orquestra o envio e recebimento de arquivos e expõe conexões seguras P2P para clientes autorizados. Opera em modo silencioso total, sem notificações ou interferência na tela física do PC.
2. **Papel de Cliente:** Fornece interface gráfica para gerenciar e interagir com os terminais da própria máquina ou conectar-se como cliente a outros computadores remotos que executem o software. Desenvolvido em Rust com Tauri e `portable-pty`.

### Cliente Mobile
O aplicativo móvel (construído em React Native) executado no celular do usuário. Fornece interface com emulador `xterm.js` acelerado via Canvas, barra de teclado auxiliar com teclas modificadoras (`Ctrl`, `Alt`, `Esc`, setas), leitor de QR Code para pareamento, explorador de arquivos, suporte a reconexão silenciosa, notificações push locais de comando concluído e proteção biométrica opcional.

### Sessão PTY (Pseudo-Terminal)
Instância de terminal virtual persistente gerenciada pelo Host. Mantém um ring buffer em memória (ex: 10.000 linhas de scrollback) e sobrevive a quedas de rede do cliente. Possui uma política de ciclo de vida que encerra a sessão automaticamente após 24 horas consecutivas de inatividade.

### Diretório Ativo da Sessão (Session Working Directory)
O diretório do sistema de arquivos onde o processo do terminal está posicionado no momento. É utilizado como local de destino padrão para arquivos recebidos do celular e ponto de partida para downloads, com capacidade de navegação irrestrita pelo disco mediante autorização.

### Servidor de Sinalização (Signaling Server)
Serviço intermediário leve em Cloudflare Workers + Durable Objects responsável unicamente por coordenar o handshake inicial entre o Host e os Clientes (troca de ofertas/respostas SDP e candidatos ICE) para viabilizar a conexão WebRTC. Não intercepta nem armazena o tráfego do terminal ou os arquivos transmitidos.

### Canal P2P (WebRTC DataChannel)
Túnel de comunicação bidirecional, direto e criptografado de ponta a ponta estabelecido entre o Cliente e o Host. O projeto adota dois DataChannels isolados:
- **`pty-channel`**: dedicado exclusivamente aos fluxos interativos de texto do terminal com latência mínima e prioridade de entrega.
- **`file-channel`**: dedicado à transferência assíncrona de blocos binários de arquivos com controle de fluxo.

### Transferência de Arquivos Resumível (Chunked Transfer)
Mecanismo de transferência de arquivos fatiada em chunks indexados de 64 KB com hashing SHA-256 por bloco. Caso a conexão oscile ou seja interrompida, a transmissão retoma do último bloco recebido com sucesso (estilo torrent). Partes parciais que permanecerem incompletas por mais de 24 horas são expurgadas automaticamente pelo Host.

### Pareamento por Chave Única (One-Time Key / OTK)
Mecanismo de autorização inicial em que o Host gera uma chave efêmera de 5 minutos exibida como QR Code. A leitura pelo Cliente autoriza o dispositivo de imediato sem intervenção manual no teclado do PC e realiza a troca de chaves criptográficas permanentes.

### Dispositivo Emparelhado (Paired Device)
Relação de confiança persistente estabelecida entre o Host e o Cliente por meio de chaves assimétricas de longo prazo (Ed25519). Permite reconexões diretas e imediatas de qualquer rede externa sem necessidade de novo escaneamento de QR Code, salvo se as chaves forem revogadas ou excluídas.

### Reconexão Transparente (Silent Reconnect)
Procedimento automatizado no cliente que, ao detectar perda de sinal (ex: mudança de Wi-Fi para 4G/5G), reestabelece a sessão WebRTC em segundo plano sem intervenção manual, recuperando o buffer de saída do terminal sem perda de dados ou estado.

### Notificação de Término de Tarefa
Alerta emitido localmente no dispositivo móvel quando um comando demorado em execução em segundo plano finaliza seu processamento e retorna o controle ao prompt do shell.

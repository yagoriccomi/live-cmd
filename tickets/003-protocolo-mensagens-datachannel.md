# Ticket 003: Protocolo de Mensagens e Multiplexação DataChannel (Terminal vs Arquivos)

**Labels:** `wayfinder:grilling`, `protocol`, `p2p`  
**Status:** Fechado (Resolvido)  
**Parent:** [Mapa](mapa.md)

## Pergunta

Como estruturar o protocolo de dados sobre WebRTC DataChannel para separar de forma limpa o tráfego de entrada/saída do terminal (baixa latência, não bufferizado) da transferência de arquivos binários (alto volume, fragmentado em chunks, com controle de fluxo para não travar a digitação do usuário)? Devemos usar DataChannels separados (um canal `pty` e um canal `files`) ou um envelope binário unificado?

## Resolução

- **Canais Separados:** Utilização de dois WebRTC DataChannels dedicados independentes:
  1. `pty-channel`: tráfego direto de texto/bytes do terminal PTY com latência mínima e prioridade de entrega.
  2. `file-channel`: tráfego de blocos binários de arquivos em segundo plano com controle de fluxo de envio.
- **Transferência Resumível (Chunking estilo Torrent):**
  - Fatiamento em chunks de 64 KB com índice, hash SHA-256 por bloco e identificador de arquivo.
  - O Host e o Mobile mantêm controle de blocos recebidos (bitfield/offset). Se a conexão for interrompida, a transmissão retoma exatamente do bloco que faltava.
  - **Expiração:** Arquivos parciais que não forem concluídos no período de 24 horas são automaticamente excluídos do diretório de staging do Host para liberar espaço.

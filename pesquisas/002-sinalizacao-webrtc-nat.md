# Pesquisa Técnica: Servidor de Sinalização e NAT Traversal (Ticket 002)

**Status:** Concluído (Veredito: Cloudflare Workers + Durable Objects + Cloudflare Realtime TURN)

## 1. Resumo Executivo e Recomendação
- **Sinalização:** Cloudflare Workers com Durable Objects e WebSocket Hibernation ($0,00/mês no plano Free).
- **STUN:** Público híbrido (`stun.cloudflare.com:3478` e `stun.l.google.com:19302`).
- **TURN:** Cloudflare Realtime TURN (1.000 GB/mês grátis, suporte a turns:...:443?transport=tcp para furar CGNAT 4G/5G).

## 2. Viabilidade e Performance
- O tráfego do terminal PTY consome ~5 a 50 KB/s; 1 TB/mês comporta mais de 100 operadores diários.
- Latência Anycast < 20 ms em mais de 300 data centers, sem servidores Linux para manter.

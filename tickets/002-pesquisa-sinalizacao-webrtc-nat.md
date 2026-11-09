# Ticket 002: Pesquisa de Servidor de Sinalização e Infraestrutura WebRTC NAT Traversal

**Labels:** `wayfinder:research`, `network`, `webrtc`  
**Status:** Fechado (Resolvido)  
**Parent:** [Mapa](mapa.md)

## Pergunta

Qual é a arquitetura mais enxuta, confiável e de baixo custo para o Servidor de Sinalização e NAT Traversal (ex: Cloudflare Workers + Durable Objects/WebSockets vs servidor leve em Go/Node hospedado em VPS gratuita/econômica), incluindo a estratégia de servidores STUN/TURN públicos para garantir conexão mesmo em redes 4G/5G com CGNAT?

## Resolução

**Veredito:** **Cloudflare Workers + Durable Objects + Cloudflare Realtime TURN**.
- Relatório técnico detalhado: [pesquisas/002-sinalizacao-webrtc-nat.md](../pesquisas/002-sinalizacao-webrtc-nat.md).
- **Sinalização Serverless:** Cloudflare Workers com Durable Objects usando WebSocket Hibernation ($0,00/mês no plano Free).
- **NAT Traversal:** STUN público híbrido (`stun.cloudflare.com:3478` + `stun.l.google.com:19302`) com fallback incondicional para Cloudflare Realtime TURN (1.000 GB/mês grátis, porta 443 TLS) para contornar CGNAT simétrico de redes celulares 4G/5G.
- Latência Anycast < 20 ms globalmente sem necessidade de manter instâncias Linux.

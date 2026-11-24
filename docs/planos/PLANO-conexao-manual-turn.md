# Plano — Conexão manual e TURN (v0.2.2)

**Problema:** se o servidor de sinalização (Cloudflare) cai, os PCs não se apresentam; atrás de NAT simétrico o P2P direto falha.

**Resultado esperado:**
- Modo manual: PC A gera um código de convite, PC B cola e devolve um código de resposta, A cola e a conexão P2P abre, sem servidor.
- TURN configurável em Configurações (URLs, usuário, senha), gravado só no PC, somado ao STUN de `desktop/src/lib.rs:82`.

**Como validar:** teste Rust que conecta dois peers só com troca de códigos (sem sinalização); teste que a config TURN entra no `RTCConfiguration`; `cargo test` e E2E verdes.

## Premissas (modo loop)

- Código = SDP completo (ICE gathering concluído, sem trickle) em JSON, base64url, prefixo `lcm1.`. Leva a chave pública do host para a autenticação C2 já existente valer.
- TURN (dados reais, 2026-11-24): `turn:turn.yagoriccomi.com.br:3478?transport=udp`, `...:3478?transport=tcp`, `turns:turn.yagoriccomi.com.br:5349?transport=tcp`; só IPv4, sem 443. Credencial temporária (coturn `use-auth-secret`): `username = <vencimento unix s>:<serviço>`, `credential = base64(HMAC-SHA1(segredo, username))`, TTL 6 h, renovar a cada 1 h. O app guarda serviço + segredo em `turn.json` e gera a credencial a cada conexão. Serviço próprio `live-cmd` a cadastrar (`servico.sh cadastrar live-cmd`). Produção (decisão 2026-11-24): o Worker guarda `TURN_SECRET` (wrangler secret) e entrega `{urls, username, credential}` com TTL 6 h junto da apresentação; segredo nunca no binário. O app tem em Configurações um campo de caminho para um `.env` de TURN, que tem prioridade sobre o do Worker (dev, ou Worker fora). Config lida de `%APPDATA%\live-cmd\turn.env` (`TURN_URLS`, `TURN_SERVICE`, `TURN_SECRET`, `TURN_TTL_S`), já preenchido pelo usuário. Teste 4G só quando o mobile funcionar; até lá, opção temporária "Forçar TURN" em Configurações (`iceTransportPolicy: relay`) prova o relay entre os PCs; será removida depois.
- Entrega do Worker (decisão 2026-11-24): com `TURN_SECRET` definido, todo envelope `hello` (waiting e ready, para host e client) leva `payload.turn = {urls: string[], username, credential}`; campo extra que peer antigo ignora, sem tipo novo. Sem o secret, nada muda.
- Arquivo `%APPDATA%\live-cmd\turn.json`, fora do repositório; senha nunca no código nem no log.
- Release v0.2.2 só depois dos dados reais do TURN testados; push e release pedem o "pode publicar" do usuário.

## Fatias

| # | Fatia | Nível | Estado |
|---|---|---|---|
| 1 | TURN configurável (Rust + Configurações) | alto | em andamento |
| 2 | Conexão manual por códigos (Rust + UI) | alto | em andamento |
| 3 | v0.2.2: versão, `release.ps1` com gate E2E | — | aguarda dados TURN e autorização |

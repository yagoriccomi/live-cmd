# Ticket 004: Modelo de Segurança, Chaves Criptográficas e Pareamento por QR Code

**Labels:** `wayfinder:grilling`, `security`, `auth`  
**Status:** Fechado (Resolvido)  
**Parent:** [Mapa](mapa.md)

## Pergunta

Como deve ser estruturado o modelo de confiança e pareamento por QR Code (One-Time Key)? Quais dados devem ser embarcados no QR Code (ID da sessão de sinalização, chave temporária simétrica, fingerprint de chave pública), como expirar a autorização após o pareamento inicial e se o Host deve exigir uma confirmação humana no terminal do PC antes de autorizar o acesso remoto total.

## Resolução

- **Pareamento Inicial Instantâneo via QR Code:**
  - O Host exibe no terminal um QR Code contendo a URL de sinalização, o `sessionId` efêmero e uma One-Time Key (OTK) com validade de 5 minutos.
  - A leitura do QR Code pelo Cliente Mobile autentica a conexão inicial de imediato sem necessidade de intervenção adicional no teclado do PC.
- **Relação de Confiança Permanente (End-to-End Keys):**
  - Durante o handshake autenticado pelo OTK, o Host e o Cliente Mobile trocam chaves públicas permanentes (Ed25519) e estabelecem uma associação confiável mútua.
  - A partir desse momento, o celular pode se reconectar ao computador remotamente sem novo escaneamento de QR Code.
  - A conexão de confiança só é encerrada se o usuário revogar o dispositivo no Host (`host devices revoke <id>`) ou desinstalar o app mobile.
- **Proteção Biométrica Local:**
  - O aplicativo mobile inclui uma configuração opcional de bloqueio biométrico (Face ID / impressão digital), permitindo ao usuário escolher se exige ou não biometria ao abrir o app para interagir com o terminal.

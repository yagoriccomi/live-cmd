# PLANO DE EXECUÇÃO — live-cmd

| Campo | Valor |
|---|---|
| Tarefa | live-cmd: rota integral, contratos e cinco marcos de produto |
| Origem | Descrição livre; answers 001–003 e tickets 004, 006–009 |
| Tipo | História; pesquisa e planejamento nesta delegação |
| Modo | Loop; coordenador apenas orquestra |
| Data | 2026-11-06 |
| Branch | Nenhuma criada; checkout original E:/Projetos/live-cmd |
| Estado | Rota 1–16 autorizada em Loop; coordenador delega implementação; S1 com gate concorrência/teardown aprovado independentemente; D1 implementada, correção rtc, ajustes de encerramento/overflow e backpressure loopback com aceite restrito independente; D1/M1 integral NÃO aprovado; runner durável implementado e aprovado restritamente, somente runner (seção 8); mobile somente design, sem implementação/E2E; integração depende de D1, transporte dispositivo/ICE e validação integrada; C1 eco local sem shell e C3 somente nomes de canais para M1; demais contratos propostos |

## 1. Enunciado Canônico

- **Problema:** existem decisões e specs, mas ainda não há produto executável nem contratos interoperáveis completos.
- **Resultado esperado:** desktop Rust/Tauri, mobile React Native/Expo e signaling Worker/DO integrados, com pareamento autenticado, terminal persistente e arquivos resumíveis.
- **Como validar:** demonstrar eco, negar clientes não autorizados, operar terminal após reconexão, retomar arquivo de 100 MB e operar desktop no tray; distinguir testes locais de evidência em celular/Cloudflare.

### Checkpoint de planejamento

Complexa: cruza três módulos, múltiplos marcos e fronteiras de confiança. O planejamento inicial entregou somente um plano; agora a rota 1–16 está autorizada em Loop, com implementação delegada pelo coordenador. Esta atualização permanece exclusivamente documental. Superfície visual futura exige desenho antes de implementação. Contratos, credenciais externas, disponibilidade de dispositivos e limitações de segundo plano são dependências. Práticas: simplicidade [#7], escopo mínimo [#8], coesão [#13], validação [#51], testes de integração [#42], rollback [#84].

## 2. Escopo

**Fazer na execução futura:** cinco marcos na ordem eco P2P sem shell → pareamento autenticado → terminal → arquivos resumíveis → desktop híbrido/tray. Incluir abas, teclado auxiliar, reconexão, revogação, biometria opcional e notificações locais quando tecnicamente disponíveis.

**Não fazer nesta delegação:** código, scaffolding, instalação, execução de agentes, commit, push, PR, deploy, alterações pessoais ou presets. Sem Jira: tracker Markdown existente permanece canônico. Sem desktop remoto, contas na nuvem ou editor completo [#8].

A ordem de terminal/pareamento do ticket 009 fica substituída pela instrução atual: nenhum shell remoto antes da autenticação. Deploy solicitado pelo ticket 009 não está autorizado.

## 3. Premissas Assumidas

| # | Premissa | Justificativa | Se incorreta |
|---|---|---|---|
| P1 | Nível lógico alto para todas as fatias, usando agente disponível/modelo herdado | Não instalar presets pessoais; complexidade de segurança [#34] | Coordenador registra limitação, reduz concorrência; não simula capacidades |
| P2 | Windows primeiro; Linux/macOS e iOS exigem ambiente próprio | Máquina atual Windows; paridade verificável [#81] | Replanejar matriz sem declarar plataformas testadas |
| P3 | Expo development build, não Expo Go; versões compatíveis verificadas antes de fixar | WebRTC nativo e dependências da spec [#62] | Ajustar matriz Expo/RN/xterm sem upgrade arbitrário |
| P4 | Sem banco central; JSON protegido e staging local versionados | Spec existente; mínimo necessário [#7][#87] | Migrar apenas com necessidade demonstrada |
| P5 | SDKs, custos, franquias e metas de memória/latência dos documentos são hipóteses | Não há medição ou validação atual [#99] | Medir, registrar orçamento; não prometer custo zero |
| P6 | Autostart é opção explícita do usuário, no login da sessão | Tray não equivale a serviço anterior ao login [#55] | Serviço de sistema exigiria decisão separada |
| P7 | Fila de teclado não é descarregada cegamente após reconexão | Evitar duplicar comandos ou executar entrada antiga [#51] | Usar sequência/ACK e confirmação para entradas incertas |
| P8 | Rota nominal 1–16 reconciliada nesta revisão | Correção explícita do coordenador na seção 6 | Mudanças futuras exigem nova decisão registrada |
| P9 | Suspensão do mobile pode impedir conexão e notificações locais | Limites nativos de segundo plano [#98] | Registrar pendência de produto; não adicionar push remoto sem autorização |

## 4. Decisão Visual

- **Superfície visual:** sim, eco mobile, pareamento, terminais, arquivos, configurações e desktop/tray.
- **Mockup:** sim, telas novas; estados vazio, carregando, offline, bloqueado, revogado e erro devem existir.
- **Skill design-de-interface-projeto:** acionada; design mobile entregue somente como especificação de eco local, sem implementação. Demais superfícies continuam exigindo desenho antes de UI.
- **Referência:** fluxos do ticket 008, sem identidade visual definida. Prever contraste, foco, leitor de tela, alvos de toque e alternativa acessível ao Canvas.
- **Eco mobile especificado:** tela única “Eco local”, sem shell; estados desconectado, conectando, conectado e erro. “Conectado” somente com DataChannel aberto, não apenas WebSocket. Ações conectar/cancelar/desconectar, campo “Mensagem” e enviar; histórico limitado a 50 entradas em memória, uma mensagem pendente; preservar rascunho em erro, sem reenvio automático [#7][#8].
- **Acessibilidade e privacidade:** alvos de toque de 48 dp, fontes escaláveis, labels acessíveis, contraste e teclado sem cobrir envio; sem logs de conteúdo. Requisitos especificados, ainda não auditados em UI [#63].
- **Decisão do coordenador:** manter limite contratual de 16 KiB UTF-8 para mensagem de eco; proposta de 1 KiB não adotada. Timeout de eco de 10 s permanece proposta não aprovada; handshake de 30 s permanece [#51][#65].
- **Rede e arquivos candidatos:** ICE `127.0.0.1` aponta ao próprio Android, não ao PC; `adb reverse` encaminha TCP, não UDP. Alias `10.0.2.2` com adaptação de candidato é somente hipótese de teste, NÃO solução aprovada/provada; mudança de topologia exige decisão antes de executar, sem exposição pública. Candidatos `mobile/app/index.tsx`, `mobile/app/_layout.tsx` e `mobile/src/theme.ts` ainda NÃO criados; implementação/E2E mobile ausentes. Gate concorrência/teardown de S1 aprovado; PASS da correção rtc não aprova D1 integralmente. Integração depende do aceite integral de D1, transporte dispositivo/ICE e validação integrada; peers no PC não contam como E2E mobile.

## 5. Terreno

Todos os caminhos abaixo são relativos ao checkout original `E:/Projetos/live-cmd`.

| Arquivo/pasta | Papel | Ação futura |
|---|---|---|
| answers/2026_11_05 gerenciador-terminais-remoto 001.md | Escopo e topologia | Preservar |
| answers/2026_11_05 gerenciador-terminais-remoto 002.md | Stack, arquivos, confiança | Preservar |
| answers/2026_11_06 gerenciador-terminais-remoto 003.md | Cwd, ciclo de vida, desktop | Preservar |
| CONTEXT.md | Vocabulário | Reusar; esclarecer limites de replay e suspensão |
| tickets/004-modelo-seguranca-pareamento-qr.md | Confiança | Complementar após aprovação do contrato |
| tickets/006-spec-arquitetura-host-rust.md | Host | Aplicar módulos previstos |
| tickets/007-spec-protocolo-mensagens.md | Wire protocol incompleto | Consolidar contrato aprovado, sem duas fontes concorrentes |
| tickets/008-spec-app-mobile-react-native.md | Mobile | Aplicar fluxos após desenho |
| tickets/009-plano-implementacao-fatias.md | Marcos | Registrar nova ordem sem apagar histórico |
| tickets/mapa.md | Tracker Markdown | Atualizar evidências durante execução; fechado como spec não significa implementado |
| desktop/ | D1 implementada em `desktop/Cargo.toml`, `desktop/Cargo.lock` e `desktop/src/main.rs`; correção rtc, encerramento/overflow e backpressure loopback com aceite restrito independente, não aceite integral de D1/M1 | Runner durável `desktop/scripts/d1-integration.cjs` implementado e aprovado restritamente, somente runner; fechar gates D1 da seção 8; Tauri apenas no marco desktop |
| mobile/ | Somente design; implementação/E2E ausentes; SDK presente, emulador/system-images/AVD não encontrados | Decidir topologia antes de executar; Expo development build e cliente; validar transporte dispositivo/ICE e integração |
| signaling/ | S1 implementada; gate concorrência/teardown aprovado independentemente | 20 upgrades: 2 HTTP101 + 18 HTTP409; 23 testes e typecheck conferidos; não bloqueia mais por esse gate |

Reaproveitar decisões e bibliotecas escolhidas [#6]. No levantamento inicial não existia código de produto nos artefatos encontrados; estado atual recebido: S1 com gate concorrência/teardown aprovado, D1 implementada com aceite restrito independente da correção rtc, de encerramento/overflow e de backpressure loopback, aceite integral pendente, mobile somente design. Isso não comprova integração nem conclusão de M1. Separar core de UI; tipos explícitos, erros previsíveis e nomes canônicos [#5][#13]. Fontes adicionais lidas: README.md e handoff/gerenciador-terminais-remoto/001 - 06_11_2026 specs e roadmap de execucao.md.

**Diagnóstico de ambiente recebido (não revalidado nesta revisão):** Rust, MSVC, Node, Android SDK e WebView2 presentes. Ausência inicial de Expo/Wrangler locais é registro histórico; houve integrações Wrangler posteriores (seção 8), sem homologar a matriz mobile. Investigação confirmou SDK Android presente, mas emulador, system-images e AVD não encontrados. Compatibilidade do JDK25 pendente. Nenhuma instalação ou alteração de rede realizada na investigação; nenhum build executado nesta atualização documental. Builds desktop recebidos constam na seção 8; presença de ferramentas não comprova compatibilidade mobile [#81].

**Versões candidatas informadas para validação:** Wrangler `4.158.0`, Rust `webrtc` `0.21.1`, Expo `56.0.24`, React Native `0.85.3`, React `19.2.3`, `react-native-webrtc` `124.0.7`, plugin `15.0.0`. Não constituem matriz homologada; compatibilidade ainda depende de build, especialmente development build nativo mobile. Nenhuma instalação ou build nesta atualização [#62][#81].

## 6. Passos Atômicos e Pipeline

### Rota 1–16: reconciliada nesta revisão

Correspondência nominal corrigida conforme revisão do coordenador. Usuário autorizou a rota 1–16; coordenador apenas orquestra. A tabela registra a rota autorizada, não conclusão das etapas nem aprovação integral dos contratos. O planejamento anterior usou executar-projeto; esta revisão documental usa documentar-projeto, sem implementação.

| # | Skill | Mudança ou entrega | Verificação |
|---|---|---|---|
| 1 | criar-projeto | Criar somente estrutura necessária | Toolchain e builds mínimos |
| 2 | git-flow-projeto | Verificada/adaptada: Git existente com remoto GitHub; diagnóstico somente local | Evidências e limites abaixo; documentos untracked, não versionados |
| 3 | jira-projeto | Verificada/adaptada: integração remota dispensada; tracker Markdown deliberadamente mantido | tickets/mapa.md lido; sem prefixo Jira, Smart Commits ou migração |
| 4 | executar-projeto | Orquestrar marcos M1–M5 | Gates por marco abaixo |
| 5 | banco-de-dados-projeto | Condicional à persistência | Versionamento e integridade quando aplicável |
| 6 | refatorar-codigo-projeto | Condicional à necessidade demonstrada | Preservar comportamento; sem abstrações antecipadas [#8] |
| 7 | design-de-interface-projeto | Definir mockups e estados antes de UI | Revisão visual registrada |
| 8 | acessibilidade-projeto | Auditar toque, foco, leitura e Canvas | Evidência por plataforma |
| 9 | testes-projeto | Cobrir contratos e integração | Bateria reproduzível |
| 10 | otimizar-performance-projeto | Adiada sem gargalo | Otimizar somente após medição [#99] |
| 11 | revisar-projeto | Revisar módulos e integração | Achados tratados; evidências reais |
| 12 | seguranca-projeto | Auditar transporte, revogação e disco | Negativos passam; achados tratados |
| 13 | auditar-licencas-projeto | Registrar licenças e vulnerabilidades | Bloqueantes resolvidos |
| 14 | configurar-ci-cd-projeto | Preparar checks sem deploy ou push | Comandos locais passam; CI remota pendente |
| 15 | documentar-projeto | Registrar entrega, limitações e retomada | Pendências explícitas; contratos alinhados |
| 16 | rodar-projeto | Executar fluxos permitidos | Evidências por plataforma, não só build |

A rota não autoriza hooks pessoais, presets, integração Jira, commits, push, PR ou deploy. Etapas condicionais e adiadas preservam seus gates.

### Diagnóstico local das etapas 2 e 3 — 2026-11-06

Skills executadas na ordem `git-flow-projeto` e `jira-projeto`, com leitura das respectivas referências; adaptadas à autorização local e ao GitHub existente, sem setup Bitbucket [#7][#8]. Ownership desta atualização: somente este plano no checkout original; `signaling/` pertence ao agente implementador.

- **Etapa 2 — evidências:** `git --version` retornou `2.56.0.windows.2`; `git rev-parse --show-toplevel` confirmou `E:/Projetos/live-cmd`; `git remote -v` mostrou `origin git@github.com:yagoriccomi/live-cmd.git` para fetch/push; último commit local `5e9c313 Initial commit`. Nenhum acesso remoto/SSH foi testado.
- **Árvore e versionamento:** `git status --short` mostrou `.gitignore`, `CONTEXT.md`, `README.md`, `answers/`, `docs/`, `handoff/`, `pesquisas/`, `signaling/` e `tickets/` como untracked. `git ls-files docs tickets README.md CONTEXT.md .gitignore` não retornou arquivos. Documentos, tracker e este plano não estão versionados; repositório existente não significa versionamento pronto [#31][#37].
- **Proteção local:** `.gitignore` lido cobre `.env`, `.env.*`, `.dev.vars`, dependências e builds, com exceções para exemplos; não contém regras genéricas `*.pem`/`*.key`. Isso não comprova ausência de segredos: revisão antes de qualquer staging permanece necessária. Nenhuma alteração no ignore, configuração Git ou hooks; necessidade de hooks não validada nesta tarefa [#37][#40][#49].
- **Etapa 3 — evidências:** `tickets/mapa.md` declara `Tracker: markdown local (tickets/)`; seus itens concluídos são especificações, não implementação validada. Preservados tracker e IDs locais, sem prefixo Jira nem Smart Commits. Marcos M1–M5 e fila abaixo orientam execução; gates de testes continuam pendentes, sem alegar proteção remota de branch ou CI funcionando [#7][#49][#76][#98].
- **Limites:** `CLAUDE.md` não encontrado na raiz; não criado nem alterado. Limites daquela adaptação: sem Jira remoto, commits, push, PR, git config, hooks pessoais, instalação global ou delegação. Na orquestração atual em Loop, o coordenador delega implementação; demais restrições preservadas. Nenhuma variável remota a cadastrar nesta adaptação. Convenções futuras, apenas quando autorizadas: commits atômicos Conventional Commits, branches curtas `feature/` ou `bugfix/`, sem chave Jira [#31][#32][#33][#36].

### Passos de produto [#31][#42][#51]

| Passo | Alvo | Mudança lógica | Verificar |
|---|---|---|---|
| C0 | ticket 007 | Registrar C1 para eco local e nomes de canais C3 para M1; especificar e aprovar demais contratos antes dos respectivos marcos | Vetores Rust/TS e rejeições conforme contrato de cada marco |
| M1.1 | signaling/ | Encaminhar offer/answer/ICE em salas limitadas | Isolamento e expiração local |
| M1.2 | desktop/ | Cliente CLI WebRTC de eco, sem acesso a shell/disco | Eco e desconexão |
| M1.3 | mobile/ | Development build com UI de eco desenhada | Eco bidirecional no aparelho |
| M2.1 | desktop/ | Identidade protegida, OTK, autenticação e revogação | Replay/MITM/expiração recusados |
| M2.2 | mobile/ | QR, pinning, identidade protegida e biometria | Parear e reconectar sem QR |
| M2.3 | signaling/ | Admission/rate limits e emissão TURN autorizada | Negar abuso sem revelar segredos |
| M3.1 | desktop/ | PTY, sessões, resize, replay e TTL | ConPTY real; testes com relógio controlado |
| M3.2 | mobile/ | xterm local, abas, teclado e reconexão | UTF-8, Ctrl+C, IME e resize |
| M3.3 | desktop/ e mobile/, fatias distintas | Evento de conclusão de comando via integração de shell | Não confundir fim de shell com fim de comando |
| M4.1 | desktop/ | Staging, chunks duráveis, resume e finalização | Queda/restart/hash/expurgo |
| M4.2 | mobile/ | Upload/download, seletor e progresso | Arquivo de 100 MB retoma |
| M5.1 | desktop/ | Tauri, tray, cliente local/remoto e opt-in autostart | Reinício/login e revogação |
| M5.2 | três módulos, revisões separadas | Integrar e registrar evidências | Matriz local/externa completa |

## 7. Impacto em Dados e Contratos

**Decisão do coordenador nesta revisão:** C1 aprovado exclusivamente para eco local sem shell; C3 aprovado somente quanto aos nomes `pty-channel` e `file-channel` para M1. C2, C3 completo (incluindo propriedades dos canais, framing e sessões) e C4 continuam propostas, aguardando especificação técnica e aprovação antes de M2, M3 e M4, respectivamente. Não há aprovação integral dos contratos. Recursos de C1 além do eco local não estão liberados por esta decisão. Os detalhes abaixo não representam compatibilidade já implementada [#28][#51].

**Atualização 2026-11-08:** C2, C3 e C4 especificados e aprovados (answers 012); implementação continua dependendo de liberação explícita e dos gates por marco.

### C1 — Sinalização e negociação

- WS `/v1/ws/{room_id}`; WSS fora de loopback. Sala aleatória, efêmera, até dois peers por conexão; host mantém registro de presença para reconexão. `room_id` não autoriza shell. Autoridade de identidade é o endpoint, não o Worker.
- Envelope JSON `{v:1,type,connection_id,request_id,payload}`; tipos hello, offer, answer, ice, error, close. Mobile é offerer; host answerer. Cada tentativa usa novo connection_id aleatório de 128 bits. ICE pertence à tentativa; mensagens antigas são descartadas.
- Limites propostos: JSON/SDP 64 KiB, ICE 4 KiB, até 256 candidatos/tentativa, handshake 30 s; mensagens inválidas encerram a tentativa. Rate limit por conexão/identidade/IP com limites configuráveis e teste de abuso. Admission do Worker não substitui autenticação P2P.
- TURN por credencial efêmera emitida em endpoint autenticado, segredo de provedor apenas no Worker. M1 usa ambiente de teste sem shell; sem endpoint público de emissão anônima. Prever reconexão/presença após hibernação.

### C2 — Identidade vinculada ao transporte

- QR preserva URI da spec, com versão, host_pk, server permitido, session/room aleatório e OTK CSPRNG de 256 bits, validade máxima de 300 s. QR é segredo de autorização; não vai para logs, analytics ou Worker. Quem possui o QR pode parear durante a janela.
- Host valida sua própria expiração com relógio monotônico. OTK é consumida atomicamente apenas ao concluir prova válida; tentativas concorrentes só permitem um pareamento. Uma chave Ed25519, isoladamente, não autentica um canal WebRTC.
- Antes de qualquer operação privilegiada, handshake no canal de controle dentro de `pty-channel`: nonces novos de 256 bits de ambos os lados, identidade de ambos, papéis, versão, connection_id e fingerprints SHA-256 dos certificados DTLS efetivamente negociados entram no transcript.
- Codificação assinada: campos em ordem fixa, bytes com comprimento u32 big-endian, domínio `live-cmd/auth/v1`; nenhum JSON não canônico. Host e cliente assinam o transcript com Ed25519. No primeiro pareamento, ambos acrescentam HMAC-SHA256 com a OTK e domínios separados por papel. Cliente fixa host_pk lida do QR; reconexão exige chaves previamente pareadas dos dois lados.
- Fingerprints devem ser obtidos/verificados contra o transporte real pela biblioteca, não apenas aceitos do SDP recebido. Se os bindings não permitirem essa garantia, M2 fica bloqueado para revisão de alternativa autenticada; não liberar shell usando somente assinaturas desconectadas do canal.
- Viabilidade do binding DTLS (pesquisa estática, sem compilar nem executar): **desktop viável** com `webrtc =0.21.1`. Certificado remoto via `DtlsTransport::get_remote_certificates()` (`webrtc-0.21.1/src/peer_connection/transport/mod.rs:251`), somente após o DTLS chegar a `Connected`; o SHA-256 de `certs[0]` é calculado pelo host. Fingerprint local via `RTCCertificate::get_fingerprints` (`rtc-0.21.1/src/peer_connection/certificate/mod.rs:693`), usando certificado gerado pelo host e passado na configuração. A biblioteca compara o certificado com o `a=fingerprint` do SDP (`rtc-0.21.1/src/peer_connection/transport/dtls/mod.rs:203-235`, aceita só sha-256); `with_disable_certificate_fingerprint_verification` nunca deve ser usado. Proibido trocar local por remoto: o transcript precisa de testes negativos para isso. Atualizar a versão fixada exige nova revisão, porque a API mudou entre 0.17 e 0.20. **Mobile provavelmente viável (pesquisa externa estática, react-native-webrtc 155.0.1 master):** Android (`StringUtils.statsToJSON`) e iOS (`statsToJSON`) serializam `getStats()` inteiro; o libwebrtc gera entradas `certificate` com `fingerprint`, `fingerprintAlgorithm` e `base64Certificate`. Caminho: depois de `connectionState==='connected'`, ler `transport.localCertificateId`/`remoteCertificateId`, usar só o certificado folha e recalcular o SHA-256 a partir do DER. Não existem `RTCDtlsTransport` nem `getRemoteCertificates`. `generateCertificate` e `certificates` são suportados, mas o Android cai no certificado padrão sem erro quando o ID é desconhecido: comparar o fingerprint gerado com o local dos stats. Não confirmado: atribuição dos IDs no transport e rejeição de mismatch pelo libwebrtc, ambos sem leitura de código. O transcript ordena fingerprints por papel (offerer/answerer), não por local/remoto. Alternativa, se os stats falharem: assinar os `a=fingerprint` dos dois SDPs, mas só depois de provar que o mismatch é rejeitado. Liberado especificar C2; aceite de M2 exige teste em dispositivo real Android e iOS, incluindo os negativos de troca local/remoto e certificado divergente.
- Estados: NEW → CHALLENGED → AUTHENTICATED ou CLOSED. Timeout 30 s, nonce consumido uma vez, tentativa nova em toda reconexão, rejeição de versão/papel/identidade divergentes. Não repetir desafio após reinício.
- Trust store persistida atomicamente e protegida pelo mecanismo nativo do SO; chaves privadas nunca em JSON em claro. Revogar remove autorização, fecha conexões ativas, cancela acesso a PTYs/transferências e impede resume. Exclusão do app não garante revogação no host; oferecer revogação explícita. Biometria controla desbloqueio local, não substitui autenticação remota.

### C3 — Canais, framing e sessões

- Dois DataChannels confiáveis e ordenados: `pty-channel` e `file-channel`; sem limite de retransmissão. Separação não garante prioridade de rede: filas limitadas e backpressure são obrigatórios.
- Texto WebRTC transporta controles JSON versionados; binário transporta frames. Limite de controle 64 KiB, saída PTY dividida em payloads de até 16 KiB. Negociar tamanho máximo real de mensagem.
- Frame PTY v1 proposto: `version:u8 | opcode:u8 | session_id:16 bytes | sequence:u64 BE | payload_len:u32 BE | payload`. Preservar opcodes 01 input, 02 output, 03 resize, 04 heartbeat, 05 evento de término; distinguir `command_completed` de `session_exited` nos controles. Resize: cols/rows u16 BE, ambos 1–1000. Bytes PTY não devem ser decodificados prematuramente como strings UTF-8.
- Controles: session_create/list/attach/close, input_ack, replay_begin/end/gap, cwd_changed, error. IDs completos UUID; erros estáveis INVALID_MESSAGE, UNAUTHORIZED, REVOKED, NOT_FOUND, LIMIT_EXCEEDED, EXPIRED, HASH_MISMATCH, IO_ERROR; request_id correlaciona respostas sem detalhes sensíveis.
- Host autoriza sessão por dispositivo, não só UUID. Input tem sequência por sessão/dispositivo e deduplicação; ACK significa aceitação no PTY, não execução do comando. Entrada incerta não é automaticamente reenviada após perda de estado.
- Saída tem offset monotônico; attach informa último offset confirmado. Buffer até 10.000 linhas com teto adicional proposto de 8 MiB/sessão; quando faltar histórico, informar gap e restaurar snapshot/estado coerente. Replay bruto truncado não garante reconstrução de aplicações de tela cheia: demonstrar restauração antes de alegar recuperação total.
- TTL de sessão: 24 h sem clientes conectados e sem I/O, usando relógio controlável em testes. Desconectar não encerra PTY. Persistência prometida atravessa queda de rede, não reinício do host.
- Cwd e conclusão de comando requerem integração explícita de shell; não inferir por regex do prompt nem confundir processo PTY com comando filho. Sem suporte, mostrar cwd desconhecido e pedir destino; não afirmar sucesso de notificação.

### C4 — Arquivos e retomada

- file_init inclui v, request_id, file_id UUID completo, session_id, direção, filename, size_bytes decimal string, chunk_size=65536, total_chunks, sha256 total e target_dir opcional. Inteiros grandes não usam Number JS. Validar consistência; u32 de índice limita capacidade a 2^32 chunks, não prometer tamanho ilimitado.
- Frame chunk: `version:u8 | file_id:16 | chunk_index:u32 BE | payload_len:u32 BE | sha256:32 | data`. Não usar UUID truncado de 8 bytes. Último bloco tem tamanho exato; arquivo vazio tem zero chunks e hash do vazio.
- Chunk lógico continua 64 KiB. Se cabeçalho+payload exceder máximo SCTP negociado, fragmentar transporte com índice/offset/comprimento e reassembly limitado; nunca assumir que 65536 bytes mais cabeçalho cabem.
- ACK somente após validação de hash, gravação durável e atualização atômica do mapa. ACK duplicado é idempotente; duplicata com conteúdo divergente falha. Manifesto e ownership persistem após reinício.
- Resume vincula file_id, proprietário, manifesto/hash e direção; retorna intervalos de chunks ausentes paginados, não lista sem limite. Cliente revogado não consulta nem retoma staging.
- Finalizar após hash total, reservar destino e substituir atomicamente somente com consentimento explícito para overwrite; se outro volume, staging final no volume de destino. Falha/disco cheio preserva original e parcial recuperável. Expurgo após 24 h sem chunk válido, restrito ao staging gerenciado.
- Navegação livre significa permissões da conta do host, não privilégio administrativo. Validar filename, caminhos absolutos escolhidos, symlinks/junctions, troca de destino e corridas; nunca concatenar entrada não validada. Aplicar cotas configuráveis, limite de concorrência e backpressure; terminal permanece responsivo.

### Dados, configuração e privacidade

Sem banco central. Trust store e manifestos têm schema_version e escrita atômica [#87]. Configuração de URL de signaling, STUN/TURN, TTLs e quotas separada de código [#80]. Credenciais reais não entram no repo [#37]. Logs limitados a IDs de correlação, estados e erros saneados; não registrar chaves, OTK, SDP integral, comandos, conteúdo de arquivos ou caminhos pessoais [#63]. Worker conhece metadados de conexão; TURN pode retransmitir tráfego cifrado, portanto P2P não significa sempre conexão direta.

## 8. Plano de Testes

| Nível | Cobertura local | Falhas |
|---|---|---|
| Unidade [#41] | Vetores Rust/TS de framing, transcript, TTL, hashes, limites | Truncamento, overflow, endian, UTF-8 fragmentado, tamanho inválido |
| Segurança | Handshake e trust store com fixtures | OTK expirada/reutilizada, corrida, replay, identidade trocada, fingerprint MITM, revogação ativa |
| Integração [#42] | Worker/DO local e peers locais; ConPTY; arquivos temporários | Sala cruzada, queda WS, restart, disco cheio, hash inválido, symlink/junction, ACK perdido |
| E2E local [#43] | Eco desktop; terminal; resume; tray manual | Shell inacessível antes de auth, sessão indevida negada, overwrite recusado |
| E2E externo | Celular físico, Wi-Fi/4G, TURN TLS 443, cloud real autorizada | CGNAT, suspensão, câmera, IME, biometria, teclado e permissões |

**Estado de validação confirmado pela coordenação, não reexecutado nesta atualização:** gate concorrência/teardown de S1 **aprovado independentemente**: 20 upgrades produziram 2 HTTP101 + 18 HTTP409; 23 testes e typecheck conferidos. `ECONNRESET` foi isolado no harness e corrigido na gestão dos sockets rejeitados, não ignorado. O bloqueio anterior desse gate foi resolvido; integração e M1 continuam pendentes [#42][#49].

**D1 — histórico do PASS independente da correção rtc; aceite integral pendente:** implementação em `desktop/Cargo.toml`, `desktop/Cargo.lock` e `desktop/src/main.rs`. A verificação identificou `rtc 0.21.1` descartando metadados de confiabilidade DCEP; patch Git upstream SHA `70901c3df6ad92dcb87d97af3d1e2fa8a2e2ae38`, mantendo `webrtc=0.21.1` e a guarda host, recebeu PASS independente do agente `aeac6432e7f63e1d0`. Coordenação repetiu build/test/clippy com `--locked --offline`: exit 0, 2 testes aprovados e 8 ignored. O verificador executou separadamente as 8 integrações Wrangler e probes próprios: vazio, Unicode/NUL, repetidos, 16 KiB, retransmits/lifetime 0/1/65535, unordered, label com invisível, segundo canal, binário e 16385 bytes. Evidências recebidas, não reexecutadas nesta atualização; aprovação restrita à correção, **NÃO D1/M1 integral** [#34][#42][#62].

**D1 — aceite restrito de encerramento/overflow:** ajustes de `ab3cc8b66ac594b5d` em `desktop/src/main.rs` verificados independentemente pelo agente `a028222383a081281`; coordenação reconferiu build/probe/integration no log `C:/Users/yagor/AppData/Local/Temp/d1-a42954b9-verify/a0282223-verification.log`, linhas 89–176. Build/test/clippy `--locked --offline` aprovados: 3 testes passaram, 8 ignored na bateria padrão; `handler_overflow` cobre fila 260/261, não pressão real de buffer. Probe do bloco de cleanup com matriz de 6 casos de close **SIMULADO** aprovado: sucesso/erro/timeout de close com e sem erro primário; propaga falha de `pc.close` quando resultado primário é `Ok`, preserva erro primário. `wrangler_echo` real executado separadamente e aprovado, exigindo conclusão cooperativa do host; não equivale a Ctrl+C real. Evidência lida, testes não reexecutados nesta atualização [#34][#42][#96].

**D1 — aceite restrito do runner durável:** `desktop/scripts/d1-integration.cjs` implementado por `a51b6dc4bcfe7a058`, com PASS restrito do verificador `af9270c34ea97b905`; **aprovado somente o runner**, não D1/M1 integral. Evidência independente: `C:/Users/yagor/AppData/Local/Temp/d1-af9270c3-verification/evidence.log`. Coordenação reconferiu sintaxe/self-test, `probes.cjs` (portas ocupadas, entradas inválidas, UTF-8, captura, zero testes, falhas de spawn/worker/deadlines e cleanup) e oito integrações reais em cópia TEMP: **PASS 8/8**. A primeira repetição da integração recusou a porta 18849 ocupada; diagnóstico posterior sem listener, repetição aprovada, sem encerrar processo alheio. Validação isolada, somente Windows; não comprova Ctrl+C físico, backpressure real nem mobile. Evidências recebidas da coordenação, não reexecutadas nesta atualização documental [#34][#42][#96].

**Reprodução do runner:** na raiz do checkout preparado, `node desktop/scripts/d1-integration.cjs`; self-test: `node desktop/scripts/d1-integration.cjs --self-test`. Requer dependências e caches disponíveis, toolchain Rust/MSVC no Windows e linker configurado via ambiente quando necessário (`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`); caminho pessoal não é requisito universal. A aprovação acima corresponde à execução isolada em cópia TEMP, não a nova execução no checkout original [#42][#80][#96].

**D1 — aceite restrito de backpressure loopback:** teste `tests::loopback_backpressure` em `desktop/src/main.rs:176–328`, com **PASS** independente dos verificadores `a1049afa6592ee02e` e `aafcb9d5cbcb57140`. Coordenação reconferiu `cargo build --manifest-path desktop/Cargo.toml --locked --offline`, `cargo test --manifest-path desktop/Cargo.toml --locked --offline -- --nocapture` e `cargo clippy --manifest-path desktop/Cargo.toml --locked --offline --all-targets -- -D warnings`: build/Clippy aprovados; Cargo não mostrou stdout da suíte. A execução direta do binário TEMP confirmou **4 passed / 0 failed / 8 ignored**; probe Python com três processos concorrentes confirmou **1 passed** e `saldo_final=0` em cada processo. Evidências em `C:/Users/yagor/AppData/Local/Temp/d1-a1049afa-verification`, recebidas da coordenação, não reexecutadas nesta atualização documental [#34][#42].

**Métricas da reconferência da suíte:** 579 mensagens enviadas + 1 pendente; saldo de buffer 49129 bytes; bloqueio de 2.0065039 s; a mesma future retomada em 1.2183291 s; 580 mensagens recebidas íntegras e ordenadas, payload total 9519104 bytes; saldo final 0; duração 4.9925929 s. Pressão de buffer e retomada observadas **somente em loopback**: não comprova timeout dentro de `run()`, RSS nem cleanup em falhas com injeção dinâmica (cleanup avaliado somente estruturalmente). Não estende o aceite histórico do runner nem do overflow unitário [#42][#99].

**Reprodução do backpressure:** na raiz do checkout preparado, `cargo test --manifest-path desktop/Cargo.toml --locked --offline tests::loopback_backpressure -- --exact --nocapture`. Requer caches/dependências e toolchain Rust/MSVC disponíveis, linker configurado via ambiente quando necessário, como na reprodução do runner [#80][#96].

**D1 — gates restantes:** **D1/M1 integral NÃO aprovado**. Ctrl+C físico no Windows permanece pendente; pressão de buffer e retomada aprovadas restritamente em loopback, com as limitações acima; runner durável aprovado somente no seu escopo. Mobile permanece somente design, sem implementação/E2E; transporte dispositivo/ICE e validação integrada pendentes. Integração entre peers no PC não conta como E2E mobile [#34][#42][#43].

**Comandos do planejamento inicial, sem nova verificação de disponibilidade ou execução nesta atualização:** `cargo test --manifest-path desktop/Cargo.toml`, `cargo clippy --manifest-path desktop/Cargo.toml -- -D warnings`, `npm --prefix mobile test`, `npm --prefix mobile run typecheck`, `npm --prefix signaling test`, `npm --prefix signaling run typecheck`. Agentes devem definir scripts reais e registrar saída/versões, sem inventar resultados.

**Gates por marco:** M1 eco sem shell; M2 negativos de segurança e pinning de transporte; M3 terminal autenticado com replay e não duplicação; M4 upload/download de 100 MB com interrupção, hash igual e baixo impacto no terminal; M5 tray/cliente/autostart opt-in. Metas de latência são medições, não garantia de 1,5 s.

## 9. Riscos e Rollback

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Binding WebRTC não expõe transporte suficiente | Média | Identidade não vinculada | Bloquear M2; prova técnica/revisão antes de shell |
| Dependências nativas incompatíveis | Média | Mobile sem build | Matriz de versões pequena e spike local |
| Replay truncado não restaura tela | Alta | Contexto visual incorreto | Offset/gap/snapshot, ensaio com aplicação interativa |
| Suspensão bloqueia notificação | Alta | Promessa impossível no celular | Teste físico, limitação explícita; nada de push extra implícito |
| Arquivos grandes exaurem disco/memória | Média | Perda de dados | Streaming, quota, ACK durável, finalização segura |
| Agentes editam contrato simultaneamente | Média | Incompatibilidade | Coordenador congela contrato; revisão única |
| Cloud/custos não disponíveis | Média | Sem prova NAT/TURN | Continuar local; gate externo pendente |

**Rollback [#84]:** o planejamento original entregou somente um documento novo; esta atualização altera apenas o plano existente, sem modificar código, contratos ou configuração. Para revertê-la, desfazer somente seus trechos documentais, preservando alterações alheias. Na execução, manter mudanças por módulo, checkpoints/diffs revisados, sem comandos destrutivos. Não sobrescrever trabalho alheio. Desligar recursos por configuração local; revogar acesso e encerrar canais se houver incidente. Preservar arquivos originais e staging. Nenhum rollback de deploy é aplicável sem deploy autorizado. Alterações de autostart reais exigem consentimento e remoção reversível da entrada criada pelo aplicativo.

## 10. Definição de Pronto

### Esta delegação
- [x] Ler template, práticas, answers, specs, glossário e tracker.
- [x] Escrever plano com premissas, contratos e fatias.
- [x] Separar evidência local de validação externa.
- [x] Rota nominal 1–16 reconciliada nesta revisão.
- [x] Decisão do coordenador registrada: C1 para eco local sem shell; C3 somente nomes de canais para M1.
- [ ] Especificar tecnicamente e aprovar C2, C3 completo e C4 antes dos respectivos marcos.

### Produto, execução parcial; gates pendentes
- [ ] Todos os passos e gates M1–M5 atendidos.
- [ ] Design executado antes de UI; acessibilidade revisada.
- [ ] Build, testes novos/existentes e negativos de segurança passam [#49].
- [ ] Aplicação realmente operada; evidências por plataforma registradas.
- [ ] Zero segredos no código; logs saneados e erros tratados [#37][#92][#93].
- [ ] Celular/Cloudflare/TURN validados ou explicitamente pendentes, sem chamar produto completo.
- [ ] Entrega futura em docs/planos/ENTREGA-live-cmd.md, somente ao concluir revisão.
- [ ] Tracker Markdown atualizado; CONTEXT/specs alinhados com contratos aprovados.
- Commits, PRs e Jira: não aplicáveis à autorização atual; não criar para satisfazer template.
- CLAUDE.md: sem alteração pessoal; regra de projeto só se necessária e autorizada.

## 11. Fatias e Fila do Coordenador

Todas as fatias têm nível lógico **alto**, agente disponível/modelo herdado; sem presets instalados. Branch **não criada** em todas; worktrees somente quando permitidas pelo coordenador. Sem lançamentos nesta delegação. Cada linha é uma tarefa de módulo único, com escopo e aceite na seção 6. Integração é gate orquestrado, não fatia que edita três módulos.

| Fatia | Módulo/ownership | Depende | Estado |
|---|---|---|---|
| S1 sinalização eco | signaling/ | C0 limitado a M1 | Gate concorrência/teardown aprovado independentemente: 20 upgrades, 2 HTTP101 + 18 HTTP409; 23 testes e typecheck conferidos; ECONNRESET corrigido no harness, não ignorado |
| D1 host eco | desktop/ | C0 limitado a M1; S1 para integração | Correção rtc com PASS independente por aeac6432e7f63e1d0; encerramento/overflow por ab3cc8b66ac594b5d com aceite restrito de a028222383a081281, reconferido pela coordenação. Backpressure loopback com aceite restrito de a1049afa6592ee02e e aafcb9d5cbcb57140, reconferido pela coordenação (seção 8). D1/M1 integral NÃO aprovado; runner durável implementado e aprovado restritamente, somente runner (seção 8); fechar gates da seção 8 |
| M1 mobile eco | mobile/ | C0 limitado a M1, design; S1 e D1 para integração | Somente design (seção 4); implementação/E2E ausentes; gate S1 resolvido. Decisão de topologia antes de executar; alias 10.0.2.2 é hipótese não aprovada/provada. Integração depende do aceite integral de D1, transporte dispositivo/ICE e validação integrada; peers no PC não contam como E2E mobile |
| D2 autenticação | desktop/ | Gate eco, C2 aprovado | Bloqueada |
| S2 admission/TURN | signaling/ | Gate eco, C2 aprovado | Bloqueada |
| M2 QR/identidade | mobile/ | Gate eco, C2 e design | Bloqueada |
| D3 PTY/replay/eventos | desktop/ | Gate autenticação | Bloqueada |
| M3 terminal/teclado | mobile/ | Gate autenticação, design | Bloqueada |
| D4 arquivos | desktop/ | Gate terminal, C4 | Bloqueada |
| M4 arquivos | mobile/ | Gate terminal, C4 e design | Bloqueada |
| D5 Tauri/cliente/tray | desktop/ | Gate arquivos, design desktop | Bloqueada |
| S3 revisão signaling | signaling/ | S2 | Bloqueada |
| M5 revisão mobile | mobile/ | M4 | Bloqueada |
| D6 revisão desktop | desktop/ | D5 | Bloqueada |

Critérios de lançamento: módulo único, dependências prontas, contrato congelado, comando de verificação e escopo exclusivo. Um agente por módulo por vez; módulos independentes podem avançar em paralelo dentro do mesmo marco. Arquivos comuns de documentação ficam sob coordenação para evitar colisões. Fila: pendente → em execução → entregue → revisão → pronta ou correção; nunca marcar pronta por relato sem evidência. Coordenador só orquestra, revisa e registra gates. Falta de ferramenta de agentes/uso deve ser registrada, não disfarçada como execução em segundo plano.

## 12. Próxima ação

Prioridade autorizada: corrigir execução oculta dos testes. Separar a entrada CLI do preload `desktop/scripts/hidden-processes.cjs`, preservando `NODE_OPTIONS`, stdout/stderr e códigos de saída; usar `--test-skip-pattern=^Miniflare` nas execuções restritas, sem excluir integração da suíte padrão. Regressão mínima com dois testes fictícios e sonda UTF-8/exit 23, inclusive preload herdado; revisão independente antes do aceite. Não executar Cargo, Wrangler ou Miniflare real nesta correção. Ocultação visual e descendentes nativos continuam não comprovados [#7][#42].

Aceite restrito da separação CLI/preload: `hidden-run.cjs` é a entrada CLI; `hidden-processes.cjs` permanece preload; `hidden-run.test.cjs` cobre a regressão. Verificador `ab9058d419cdba0ad`: PASS, reconferido pela coordenação nos três comandos do relatório, com saídas iguais: sintaxe/regressão/self-test D1; erros CLI 2/127/1; signaling restrito com 22 testes aprovados, zero falhas. UTF-8, canais e exit 23 preservados; suíte padrão mantém integração. Nenhum Cargo, Wrangler ou Miniflare real executado nesta validação. Tarefa de ocultação permanece aberta pelo limite visual/nativo; não liberar suítes pesadas automaticamente [#34][#42].

Com a rota 1–16 autorizada em Loop, coordenador delega implementação e registra gates; o gate concorrência/teardown de S1 está aprovado independentemente. Runner durável implementado e aprovado restritamente, somente runner (seção 8). Próximas fatias: fechar gate de Ctrl+C físico no Windows e demais pendências da seção 8, liberando somente dependências aprovadas. Pressão de buffer e retomada com aceite restrito em loopback pelos verificadores `a1049afa6592ee02e` e `aafcb9d5cbcb57140`, reconferidas pela coordenação; não comprovam timeout dentro de `run()`, RSS nem cleanup em falhas com injeção dinâmica. Correção rtc com PASS de `aeac6432e7f63e1d0`; encerramento/overflow com aceite restrito de `a028222383a081281`, evidência reconferida pela coordenação. Close SIMULADO e conclusão cooperativa de `wrangler_echo` não substituem Ctrl+C real; overflow unitário não comprova pressão de buffer. **D1/M1 integral NÃO aprovado** [#34][#42][#96]. Mobile somente design, sem implementação/E2E; decidir topologia antes de executar, sem tratar alias `10.0.2.2` com adaptação de candidato como solução aprovada/provada. Transporte dispositivo/ICE e validação integrada continuam pendentes; peers no PC não contam como E2E mobile, sem bloqueio pelo gate S1 já resolvido. Preparação de M1 permanece restrita ao eco local sem shell, C1 e nomes de canais C3 aprovados, limite de 16 KiB UTF-8; preservar decisões e requisitos da seção 4, sem implementar nem testar desktop/signaling/mobile nesta atualização documental. Antes de M2, especificar e aprovar C2 e determinar viabilidade do binding DTLS; antes de M3, especificar e aprovar C3 completo; antes de M4, especificar e aprovar C4. Sem shell, deploy, commit, push ou ações irreversíveis nesta autorização. Infra externa, custos, instalação pessoal e autostart real permanecem fora da execução automática autorizada.

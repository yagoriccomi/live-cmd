# Decisões — gerenciador-terminais-remoto — 2026-11-07

**Contexto:** revisão do rascunho `docs/planos/SPEC-C2-identidade.md` (autenticação QR/Ed25519 vinculada ao DTLS) e das perguntas abertas da seção 8.
**Modo:** levantar

| # | Peso | Pergunta | Decisão | Consequência |
|---|---|---|---|---|
| 1 | Bloqueante | Aprovar os 5 desvios D1–D5 em bloco? | Revisar um a um (itens 5–9) | Cada desvio decidido separadamente |
| 2 | Importante | Versão do `react-native-webrtc` | Fixar 155.0.1 | Atualizar exige nova revisão da obtenção de certificados |
| 3 | Importante | Bibliotecas de cripto e cofre no mobile | Pesquisar e recomendar | Agente compara opções; nada instalado sem aprovação |
| 4 | Importante | Servidores aceitos pelo QR (`server`) | Lista fixa no app | QR falso não redireciona; trocar servidor exige atualizar o app |
| 5 | Importante | D1: formato de `otk`/`host_pk` no QR | base64url sem padding | Sem `+ / =` no URI |
| 6 | Importante | D2: versão no QR | Incluir `v=1` | Versão desconhecida recusada com aviso |
| 7 | Importante | D3: `mode` e `room_id` no transcript | Incluir os dois | Prova não reaproveitável entre pareamento/reconexão ou salas |
| 8 | Detalhe | D4: códigos de erro | Uma lista só, mas completa | Lista única de erros para o app inteiro, cobrindo todos os casos de autenticação |
| 9 | Detalhe | D5: rótulos de papel no transcript | Renomear para neutro (`offerer`/`answerer`) | Rótulo vale para qualquer cliente; muda o tamanho do vetor de teste |

**Confirmações:** pareamento por QR efêmero com chaves Ed25519 permanentes (answers 002, 2026-11-05; 003, 2026-11-06) continua valendo.
**Premissas:** nenhuma.
**Ainda em aberto:** escolha concreta das bibliotecas do mobile (depende da pesquisa); confirmação em aparelho Android/iOS dos IDs de certificado e da rejeição de fingerprint divergente.

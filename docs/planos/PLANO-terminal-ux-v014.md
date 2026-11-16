# PLANO — terminal UX v0.1.4 (pendências das 20 sugestões)

Origem: auditoria de 2026-11-16 (10 feitos, 9 parciais, 1 falta, 2 decisões divergentes). Modo Loop: premissas registradas.

## Checklist

| # | Item | Fatia | Estado |
|---|---|---|---|
| D2c | Botão direito cola; Shift+botão direito / ☰ / Shift+F10 abre menu | G3 | feito |
| D4 | Nome da sessão persistido em disco no host | G1 | não feito por decisão: nome fica em memória, sincronizado entre PCs; a sessão morre com o host |
| 3 | Zoom de fonte salvo por PC | G3 | feito |
| 8/9/17/18 | cmd: OSC 133/7 via `PROMPT` ($E) → atividade, fim de comando, cwd, confirmar fechar | G1 | feito |
| 10 | Arrastar arquivo para sessão remota: envia o arquivo ao outro PC (local: cola caminho) | G3 | parcial: cola caminho. Cliente desktop (`desktop/src/client.rs`) só abre `pty-channel`; upload C4 exige `file-channel` + `file_upload_init` + frags com backpressure no cliente. Fica para v0.1.5 |
| 11 | Histórico de rolagem configurável na tela | G2 | feito |
| 12 | Exportar para `.txt` (Salvar como) e linhas quebradas unidas | G2 | feito |
| 14 | Qualidade da conexão: latência + direta/relay na lateral | G2 (Rust) + G3 (UI) | feito |
| 16 | Perfis valem sem reiniciar | G2 | feito |
| 19 | Atalhos configuráveis (F1 editável) | G3 | feito |
| Clear | "Limpar" funciona em cmd (limpa buffer vt100 no app) | G2 | feito |

## Contratos entre fatias
- G2 expõe `term_conn_stats({conn}) -> { rtt_ms: number|null, relay: bool|null }`; G3 consulta a cada 5 s e mostra na lateral.
- G2 expõe `term_clear({conn, sessionId})`; G3 usa em "Limpar".
- G2 é dono de `action === "export"` em terminal.js (só essa linha).

## Premissas (Loop)
- Atalhos configuráveis: ações fixas, tecla remapeável, salvo em localStorage.
- Item 10: usa transferência de arquivos já existente (C4); sem ela, fica caminho e registrado.

# Plano — Correção pós-teste v0.2.1

Fontes:
- Mapa: `tickets/mapa-correcao-v0.2.1.md`.
- Decisões: #58–#67, em `answers/2026_11_24 gerenciador-terminais-remoto 026.md`.

**Problema:** 10 dos 14 tickets (014–027) falharam no teste do usuário no app.

**Resultado esperado:**
- Todas as correções combinadas estão feitas.
- Um robô WebDriver testa o app real, e o `release.ps1` bloqueia se ele falhar.

**Como validar:** `desktop/app/e2e` passa com um cenário por critério de aceite, e o usuário testa ao fim.

## Regras

- Branch: `feat/mobile-basic`, com commits atômicos. Sem push, sem release e sem merge em `main`.
- Não mexer no mobile.
- Matar processo só por PID.
- `.rs` e `.js` em `app/` usam CRLF.
- Nunca declarar pronto sem rodar o app. Cada correção precisa de um print ou de um cenário E2E.

## Fatias

| # | Fatia | Nível | Estado |
|---|---|---|---|
| A | Robô E2E (tauri-driver + WebDriver) em `desktop/app/e2e/`, com gate no `release.ps1` e cenários do arrastar (cards e guias), badge e Ctrl+Home | alto | feito: robô + gate; cenários 001, 017, 021 (badge e Ctrl+Home ficam com B2 e B5/B6) |
| B1 | M2: `.disable_drag_drop_handler()` em `main.rs:275`. Ordem e grupos de cards (017). Arrastar guia para dividir (021) | alto | feito: E2E passa (arrasto por ponteiro; layout salvo não era lido ao abrir) |
| B2 | Badges: clique no badge do card abre a lista. Os badges saem do menu de contexto. O badge segue a sessão ao inverter ou mover (#60, #61) | alto | feito: espaço do badge no card abre a lista; cenário 060 |
| B3 | 016: coluna única visual. O hover abre o painel do item sobre o terminal. QR navega. O vão não fecha o hover (#62) | alto | feito: trilho e lista numa coluna; hover em Agentes/SSH/Histórico abre o painel, clique fixa, QR/Dispositivos/Ajustes só no clique; cenário 062 |
| B4 | 018: shells cadastrados e agentes num menu de novo chat com grupos (#63). 019: o padrão do host salva | alto | feito: menu com grupos Shells/Agentes; padrão falhava com INVALID_MESSAGE (pedido sem `api`); cenário 063 |
| B5 | 019/020: seletor nativo no local (`tauri-plugin-dialog`) e navegador Aurora no remoto (#64). Autocomplete com Tab, lista ao digitar, conteúdo sem `\` final e visual Aurora | alto | feito: nativo no local, navegador Aurora no remoto, autocomplete próprio; cenário 064 |
| B6 | 023: Shift+clique direito não destrói a seleção (#65). Exportar abre "Salvar como" e grava `.txt`: com seleção só o trecho, sem seleção o buffer sem repetição (#66) | alto | feito: menu sem mexer na seleção, exportar seleção ou buffer; cenário 065 |
| B7 | 027: seleção arrastando além da borda rola. Ctrl+Home vai ao início | alto | feito: arrasto rola via term_scroll e copia via term_lines; Ctrl+Home/End; cenário 066 |
| B8 | 014: o rename trava a seleção. 021: barra escura atrás das pílulas. 022: lista do F1 e contraste das teclas. 025: Configurações > Agentes com "Trocar imagem" e inicial colorida para todos (#67) | alto | aberto |

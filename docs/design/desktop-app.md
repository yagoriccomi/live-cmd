# Design D5 — App de bandeja do live-cmd (desktop)

**Status:** proposta de design (D5-design). Sem código de app. Mockup: [`desktop-app-mockup.html`](desktop-app-mockup.html). Abra o arquivo direto no navegador.

**Fontes:** `tickets/006-spec-arquitetura-host-rust.md` §2.5 (menu da bandeja), `answers/... 003.md` item 4 (híbrido: host + cliente desktop), `answers/... 017.md` (terminal invisível, modo silencioso), `docs/planos/PLANO-live-cmd.md` P6 (autostart opt-in no login), `docs/planos/SPEC-C2-identidade.md` §1, §2.2 e §6 (QR secreto, 300 s, uso único, revogação), `docs/planos/PLANO-M4-M5-desktop.md` (D5).

**Plataforma:** Tauri 2, frontend HTML + CSS em WebView2, sem framework. O mockup é o ponto de partida do frontend. Por isso usa HTML semântico e variáveis CSS.

## 0. Princípios

- **Discreto.** O app vive na bandeja. Nenhuma janela abre sozinha, nem quando o celular conecta ("modo silencioso", answers 017). Os terminais do host nunca abrem console visível (ConPTY oculto).
- **Poucas telas.** São 5 superfícies: bandeja, Pareamento, Aparelhos, Configurações e Terminal. Pareamento, Aparelhos e Configurações são **uma janela só com 3 abas**. Isso evita três janelas soltas [#7]. Cada item do menu abre a janela já na aba certa.
- **Zero falha silenciosa.** Toda chamada ao host tem estado de carregando, erro e vazio. O erro aparece em alerta inline com a mensagem amigável de C2 §2.2. O log local recebe só o código (`WARN`/`ERROR`), nunca o URI, a OTK ou um stack trace [#91][#92][#93].
- **Segredo não vaza.** O URI do QR nunca aparece em texto, log, tooltip ou leitor de tela. O QR fica dentro de um `<img>` com `alt` genérico ("QR Code de pareamento").

## 1. Tokens de tema

O app segue o tema do Windows via `prefers-color-scheme`. WebView2 repassa a configuração "Modo de app" do sistema. Nenhuma cor fica chumbada em componente: todas saem de variáveis [#3][#6].

| Token | Claro | Escuro | Uso |
|---|---|---|---|
| `--bg` | `#f3f3f3` | `#202020` | fundo da janela (padrão Windows 11) |
| `--surface` | `#ffffff` | `#2b2b2b` | cartões, listas, menu |
| `--surface-2` | `#f7f7f7` | `#323232` | hover, aba inativa |
| `--text` | `#1a1a1a` | `#f2f2f2` | texto principal |
| `--text-muted` | `#5c5c5c` | `#b3b3b3` | texto secundário (contraste ≥ 4.5:1 nos dois) |
| `--border` | `#e0e0e0` | `#3d3d3d` | divisórias |
| `--accent` | `#005fb8` | `#60cdff` | ação primária, foco (accent do Windows) |
| `--on-accent` | `#ffffff` | `#003a5c` | texto sobre accent |
| `--ok` / `--warn` / `--danger` | `#0f7b0f` / `#9d5d00` / `#c42b1c` | `#6ccb5f` / `#fce100` / `#ff99a4` | status e ações destrutivas |
| `--term-bg` / `--term-fg` | `#0c0c0c` / `#cccccc` | iguais | terminal (sempre escuro, padrão do Windows Terminal) |
| `--radius` | `8px` | | cantos |
| `--space` | `8px` (múltiplos 1–4) | | espaçamento |
| `--font` | `"Segoe UI Variable", "Segoe UI", system-ui` | | texto |
| `--font-mono` | `"Cascadia Mono", Consolas, monospace` | | terminal e IDs |

**Acessibilidade comum a todas as telas:**
- O foco fica visível (`outline: 2px solid var(--accent)`). A ordem de Tab segue a leitura.
- `Esc` fecha diálogos e a janela secundária (janela fecha para a bandeja, sem encerrar o host).
- Status nunca é só cor: sempre há texto ou ícone junto.
- Mudanças de estado usam `aria-live="polite"`, e erros usam `role="alert"`.
- O texto escala com a configuração de texto do Windows (unidades `rem`).
- `prefers-reduced-motion` desliga animações (spinner e contagem).

## 2. Bandeja (ícone + menu)

**Propósito:** ponto de entrada único. Mostra se o host está de pé sem abrir nada.

**Ícone (16/20/24 px, monocromático, com variantes claro e escuro):**

| Estado | Ícone | Tooltip |
|---|---|---|
| Rodando | `>_` normal | `live-cmd — pronto · 1 aparelho conectado` |
| Conectando ao servidor | `>_` com ponto cinza | `live-cmd — conectando ao servidor…` |
| Host com erro / offline | `>_` com ponto vermelho `!` | `live-cmd — sem conexão com o servidor. Clique para ver.` |

O tooltip nunca mostra o nome do aparelho conectado (modo silencioso, e o tooltip é visível para quem está no PC).

**Menu de contexto (clique direito ou esquerdo):**

```
● Pronto · 2 aparelhos conectados      (desabilitado, só status)
─────────────────────────
Abrir Terminal Desktop
Novo Pareamento (Exibir QR Code)
Dispositivos Conectados
─────────────────────────
✓ Iniciar com o Windows
Configurações
─────────────────────────
Sair
```

- **Linha de status:** `Pronto · N aparelhos conectados` (N = 0 → `Nenhum aparelho conectado`; 1 → `1 aparelho conectado`). Com erro: `⚠ Sem conexão com o servidor — tentando de novo`. Nesse caso o item seguinte vira `Ver detalhes`, que abre Configurações.
- **Iniciar com o Windows:** checkbox nativo. Vem **desmarcado** no primeiro uso (P6, opt-in). Marcar grava `HKCU\…\Run`. Se a gravação falhar, o checkbox volta ao estado anterior e aparece notificação do Windows: "Não foi possível ativar o início com o Windows."
- **Configurações:** item extra fora da §2.5. É necessário para URL do servidor, perfis e versão. Custa uma linha de menu.
- **Sair:** encerra o host. Se houver aparelho conectado, um diálogo nativo confirma: "Sair do live-cmd? 2 aparelhos serão desconectados e os terminais abertos serão encerrados." Botões: `Sair` e `Cancelar` (padrão = Cancelar). Sem aparelho conectado, sai direto.

**Teclado:** o menu é nativo do Windows (`Win+B` foca a bandeja, setas navegam, `Enter` ativa). Não há trabalho extra.

## 3. Janela principal — aba "Parear"

**Propósito:** exibir o QR de pareamento por no máximo 300 s, uso único (C2 §1).

**Layout:** janela de 420 × 560 px, sem redimensionar, com abas no topo: `Parear | Aparelhos | Configurações`. Conteúdo centralizado: título, QR 240 px, contagem, aviso, botões.

**Componentes:** `TabBar`, `QrPanel`, `Countdown` (barra + texto), `SecretNotice`, `InlineAlert`, botões `Gerar novo QR` / `Fechar`.

**Estados e textos:**

| Estado | Visual | Texto |
|---|---|---|
| Gerando | spinner no lugar do QR | "Gerando QR Code…" |
| Exibindo | QR + barra decrescente + `4:59` | Título "Parear um celular". Instrução: "Abra o live-cmd no celular e leia este código." Contagem: "Expira em 4:59". |
| Últimos 30 s | barra em `--warn` | "Expira em 0:29" |
| Expirado | QR borrado e oculto (`filter: blur` + sobreposição) | "Este QR Code expirou." Botão primário `Gerar novo QR`. |
| Pareado | ícone ✓ em `--ok`, QR some | "Pareado com **Pixel de Yago**." Subtexto: "Ele já pode abrir terminais deste PC." Botão `Concluir`. |
| Erro de servidor | `InlineAlert` `--danger` | "Não foi possível falar com o servidor de sinalização. Confira a internet ou o endereço em Configurações." Botões `Tentar de novo` / `Abrir Configurações`. |
| Erro de pareamento | `InlineAlert` | Mensagem de C2 §2.2 pelo código, ex.: `EXPIRED` → "Tempo esgotado. Gere um novo QR Code ou tente de novo." |

**Aviso de segredo (sempre visível abaixo do QR):** "🔒 Este código dá acesso aos terminais deste PC. Não compartilhe nem tire print. Ele vale uma vez e por 5 minutos."

**Regras:**
- `Gerar novo QR` invalida a OTK anterior (C2 §1). O botão fica sempre disponível.
- Trocar de aba **não** invalida o QR: ao voltar, a contagem continua. Fechar a janela invalida a OTK (proposta, ver Pergunta 3).
- Com a janela minimizada, o QR fica oculto (evita print acidental via Alt+Tab).
- O nome do aparelho vem do `label` da trust store (C2 §6).

**Acessibilidade:** a contagem tem `aria-live="off"` (evita anunciar a cada segundo). Um `aria-live="polite"` separado anuncia só "Faltam 1 minuto" e "QR Code expirou". `alt` do QR: "QR Code de pareamento" (nunca o conteúdo). O foco inicial cai em `Gerar novo QR`.

## 4. Janela principal — aba "Aparelhos"

**Propósito:** ver quem tem acesso e revogar.

**Layout:** lista vertical de cartões, ordenada por conectados primeiro e depois por último acesso. Botão `Parear novo aparelho` no rodapé (leva à aba Parear).

**Cartão (`DeviceRow`):**
```
📱 Pixel de Yago                         ● Conectado agora
   Pareado em 08/11/2026 · Visto agora            [Remover]
```
- Status: `● Conectado agora` (`--ok`) ou `○ Visto por último há 3 dias` (`--text-muted`).
- Datas em formato pt-BR. "Visto" é relativo: agora, há 5 min, há 3 dias, e data absoluta após 30 dias.

**Remover (irreversível) — diálogo modal:**
> **Remover "Pixel de Yago"?**
> Este celular perde o acesso aos terminais deste PC na hora. Se estiver conectado, será desconectado. Para usar de novo, será preciso parear com um novo QR Code.
> `[Cancelar]` `[Remover aparelho]` (em `--danger`)

O foco inicial cai em **Cancelar**. `Esc` cancela. Em sucesso, o cartão some e aparece um toast: "Pixel de Yago foi removido." Não há "desfazer": o tombstone é definitivo (C2 §6).

**Estados:**

| Estado | Texto |
|---|---|
| Carregando | 3 cartões esqueleto + `aria-busy="true"` |
| Vazio | "Nenhum aparelho pareado ainda." Botão `Parear um celular`. |
| Erro ao ler | "Não foi possível ler a lista de aparelhos." Botão `Tentar de novo`. Log `ERROR IO_ERROR`. |
| Erro ao remover | No diálogo: "Não foi possível remover. O aparelho continua com acesso. Tente de novo." O diálogo fica aberto. |

**Acessibilidade:** lista `<ul>`. Botão com rótulo completo: `aria-label="Remover Pixel de Yago"`. O status tem texto, não só a bolinha.

## 5. Janela principal — aba "Configurações"

**Propósito:** poucas opções, todas com efeito claro.

**Seções:**

1. **Geral**
   - `[ ] Iniciar com o Windows` — "Abre o live-cmd na bandeja quando você entra no Windows." Fica sincronizado com o menu da bandeja. Começa desmarcado (P6).
2. **Servidor de sinalização**
   - Campo `Endereço` (ex.: `wss://sinal.exemplo.workers.dev`) + botão `Salvar`.
   - Validação inline: precisa começar com `wss://`, ou `ws://` só para `localhost`/`127.0.0.1`. Erro: "Use um endereço wss://. ws:// só é aceito em localhost."
   - Ao salvar: "Salvo. Reconectando…" e depois "Conectado" ou o alerta de erro do servidor.
   - Nota: "O celular só aceita servidores conhecidos pelo app. Mudar aqui pode exigir atualizar o celular." (allowlist C2 §1).
3. **Perfis de terminal** (somente leitura)
   - Tabela: `Nome` · `Comando` (mono) — ex.: `PowerShell` · `pwsh.exe`; `Ubuntu (WSL)` · `wsl.exe -d Ubuntu`.
   - Sem `profiles.json`: "Usando perfis padrão (PowerShell e WSL)."
   - `profiles.json` inválido: alerta "O arquivo profiles.json tem um erro na linha 12. Os perfis padrão estão em uso." (sem stack trace).
   - Botão `Abrir pasta de configuração` (abre o Explorer na pasta da trust store/perfis). Mudanças valem para novos terminais após reiniciar o app.
4. **Sobre**
   - `live-cmd 0.5.0` · link `Ver logs` (abre a pasta de logs) · "Software sem assinatura digital: o Windows pode mostrar 'editor desconhecido'." (P3 do PLANO M4+M5).

**Estados:** cada seção salva sozinha, com estado próprio: salvando (botão desabilitado + "Salvando…"), salvo (texto `--ok` por 3 s) e erro (alerta inline na seção).

## 6. Terminal Desktop (janela própria)

**Propósito:** cliente desktop híbrido. Abre terminais **deste PC** (sessões do host, as mesmas que o celular vê) e de **outro PC** pareado.

**Layout:** janela redimensionável de 960 × 600 px. À esquerda fica a barra lateral `Conexões` (240 px, recolhível com `Ctrl+B`). À direita ficam abas de terminal e a área do terminal.

```
┌ Conexões ─────────┬ [PowerShell ×] [Ubuntu ×] [+] ─────────────┐
│ ESTE PC           │                                             │
│  ● PowerShell     │  PS C:\Users\yago> _                        │
│  ● Ubuntu (WSL)   │                                             │
│  + Nova sessão ▾  │                                             │
│ OUTROS PCs        │                                             │
│  ● Notebook-Sala  │                                             │
│  ○ PC-Trabalho    │                                             │
│  + Adicionar PC   │                                             │
└───────────────────┴─────────────────────────────────────────────┘
```

**Componentes:** `ConnectionList` (grupos `Este PC` e `Outros PCs`), `ProfileMenu` (`Nova sessão ▾` lista os perfis), `TabBar` de terminal (título = perfil, prefixado pelo PC remoto, ex.: `Notebook-Sala · pwsh`), `TerminalView`, `StatusBanner` sobre o terminal, diálogo `AddRemotePc`.

**Este PC:**
- Lista as sessões existentes do host (as mesmas que o celular vê). Clicar anexa numa aba. Uma sessão pode estar aberta no celular e no desktop ao mesmo tempo (C3 permite attach múltiplo; se não permitir, o item mostra "Em uso no celular").
- `Nova sessão ▾` mostra os perfis. Escolher um cria a sessão e abre a aba.
- Fechar a aba **desanexa**, sem matar a sessão. Encerrar a sessão fica no menu de contexto do item: `Encerrar sessão`, com confirmação "Encerrar PowerShell? Programas rodando nele serão fechados."

**Outros PCs:**
- Cada PC remoto aparece com status `●` conectado / `○` desconectado / `⚠` erro. Clique direito: `Remover este PC` (apaga só o pin local; o remoto continua listando este PC até revogar lá).
- **Adicionar PC** (diálogo):
  1. "No outro PC, abra live-cmd › Parear e clique em **Copiar link para outro PC**."
  2. Campo `Link de pareamento` (`type="password"`, colar com `Ctrl+V`, botão 👁 para mostrar) + `Nome deste PC remoto` (opcional, padrão = nome que o host enviar).
  3. `Conectar`.
  - Aviso: "🔒 Este link é uma senha de uso único. Cole só aqui. Ele vale por 5 minutos."
  - Ao colar e conectar, o app limpa a área de transferência. Se o conteúdo do clipboard ainda for o link, o app sobrescreve.
  - Estados: validando ("Conectando a Notebook-Sala…"), sucesso (PC entra na lista e abre a lista de sessões dele), erro pelo código C2 (`EXPIRED`, `UNAUTHORIZED` → "Link inválido ou já usado. Gere um novo no outro PC.", `SERVER_NOT_ALLOWED`, `VERSION_UNSUPPORTED`).
- **Botão "Copiar link para outro PC"** na aba Parear (lado host). É secundário, fica abaixo do aviso de segredo e copia o URI **sem exibi-lo**. Depois de copiar, mostra "Link copiado. Ele vale até o QR expirar." e limpa o clipboard quando o QR expira ou é usado. **Este botão é um desvio de C2 §1 ("URI nunca em texto")**. Ver Pergunta 1.

**Estados do terminal (`StatusBanner` sobre a área, não bloqueia a leitura do histórico):**

| Estado | Texto |
|---|---|
| Conectando | "Conectando…" + spinner |
| Reconectando | "Conexão perdida. Reconectando… (tentativa 2)" + `Cancelar` |
| Sessão encerrada | "A sessão terminou (código 0)." + `Nova sessão` / `Fechar aba` |
| Erro | mensagem C2 §2.2, ex.: `REVOKED` → "Este PC foi removido pelo outro computador. Pareie novamente." |
| Vazio (sem abas) | "Nenhum terminal aberto. Escolha uma sessão à esquerda ou crie uma nova." + `Nova sessão` |
| Remoto sem sessões | "Notebook-Sala não tem terminais abertos." + `Nova sessão ▾` |

**Teclado:**
- `Ctrl+Shift+T` abre uma nova sessão com o perfil padrão.
- `Ctrl+Shift+W` fecha a aba (desanexa).
- `Ctrl+Tab` / `Ctrl+Shift+Tab` trocam de aba.
- `Ctrl+B` recolhe a barra lateral.
- `Ctrl+Shift+C` / `Ctrl+Shift+V` copiam e colam (`Ctrl+C` vai para o shell).
- Com o terminal focado, todas as outras teclas vão para o PTY. `Ctrl+Shift+F6` devolve o foco à barra lateral (saída de armadilha de foco).

**Acessibilidade:** abas com `role="tablist"`. O terminal recebe `aria-label="Terminal PowerShell"`. Banners de estado usam `role="status"`. O tema do terminal é fixo escuro, mas a moldura segue o Windows.

**Fora deste design:** transferência de arquivos no cliente desktop (fica para depois).

## 7. Mapa de telas no mockup

| # | Tela | Estados mostrados |
|---|---|---|
| 1 | Bandeja | ícones rodando / conectando / erro; menu normal e menu com erro |
| 2 | Parear | gerando, exibindo, expirando, expirado, pareado, erro servidor |
| 3 | Aparelhos | lista, vazio, erro, diálogo remover |
| 4 | Configurações | normal + validação de URL e erro de profiles.json |
| 5 | Terminal Desktop | com abas, vazio, reconectando, diálogo Adicionar PC |

## 8. Perguntas em aberto

**1. Como este PC lê o QR de outro PC (pareamento PC-a-PC)?**
- (a) Botão `Copiar link para outro PC` na aba Parear. Copia o URI sem exibir e limpa o clipboard ao expirar. **É um desvio de C2 §1**: o URI passa pela área de transferência, que outros apps e o histórico `Win+V` podem ler.
- (b) "Ler QR da tela": o cliente captura a tela (Windows Graphics Capture), acha e decodifica o QR. Não há desvio de C2, mas exige o QR visível no mesmo monitor ou via acesso remoto, e adiciona dependência de decodificação de QR.
- (c) Ler QR de um arquivo de imagem (print). Pior: o print vira um arquivo com o segredo.
- **Recomendado: (a)**, com o clipboard marcado como "excluir do histórico" (`ExcludeClipboardContentFromMonitorProcessing`) e limpeza na expiração. Os PCs costumam estar longe um do outro, e o link chega por canal que o usuário controla (ex.: sessão remota). Precisa de aprovação explícita como desvio de C2.

**2. Uma janela com abas (Parear/Aparelhos/Configurações) ou três janelas?**
- (a) Uma janela com 3 abas, aberta na aba do item clicado. Tem menos código e menos janelas soltas.
- (b) Três janelas independentes, como os itens do menu sugerem.
- **Recomendado: (a).**

**3. Fechar a janela do QR invalida o QR?**
- (a) Não. O QR vale até 300 s ou até o uso. É mais simples, e quem troca de aba não perde o código.
- (b) Sim. Fechar invalida a OTK na hora. Fica mais seguro, mas obriga a gerar de novo.
- **Recomendado: (b) só ao fechar a janela** (trocar de aba mantém). O QR fora da tela não tem uso, e a OTK ativa sem ninguém olhando é só risco.

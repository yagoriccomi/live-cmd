# Gerenciador de Terminais Remoto — Documentação Completa do Projeto

Este arquivo compactado contém o pacote de engenharia completo (decisões, pesquisas de viabilidade, especificações de arquitetura, protocolo e roteiro de implementação) para a construção do aplicativo gerenciador de terminais remotos inspirado no 9remote.

## 📁 Estrutura do Pacote

- `handoff/gerenciador-terminais-remoto/001 - 06_11_2026 specs e roadmap de execucao.md`: **LEIA PRIMEIRO.** Documento de passagem de bastão para o agente (Claude / Codex), contendo o resumo executivo, pilha tecnológica e o foco da próxima sessão.
- `CONTEXT.md`: Glossário canônico de domínio (termos oficiais do sistema).
- `tickets/`:
  - `mapa.md`: Mapa canônico do Wayfinder.
  - `006-spec-arquitetura-host-rust.md`: Especificação do Host Daemon e Cliente Desktop em Rust (Tauri 2.x + portable-pty + Tray).
  - `007-spec-protocolo-mensagens.md`: Especificação do Protocolo WebRTC DataChannel (pty-channel e file-channel com chunks de 64 KB e resume).
  - `008-spec-app-mobile-react-native.md`: Especificação do Cliente Mobile em React Native/Expo (xterm.js Canvas + teclado ergonômico).
  - `009-plano-implementacao-fatias.md`: Roteiro de 5 fatias verticais bala traçante (tracer bullets) prontas para execução.
- `pesquisas/`: Relatórios técnicos aprofundados com fontes primárias sobre Runtime PTY, Sinalização/NAT e xterm.js Canvas.
- `answers/`: Histórico completo de todas as perguntas e decisões tomadas durante a concepção do projeto.

---
Gerado automaticamente via @handoff-mp em 06/11/2026.

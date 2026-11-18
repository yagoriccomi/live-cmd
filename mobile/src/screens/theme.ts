// Glass Aurora (mockup V1): as mesmas cores do desktop.
export const C = {
  bg: '#140b2e',
  glass: 'rgba(255,255,255,0.08)',
  border: 'rgba(206,170,255,0.5)',
  accent: '#8b5cf6',
  accent2: '#ff4fd8',
  text: '#f4efff',
  muted: '#d6c8ff',
  activeBg: '#ffffff',
  activeText: '#2a1460',
  ok: '#5ef2b0',
  warn: '#ffd35c',
  err: '#ff5c8a',
  term: 'rgba(10,4,30,0.55)',
  /** Texto escuro sobre selo claro (AO VIVO, AGUARDA). */
  ink: '#1a1030',
  dim: '#a99bd6',
};
/** Famílias empacotadas (assets/fonts, OFL). Cada peso é uma família: no Android não se combina com fontWeight. */
export const F = {
  regular: 'Sora-Regular',
  semi: 'Sora-SemiBold',
  bold: 'Sora-Bold',
  xbold: 'Sora-ExtraBold',
  mono: 'FiraCode-Regular',
  monoSemi: 'FiraCode-SemiBold',
};
/** Espaço livre embaixo do conteúdo rolável (a barra de abas fica fora da área rolável). */
export const TAB_SPACE = 12;

/** Mensagem pt-BR para os códigos de erro do host. */
export function errText(e: any): string {
  const code = e?.code ?? e?.message;
  switch (code) {
    case 'NOT_FOUND': return 'Não encontrado no PC.';
    case 'VERSION_UNSUPPORTED': return 'A versão do live-cmd no PC não tem esta tela. Atualize o PC.';
    case 'EXPIRED': return 'O PC não respondeu a tempo.';
    case 'NOT_CONNECTED': return 'Sem conexão com o PC.';
    case 'IO_ERROR': return 'O PC não conseguiu abrir a sessão.';
    default: return `Erro: ${code ?? 'desconhecido'}`;
  }
}

/** "há 5 min" a partir de um instante em ms. */
export function ago(ms: bigint, now = Date.now()): string {
  const s = Math.max(0, Math.floor((now - Number(ms)) / 1000));
  if (s < 60) return 'agora';
  if (s < 3600) return `há ${Math.floor(s / 60)} min`;
  if (s < 86400) return `há ${Math.floor(s / 3600)} h`;
  return `há ${Math.floor(s / 86400)} d`;
}

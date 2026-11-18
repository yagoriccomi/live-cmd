// Barra de teclas do terminal (mockup V1): sequências enviadas ao pty, iguais às de um terminal xterm.
export type Key = 'esc' | 'tab' | 'up' | 'down';
export const KEY_SEQ: Record<Key, string> = { esc: '\x1b', tab: '\t', up: '\x1b[A', down: '\x1b[B' };

/** Ctrl+letra (ou @ [ \ ] ^ _ ?): byte de controle; `null` se a tecla não tem Ctrl. */
export function ctrl(ch: string): string | null {
  if (ch.length !== 1) return null;
  if (ch === '?') return '\x7f';
  const c = ch.toUpperCase().charCodeAt(0);
  return c >= 0x40 && c <= 0x5f ? String.fromCharCode(c - 0x40) : null;
}

/** O que "Enviar" manda: com Ctrl ligado, só o Ctrl da 1ª letra (Ctrl+C = \x03); senão a linha + Enter. */
export function lineToSend(line: string, ctrlOn: boolean): string | null {
  if (!ctrlOn) return line + '\r';
  return ctrl(line.trimStart().slice(0, 1));
}

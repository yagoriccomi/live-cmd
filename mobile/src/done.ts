// "Vibrar ao concluir": só comandos longos. O host não manda o início do comando (só OSC 133;D),
// então mede do último Enter enviado por este celular até o `command_completed`.
export const LONG_MS = 10_000;
/** `enterMs` ausente (comando não saiu deste celular): não vibra. */
export const longRun = (enterMs: number | undefined, doneMs: number): boolean => enterMs !== undefined && doneMs - enterMs > LONG_MS;

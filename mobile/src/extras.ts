// Controles extras do pty-channel para as abas Agentes, SSH e Histórico.
// Espelha desktop/src/extras.rs; a fixture desktop/tests/fixtures/c3-extras.json prova a paridade. Puro: sem RN, sem rede.
// Todo pedido e resposta leva `api` (hoje 1); `api` numérico diferente na resposta é VERSION_UNSUPPORTED.
import { AuthError, uuidBytes, type Bytes, type Code } from './auth.ts';
import { parseU64 } from './pty.ts';

export const API = 1;
const MAX_MSG = 65536;
const fail = (c: Code): never => { throw new AuthError(c); };
const bad = (): never => fail('INVALID_MESSAGE');

// ---- pedidos (app → host) ----
export const agentList = () => ({ type: 'agent_list', api: API });
export const agentResume = (sessionId: string, cols: number, rows: number) => ({ type: 'agent_resume', api: API, session_id: sessionId, cols, rows });
export const sshList = () => ({ type: 'ssh_list', api: API });
export const sshConnect = (serverId: string, cols: number, rows: number) => ({ type: 'ssh_connect', api: API, server_id: serverId, cols, rows });
export const historyQuery = (query = '', limit = 50) => ({ type: 'history_query', api: API, ...(query && { query }), limit });
export const historyRun = (entryId: string, sessionId: string) => ({ type: 'history_run', api: API, entry_id: entryId, session_id: sessionId });

// ---- respostas (host → app) ----
export type AgentState = 'running' | 'exited' | 'saved';
export interface Agent { sessionId: string; agent: string; label: string; cwd: string | null; state: AgentState; lastActivityMs: bigint }
export interface SshServer { serverId: string; label: string; host: string; user: string; port: number; latencyMs: number | null }
export interface HistoryEntry { entryId: string; command: string; cwd: string | null; atMs: bigint; exitCode: number | null }
export type ExtraMsg =
  | { t: 'agent_list_ok'; requestId: string; agents: Agent[] }
  | { t: 'agent_resume_ok' | 'ssh_connect_ok'; requestId: string; sessionId: string }
  | { t: 'ssh_list_ok'; requestId: string; servers: SshServer[] }
  | { t: 'history_query_ok'; requestId: string; entries: HistoryEntry[] }
  | { t: 'history_run_ok'; requestId: string };

const REPLIES = ['agent_list_ok', 'agent_resume_ok', 'ssh_list_ok', 'ssh_connect_ok', 'history_query_ok', 'history_run_ok'];
const UUID4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const isObj = (x: unknown): x is Record<string, any> => typeof x === 'object' && x !== null && !Array.isArray(x);
const keys = (o: Record<string, unknown>, req: string[], opt: string[] = []) => {
  if (!req.every(k => k in o) || !Object.keys(o).every(k => req.includes(k) || opt.includes(k))) bad();
};
const str = (x: unknown): string => typeof x === 'string' ? x : bad();
const strOrNull = (x: unknown): string | null => x === null ? null : str(x);
const uuid = (x: unknown): string => typeof x === 'string' && UUID4.test(x) ? x : bad();
const int = (x: unknown, lo: number, hi: number): number => Number.isInteger(x) && (x as number) >= lo && (x as number) <= hi ? x as number : bad();
const list = <T>(x: unknown, f: (o: Record<string, any>) => T): T[] => Array.isArray(x) ? x.map(i => isObj(i) ? f(i) : bad()) : bad();

/** Tipo de resposta extra? (`conn.ts` usa para escolher o parser.) */
export function isExtraReply(text: string): boolean {
  try { const o = JSON.parse(text); return isObj(o) && REPLIES.includes(o.type); } catch { return false; }
}

/** Respostas extras, mesma regra estrita de `parseHostControl`: campo desconhecido ou tipo errado é INVALID_MESSAGE. */
export function parseExtraReply(text: string, connectionId: string): ExtraMsg {
  if (new TextEncoder().encode(text).length > MAX_MSG) bad();
  let o: any;
  try { o = JSON.parse(text); } catch { bad(); }
  if (!isObj(o)) bad();
  if (o.v !== 1) fail(typeof o.v === 'number' ? 'VERSION_UNSUPPORTED' : 'INVALID_MESSAGE');
  if (o.connection_id !== connectionId) bad();
  const t = str(o.type);
  if (!REPLIES.includes(t)) bad();
  if (o.api !== API) fail(typeof o.api === 'number' ? 'VERSION_UNSUPPORTED' : 'INVALID_MESSAGE');
  const E = ['v', 'type', 'connection_id', 'request_id', 'api'];
  const reply = (f: string[]): string => { keys(o, [...E, ...f]); return uuid(o.request_id); };
  switch (t) {
    case 'agent_list_ok': {
      const requestId = reply(['agents']);
      return { t, requestId, agents: list(o.agents, a => {
        keys(a, ['session_id', 'agent', 'label', 'cwd', 'state', 'last_activity_ms']);
        const state = a.state === 'running' || a.state === 'exited' || a.state === 'saved' ? a.state as AgentState : bad();
        return { sessionId: uuid(a.session_id), agent: str(a.agent), label: str(a.label), cwd: strOrNull(a.cwd), state, lastActivityMs: parseU64(a.last_activity_ms) };
      }) };
    }
    case 'agent_resume_ok': case 'ssh_connect_ok': { const requestId = reply(['session_id']); return { t, requestId, sessionId: uuid(o.session_id) }; }
    case 'ssh_list_ok': {
      const requestId = reply(['servers']);
      return { t, requestId, servers: list(o.servers, s => {
        keys(s, ['server_id', 'label', 'host', 'user', 'port', 'latency_ms']);
        return { serverId: str(s.server_id), label: str(s.label), host: str(s.host), user: str(s.user), port: int(s.port, 1, 65535),
          latencyMs: s.latency_ms === null ? null : int(s.latency_ms, 0, 2 ** 32 - 1) };
      }) };
    }
    case 'history_query_ok': {
      const requestId = reply(['entries']);
      return { t, requestId, entries: list(o.entries, e => {
        keys(e, ['entry_id', 'command', 'cwd', 'at_ms', 'exit_code']);
        return { entryId: str(e.entry_id), command: str(e.command), cwd: strOrNull(e.cwd), atMs: parseU64(e.at_ms),
          exitCode: e.exit_code === null ? null : int(e.exit_code, -(2 ** 31), 2 ** 31 - 1) };
      }) };
    }
    default: return { t: 'history_run_ok', requestId: reply([]) };
  }
}

/** Agentes agrupados pela pasta (`cwd`), grupos e itens do mais recente para o mais antigo. */
export function groupByFolder(agents: Agent[]): { folder: string; agents: Agent[] }[] {
  const m = new Map<string, Agent[]>();
  for (const a of [...agents].sort((x, y) => (y.lastActivityMs > x.lastActivityMs ? 1 : y.lastActivityMs < x.lastActivityMs ? -1 : 0))) {
    const k = a.cwd ?? 'Sem pasta';
    m.set(k, [...(m.get(k) ?? []), a]);
  }
  return [...m].map(([folder, agents]) => ({ folder, agents }));
}

/** Último trecho do caminho, para títulos curtos. */
export const baseName = (p: string) => p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p;
export const sidBytes = (s: string): Bytes => uuidBytes(s) ?? bad();

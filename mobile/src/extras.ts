// Controles extras do pty-channel para as abas Agentes, SSH e Histórico.
// Espelha desktop/src/extras.rs; a fixture desktop/tests/fixtures/c3-extras.json prova a paridade. Puro: sem RN, sem rede.
// Todo pedido e resposta leva `api` (hoje 1); `api` numérico diferente na resposta é VERSION_UNSUPPORTED.
// Capacidades: logo após a auth o app manda `capsProbe()` (um `session_attach` para a sessão reservada CAPS_PROBE,
// que o host 0.1.8 já entende e responde NOT_FOUND sem derrubar). Host novo responde `caps_ok`; sem a tela, a aba
// pede para atualizar o live-cmd no PC. O `auth_ok` não muda: validadores antigos recusam campo novo.
import { AuthError, uuidBytes, type Bytes, type Code } from './auth.ts';
import { parseU64 } from './pty.ts';

export const API = 1;
/** Sessão reservada da pergunta de capacidades (desktop/src/extras.rs CAPS_PROBE). */
export const CAPS_PROBE = '00000000-0000-4000-8000-000000000001';
/** `folders`: navegador de pastas do PC; `ssh_keys`: seletor da chave em %USERPROFILE%\.ssh; `agent_default`: "Salvar como padrão". */
// Novas (opt-in): 'settings' (aparência), 'ssh_opts' (manter vivo / reconectar), 'agent_waiting' (pedido de confirmação),
// 'history_sessions' (resumo por sessão e dia). Os campos novos só vêm quando o pedido leva `ext: true`.
export type Cap = 'agents' | 'ssh' | 'history' | 'folders' | 'ssh_keys' | 'agent_default' | 'settings' | 'ssh_opts' | 'agent_waiting' | 'history_sessions';
const MAX_MSG = 65536;
const fail = (c: Code): never => { throw new AuthError(c); };
const bad = (): never => fail('INVALID_MESSAGE');

/** Agentes aceitos pelo host (desktop/src/proto.rs AGENTS). */
export const AGENTS = ['claude', 'openclaude', 'codex'] as const;
/** Tipos de sessão do histórico e o nome exibido (mesmos da tela desktop, historico.js). */
export const HISTORY_KINDS: Record<string, string> = { pwsh: 'PowerShell', cmd: 'Prompt de Comando', wsl: 'WSL / bash', ssh: 'SSH', outro: 'Outra' };

// ---- pedidos (app → host) ----
export interface HistoryFilter { query?: string; limit?: number; days?: number | null; kind?: string | null; errors?: boolean; /** resumo `sessions` (cap `history_sessions`) */ ext?: boolean }
/** `keepaliveS`: 0 desliga, senão 5..3600 s (ServerAliveInterval). Só mande com o cap `ssh_opts`. */
export interface SshForm { serverId?: string; label: string; user: string; host: string; port: number; key: string | null; keepaliveS?: number; autoReconnect?: boolean }
/** Aparência compartilhada com a tela Configurações do desktop (desktop/src/extras.rs Appearance). */
export interface Appearance { theme: string; fontFamily: string; fontSize: number; opacity: number; blur: number; glow: boolean; animations: boolean }
export const THEMES = ['dark', 'cosmo', 'light', 'contrast'] as const;
export const FONTS = ['Fira Code', 'Cascadia Mono', 'Consolas', 'Courier New'] as const;
const RANGES = { font_size: [10, 24], opacity: [60, 100], blur: [0, 60] } as const;
export const capsProbe = () => ({ type: 'session_attach', session_id: CAPS_PROBE, from_offset: null });
/** `ext`: estado 'waiting' na lista e inscrição no evento `agent_waiting` (cap `agent_waiting`). */
export const agentList = (ext = false) => ({ type: 'agent_list', api: API, ...(ext && { ext: true }) });
export const agentResume = (sessionId: string, cols: number, rows: number) => ({ type: 'agent_resume', api: API, session_id: sessionId, cols, rows });
export const agentCreate = (agent: string, cwd: string | null, args: string, cols: number, rows: number) =>
  ({ type: 'agent_create', api: API, agent, ...(cwd && { cwd }), ...(args && { args }), cols, rows });
export const agentRemove = (sessionId: string) => ({ type: 'agent_remove', api: API, session_id: sessionId });
export const sshList = (ext = false) => ({ type: 'ssh_list', api: API, ...(ext && { ext: true }) });
export const sshConnect = (serverId: string, cols: number, rows: number) => ({ type: 'ssh_connect', api: API, server_id: serverId, cols, rows });
/** Nunca leva senha: o host recusa qualquer campo além destes. */
export const sshSave = (f: SshForm) => ({ type: 'ssh_save', api: API, ...(f.serverId && { server_id: f.serverId }), label: f.label, user: f.user, host: f.host, port: f.port, key: f.key,
  ...(f.keepaliveS !== undefined && { keepalive_s: f.keepaliveS }), ...(f.autoReconnect !== undefined && { auto_reconnect: f.autoReconnect }) });
export const sshRemove = (serverId: string) => ({ type: 'ssh_remove', api: API, server_id: serverId });
export const sshProbe = (serverId: string) => ({ type: 'ssh_probe', api: API, server_id: serverId });
export const sshReconnect = (sessionId: string, cols: number, rows: number) => ({ type: 'ssh_reconnect', api: API, session_id: sessionId, cols, rows });
export const historyQuery = (f: HistoryFilter = {}) => ({
  type: 'history_query', api: API, ...(f.query && { query: f.query }), limit: f.limit ?? 50,
  ...(f.days && { days: f.days }), ...(f.kind && { kind: f.kind }), ...(f.errors && { errors: true }), ...(f.ext && { ext: true }),
});
export const settingsGet = () => ({ type: 'settings_get', api: API });
/** Só os campos mandados mudam; o host devolve a aparência inteira em `settings_set_ok`. */
export const settingsSet = (p: Partial<Appearance>) => {
  const m: Record<string, string> = { theme: 'theme', fontFamily: 'font_family', fontSize: 'font_size', opacity: 'opacity', blur: 'blur', glow: 'glow', animations: 'animations' };
  const settings: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(p)) if (v !== undefined && m[k]) settings[m[k]] = v;
  return { type: 'settings_set', api: API, settings };
};
export const historyRun = (entryId: string, sessionId: string) => ({ type: 'history_run', api: API, entry_id: entryId, session_id: sessionId });
export const historyClear = () => ({ type: 'history_clear', api: API });
/** Subpastas de `path` no PC (vazio: unidades), a mesma listagem do navegador de pastas do desktop. */
export const folderList = (path: string) => ({ type: 'folder_list', api: API, path });
/** Chaves e subpastas dentro de %USERPROFILE%\.ssh do PC (sem `path`: a própria .ssh). */
export const sshKeyList = (path?: string) => ({ type: 'ssh_key_list', api: API, ...(path && { path }) });
export const agentDefaultGet = () => ({ type: 'agent_default_get', api: API });
export const agentDefaultSet = (agent: string, cwd: string | null, args: string) =>
  ({ type: 'agent_default_set', api: API, agent, ...(cwd && { cwd }), ...(args && { args }) });
/** Sessões abertas no host (pedido base do pty-channel, sem `api`): alimenta a lista "Abas". */
export const sessionList = () => ({ type: 'session_list' });

const MONTHS = ['JAN', 'FEV', 'MAR', 'ABR', 'MAI', 'JUN', 'JUL', 'AGO', 'SET', 'OUT', 'NOV', 'DEZ'];

/** Resumo "Sessões" do Histórico, calculado no cliente: comandos por sessão, a mais recente primeiro. */
export function summarizeSessions(entries: HistoryEntry[]): { label: string; kind: string; count: number; lastMs: bigint }[] {
  const m = new Map<string, { label: string; kind: string; count: number; lastMs: bigint }>();
  for (const e of entries) {
    const k = `${e.kind}\u0000${e.label}`;
    const cur = m.get(k);
    if (cur) { cur.count++; if (e.atMs > cur.lastMs) cur.lastMs = e.atMs; }
    else m.set(k, { label: e.label || e.kind, kind: e.kind, count: 1, lastMs: e.atMs });
  }
  return [...m.values()].sort((a, b) => (b.lastMs > a.lastMs ? 1 : b.lastMs < a.lastMs ? -1 : 0));
}

// ---- respostas (host → app) ----
/** 'waiting': rodando e parado esperando confirmação (só com `agentList(true)`). */
export type AgentState = 'running' | 'waiting' | 'exited' | 'saved';
export type SshStatus = 'unknown' | 'testing' | 'ok' | 'dns' | 'timeout' | 'offline';
export interface Agent { sessionId: string; agent: string; args: string; label: string; cwd: string | null; state: AgentState; lastActivityMs: bigint }
export interface SshServer { serverId: string; label: string; host: string; user: string; port: number; key: string | null; latencyMs: number | null; status: SshStatus; keepaliveS?: number; autoReconnect?: boolean }
export interface SshSession { sessionId: string; label: string; connected: boolean }
export interface HistoryEntry { entryId: string; command: string; cwd: string | null; atMs: bigint; exitCode: number | null; kind: string; label: string; pc: string }
export interface AgentDefault { agent: string; cwd: string | null; args: string }
/** Resumo "Sessões" da tela Histórico: comandos por sessão e dia (dia local do PC, `AAAA-MM-DD`). */
export interface HistorySession { label: string; kind: string; pc: string; day: string; count: number; errors: number; lastMs: bigint }
export type ExtraMsg =
  | { t: 'agent_list_ok'; requestId: string; agents: Agent[] }
  | { t: 'agent_resume_ok' | 'agent_create_ok' | 'ssh_connect_ok' | 'ssh_reconnect_ok'; requestId: string; sessionId: string }
  | { t: 'ssh_list_ok'; requestId: string; servers: SshServer[]; sessions: SshSession[] }
  | { t: 'ssh_save_ok'; requestId: string; serverId: string }
  | { t: 'caps_ok'; requestId: string; caps: string[] }
  | { t: 'history_query_ok'; requestId: string; entries: HistoryEntry[]; sessions?: HistorySession[] }
  | { t: 'settings_get_ok' | 'settings_set_ok'; requestId: string; settings: Appearance }
  /** Evento sem pedido: um agente passou a esperar (ou deixou de esperar) confirmação. */
  | { t: 'agent_waiting'; sessionId: string; label: string; waiting: boolean }
  | { t: 'folder_list_ok'; requestId: string; path: string; dirs: string[] }
  | { t: 'ssh_key_list_ok'; requestId: string; root: string; path: string; dirs: string[]; keys: string[] }
  | { t: 'agent_default_ok'; requestId: string; value: AgentDefault | null }
  | { t: 'history_run_ok' | 'history_clear_ok' | 'agent_remove_ok' | 'ssh_remove_ok' | 'ssh_probe_ok' | 'agent_default_set_ok'; requestId: string };

const SESSION_REPLIES = ['agent_resume_ok', 'agent_create_ok', 'ssh_connect_ok', 'ssh_reconnect_ok'];
const EMPTY_REPLIES = ['history_run_ok', 'history_clear_ok', 'agent_remove_ok', 'ssh_remove_ok', 'ssh_probe_ok', 'agent_default_set_ok'];
const REPLIES = ['caps_ok', 'agent_list_ok', 'ssh_list_ok', 'ssh_save_ok', 'history_query_ok', 'folder_list_ok', 'ssh_key_list_ok', 'agent_default_ok',
  'settings_get_ok', 'settings_set_ok', 'agent_waiting', ...SESSION_REPLIES, ...EMPTY_REPLIES];
const SSH_STATUS: SshStatus[] = ['unknown', 'testing', 'ok', 'dns', 'timeout', 'offline'];
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
/** Lista de nomes (pasta ou arquivo): texto sem controle; `plain` também recusa barra, `.` e `..` (um nome só, nunca um caminho). */
const names = (x: unknown, plain: boolean, max = 500): string[] => {
  if (!Array.isArray(x) || x.length > max) bad();
  return (x as unknown[]).map(n => {
    const v = str(n);
    if (!v || /[\u0000-\u001f\u007f]/.test(v) || (plain && (/[\\/]/.test(v) || v === '.' || v === '..'))) bad();
    return v;
  });
};
const bool = (x: unknown): boolean => typeof x === 'boolean' ? x : bad();
const appearance = (x: unknown): Appearance => {
  if (!isObj(x)) bad();
  const a = x as Record<string, any>;
  keys(a, ['theme', 'font_family', 'font_size', 'opacity', 'blur', 'glow', 'animations']);
  return { theme: (THEMES as readonly string[]).includes(a.theme) ? a.theme : bad(), fontFamily: (FONTS as readonly string[]).includes(a.font_family) ? a.font_family : bad(),
    fontSize: int(a.font_size, ...RANGES.font_size), opacity: int(a.opacity, ...RANGES.opacity), blur: int(a.blur, ...RANGES.blur), glow: bool(a.glow), animations: bool(a.animations) };
};

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
  const reply = (f: string[], opt: string[] = []): string => { keys(o, [...E, ...f], opt); return uuid(o.request_id); };
  if (t === 'agent_waiting') {
    keys(o, ['v', 'type', 'connection_id', 'api', 'session_id', 'label', 'waiting']);
    return { t, sessionId: uuid(o.session_id), label: str(o.label), waiting: bool(o.waiting) };
  }
  if (t === 'settings_get_ok' || t === 'settings_set_ok') { const requestId = reply(['settings']); return { t, requestId, settings: appearance(o.settings) }; }
  if (SESSION_REPLIES.includes(t)) { const requestId = reply(['session_id']); return { t: t as any, requestId, sessionId: uuid(o.session_id) }; }
  if (EMPTY_REPLIES.includes(t)) return { t: t as any, requestId: reply([]) };
  switch (t) {
    case 'agent_list_ok': {
      const requestId = reply(['agents']);
      return { t, requestId, agents: list(o.agents, a => {
        keys(a, ['session_id', 'agent', 'args', 'label', 'cwd', 'state', 'last_activity_ms']);
        const state = ['running', 'waiting', 'exited', 'saved'].includes(a.state) ? a.state as AgentState : bad();
        return { sessionId: uuid(a.session_id), agent: str(a.agent), args: str(a.args), label: str(a.label), cwd: strOrNull(a.cwd), state, lastActivityMs: parseU64(a.last_activity_ms) };
      }) };
    }
    case 'ssh_list_ok': {
      const requestId = reply(['servers', 'sessions']);
      return { t, requestId, servers: list(o.servers, s => {
        keys(s, ['server_id', 'label', 'host', 'user', 'port', 'key', 'latency_ms', 'status'], ['keepalive_s', 'auto_reconnect']);
        return { serverId: str(s.server_id), label: str(s.label), host: str(s.host), user: str(s.user), port: int(s.port, 1, 65535), key: strOrNull(s.key),
          latencyMs: s.latency_ms === null ? null : int(s.latency_ms, 0, 2 ** 32 - 1), status: SSH_STATUS.includes(s.status) ? s.status : bad(),
          ...('keepalive_s' in s && { keepaliveS: int(s.keepalive_s, 0, 3600) }), ...('auto_reconnect' in s && { autoReconnect: bool(s.auto_reconnect) }) };
      }), sessions: list(o.sessions, s => {
        keys(s, ['session_id', 'label', 'state']);
        const connected = s.state === 'connected' ? true : s.state === 'disconnected' ? false : bad();
        return { sessionId: uuid(s.session_id), label: str(s.label), connected };
      }) };
    }
    case 'caps_ok': {
      const requestId = reply(['caps']);
      const c = o.caps;
      if (!Array.isArray(c) || c.length > 32 || !c.every((x: unknown) => typeof x === 'string' && /^[a-z0-9_.:-]{1,32}$/.test(x))) bad();
      return { t, requestId, caps: c };
    }
    case 'ssh_save_ok': { const requestId = reply(['server_id']); return { t, requestId, serverId: str(o.server_id) }; }
    case 'folder_list_ok': {
      const requestId = reply(['path', 'dirs']);
      const path = str(o.path);
      // Na raiz vêm as unidades (`C:\`); dentro de uma pasta, só nomes.
      return { t, requestId, path, dirs: names(o.dirs, path !== '') };
    }
    case 'ssh_key_list_ok': {
      const requestId = reply(['root', 'path', 'dirs', 'keys']);
      return { t, requestId, root: str(o.root), path: str(o.path), dirs: names(o.dirs, true), keys: names(o.keys, true, 200) };
    }
    case 'agent_default_ok': {
      const requestId = reply(['agent', 'cwd', 'args']);
      if (o.agent === null) { if (o.cwd !== null || str(o.args) !== '') bad(); return { t, requestId, value: null }; }
      const agent = (AGENTS as readonly string[]).includes(o.agent) ? o.agent as string : bad();
      const args = str(o.args);
      if (!validArgs(args)) bad();
      return { t, requestId, value: { agent, cwd: strOrNull(o.cwd), args } };
    }
    default: {
      const requestId = reply(['entries'], ['sessions']);
      const sessions = 'sessions' in o ? list(o.sessions, g => {
        keys(g, ['label', 'kind', 'pc', 'day', 'count', 'errors', 'last_ms']);
        return { label: str(g.label), kind: str(g.kind), pc: str(g.pc), day: /^\d{4}-\d{2}-\d{2}$/.test(g.day) ? g.day : bad(),
          count: int(g.count, 0, 2 ** 32 - 1), errors: int(g.errors, 0, 2 ** 32 - 1), lastMs: parseU64(g.last_ms) };
      }) : undefined;
      return { t: 'history_query_ok', requestId, ...(sessions && { sessions }), entries: list(o.entries, e => {
        keys(e, ['entry_id', 'command', 'cwd', 'at_ms', 'exit_code', 'kind', 'label', 'pc']);
        return { entryId: str(e.entry_id), command: str(e.command), cwd: strOrNull(e.cwd), atMs: parseU64(e.at_ms),
          exitCode: e.exit_code === null ? null : int(e.exit_code, -(2 ** 31), 2 ** 31 - 1), kind: str(e.kind), label: str(e.label), pc: str(e.pc) };
      }) };
    }
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

/** Comandos agrupados por dia, como a tela desktop: "Hoje", "Ontem" ou a data por extenso. */
export function groupByDay(entries: HistoryEntry[], now = new Date()): { day: string; entries: HistoryEntry[] }[] {
  const key = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
  const y = new Date(now); y.setDate(now.getDate() - 1);
  const name = (ms: bigint) => {
    const d = new Date(Number(ms));
    if (key(d) === key(now)) return 'Hoje';
    if (key(d) === key(y)) return 'Ontem';
    // Mockup V1: "15 NOV"; o ano só aparece quando não é o atual.
    const dm = `${String(d.getDate()).padStart(2, '0')} ${MONTHS[d.getMonth()]}`;
    return d.getFullYear() === now.getFullYear() ? dm : `${dm} ${d.getFullYear()}`;
  };
  const out: { day: string; entries: HistoryEntry[] }[] = [];
  for (const e of entries) {
    const d = name(e.atMs);
    if (out[out.length - 1]?.day === d) out[out.length - 1].entries.push(e); else out.push({ day: d, entries: [e] });
  }
  return out;
}

/** Erro do formulário SSH com as mesmas regras da tela desktop (ssh.js) e do host (desktop/src/ssh.rs); `null` se ok. */
export function sshFormError(f: SshForm): string | null {
  const name = f.label.trim();
  if (!name || [...name].length > 40) return 'INVALID_SSH_NAME';
  if (!/^(?!-)[A-Za-z0-9._-]{1,64}$/.test(f.user.trim())) return 'INVALID_SSH_USER';
  if (!/^(?!-)[A-Za-z0-9._:-]{1,253}$/.test(f.host.trim())) return 'INVALID_SSH_HOST';
  if (!Number.isInteger(f.port) || f.port < 1 || f.port > 65535) return 'INVALID_SSH_PORT';
  const k = f.key?.trim();
  if (k && (!/^([A-Za-z]:[\\/]|\\\\.)/.test(k) || /["\u0000-\u001f\u007f]/.test(k))) return 'INVALID_SSH_KEY';
  return null;
}
export const SSH_MSG: Record<string, string> = {
  INVALID_SSH_NAME: 'Dê um nome de 1 a 40 caracteres.',
  INVALID_SSH_USER: 'Usuário inválido: use só letras, números, ponto, hífen ou sublinhado, sem espaços.',
  INVALID_SSH_HOST: 'Host inválido: use um nome ou IP, sem espaços nem caracteres especiais.',
  INVALID_SSH_PORT: 'Porta inválida: use um número de 1 a 65535.',
  INVALID_SSH_KEY: 'Chave inválida: informe o caminho completo do arquivo no PC (ex.: C:\\Users\\voce\\.ssh\\id_ed25519).',
  LIMIT_EXCEEDED: 'Limite de 200 servidores atingido.',
};

/** Args extras do agente: mesma regra do host (desktop/src/proto.rs valid_args). */
export const validArgs = (a: string) => a.length <= 512 && /^[A-Za-z0-9 ._=/:,-]*$/.test(a);

/** Caminho do item `name` dentro de `base` no PC (raiz vazia: `name` já é a unidade). */
export const joinPath = (base: string, name: string) => base ? `${base.replace(/[\\/]+$/, '')}\\${name}` : name;
/** Pasta de cima, como o navegador do desktop: `C:\a\b` → `C:\a\`, `C:\` → '' (unidades). */
export function parentPath(p: string): string {
  const base = p.replace(/[\\/]+$/, '');
  return base.includes('\\') ? base.slice(0, base.lastIndexOf('\\') + 1) : '';
}

/** Último trecho do caminho, para títulos curtos. */
export const baseName = (p: string) => p.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p;
export const sidBytes = (s: string): Bytes => uuidBytes(s) ?? bad();

// C3 pty-channel (docs/planos/SPEC-C3-controle.md): frames binários (§3), controles JSON (§2.1–2.3) e janela de input (§4).
// Espelha desktop/src/proto.rs; as fixtures desktop/tests/fixtures/c3-*.json provam a paridade. Puro: sem RN, sem rede.
import { AuthError, b64d, uuidBytes, type Bytes, type Code } from './auth.ts';

export const HEADER = 30;
export const MAX_IO = 16384;
export const MAX_FRAME = HEADER + MAX_IO;
const MAX_MSG = 65536;
const U64_MAX = (1n << 64n) - 1n;
const CODES: readonly Code[] = ['INVALID_MESSAGE', 'VERSION_UNSUPPORTED', 'UNAUTHORIZED', 'REVOKED', 'EXPIRED', 'LIMIT_EXCEEDED',
  'NOT_FOUND', 'ALREADY_EXISTS', 'HASH_MISMATCH', 'IO_ERROR', 'SERVER_NOT_ALLOWED'];
const fail = (c: Code): never => { throw new AuthError(c); };
const bad = (): never => fail('INVALID_MESSAGE');

export type Op =
  | { t: 'input'; data: Bytes } | { t: 'output'; data: Bytes }
  | { t: 'resize'; cols: number; rows: number } | { t: 'heartbeat' } | { t: 'exited'; code: number };
/** `seq` é u64: sempre `bigint`. */
export interface Frame { sessionId: Bytes; seq: bigint; op: Op }

const dim = (n: number) => n >= 1 && n <= 1000 ? n : bad();

export function parseFrame(b: Bytes): Frame {
  if (b.length < HEADER || b.length > MAX_FRAME) bad();
  if (b[0] !== 1) fail('VERSION_UNSUPPORTED');
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  const sessionId = b.slice(2, 18), seq = v.getBigUint64(18), p = b.slice(HEADER);
  if (v.getUint32(26) !== p.length) bad();
  const zero = sessionId.every(x => x === 0), io = p.length >= 1 && p.length <= MAX_IO;
  let op: Op;
  switch (b[1]) {
    case 1: op = io ? { t: 'input', data: p } : bad(); break;
    case 2: op = io ? { t: 'output', data: p } : bad(); break;
    case 3: op = p.length === 4 && seq === 0n ? { t: 'resize', cols: dim(v.getUint16(30)), rows: dim(v.getUint16(32)) } : bad(); break;
    case 4: return p.length === 0 && zero ? { sessionId, seq, op: { t: 'heartbeat' } } : bad();
    case 5: op = p.length === 4 ? { t: 'exited', code: v.getInt32(30) } : bad(); break;
    default: return bad();
  }
  return zero ? bad() : { sessionId, seq, op };
}

export function encodeFrame(f: Frame): Bytes {
  const o = f.op;
  const [code, p]: [number, Bytes] =
    o.t === 'input' ? [1, o.data] : o.t === 'output' ? [2, o.data]
    : o.t === 'resize' ? [3, Uint8Array.of(o.cols >> 8, o.cols & 255, o.rows >> 8, o.rows & 255)]
    : o.t === 'heartbeat' ? [4, new Uint8Array(0)]
    : [5, (() => { const x = new Uint8Array(4); new DataView(x.buffer).setInt32(0, o.code); return x; })()];
  const b = new Uint8Array(HEADER + p.length), v = new DataView(b.buffer);
  b[0] = 1; b[1] = code; b.set(f.sessionId, 2);
  v.setBigUint64(18, f.seq); v.setUint32(26, p.length); b.set(p, HEADER);
  return b;
}

/** u64 como string decimal canônica (§2.1); número JSON é rejeitado (N10). */
export function parseU64(x: unknown): bigint {
  if (typeof x !== 'string' || !/^(0|[1-9][0-9]{0,19})$/.test(x)) bad();
  const n = BigInt(x as string);
  return n <= U64_MAX ? n : bad();
}
export const u64s = (n: bigint): string => n.toString();

const UUID4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuidV4Lower = (x: unknown): Bytes => typeof x === 'string' && UUID4.test(x) ? uuidBytes(x)! : bad();
export const uuidStr = (b: Bytes): string => {
  const h = Array.from(b, x => x.toString(16).padStart(2, '0')).join('');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
};

export type Focus = 'foreground' | 'background';
export type Req =
  | { t: 'create'; profileId: string; cols: number; rows: number; label?: string }
  | { t: 'list' }
  | { t: 'attach'; sessionId: Bytes; fromOffset: bigint | null; size?: [number, number] }
  | { t: 'detach'; sessionId: Bytes } | { t: 'close'; sessionId: Bytes } | { t: 'focus'; state: Focus };
export type Control =
  | { kind: 'request'; requestId: string; req: Req }
  | { kind: 'error'; requestId?: string; code: Code };

/** Controles cliente→host, mesma regra do host. Campos desconhecidos, tipos errados ou faixas inválidas: INVALID_MESSAGE. */
function envelope(text: string, connectionId: string): any {
  if (new TextEncoder().encode(text).length > MAX_MSG) bad();
  let o: any;
  try { o = JSON.parse(text); } catch { bad(); }
  if (typeof o !== 'object' || o === null || Array.isArray(o)) bad();
  if (o.v !== 1) fail(typeof o.v === 'number' ? 'VERSION_UNSUPPORTED' : 'INVALID_MESSAGE');
  if (o.connection_id !== connectionId) bad();
  return o;
}

export function parseControl(text: string, connectionId: string): Control {
  const o = envelope(text, connectionId);
  const fields = (req: string[], opt: string[] = []) => {
    if (!req.every(k => k in o) || !Object.keys(o).every(k => ['v', 'type', 'connection_id', 'request_id', ...req, ...opt].includes(k))) bad();
  };
  const s = (k: string): string => typeof o[k] === 'string' ? o[k] : bad();
  // ponytail: JS não distingue 80 de 80.0 (o host rejeita 80.0); irrelevante no sentido app→host.
  const n = (k: string) => Number.isInteger(o[k]) ? dim(o[k]) : bad();
  const requestId: string | undefined = 'request_id' in o ? (uuidV4Lower(o.request_id), o.request_id) : undefined;
  const ty = s('type');
  if (ty === 'error') {
    fields(['code']);
    return { kind: 'error', requestId, code: CODES.includes(o.code) ? o.code : bad() };
  }
  let req: Req;
  switch (ty) {
    case 'session_create': {
      fields(['profile_id', 'cols', 'rows'], ['label']);
      const label = 'label' in o ? s('label') : undefined;
      if (label !== undefined && [...label].length > 64) bad();
      req = { t: 'create', profileId: s('profile_id'), cols: n('cols'), rows: n('rows'), ...(label !== undefined && { label }) };
      break;
    }
    case 'session_list': fields([]); req = { t: 'list' }; break;
    case 'session_attach': {
      fields(['session_id', 'from_offset'], ['cols', 'rows']);
      const fromOffset = o.from_offset === null ? null : parseU64(o.from_offset);
      if (('cols' in o) !== ('rows' in o)) bad();
      req = { t: 'attach', sessionId: uuidV4Lower(o.session_id), fromOffset, ...('cols' in o && { size: [n('cols'), n('rows')] as [number, number] }) };
      break;
    }
    case 'session_detach': fields(['session_id']); req = { t: 'detach', sessionId: uuidV4Lower(o.session_id) }; break;
    case 'session_close': fields(['session_id']); req = { t: 'close', sessionId: uuidV4Lower(o.session_id) }; break;
    case 'client_focus':
      fields(['state']);
      req = { t: 'focus', state: o.state === 'foreground' || o.state === 'background' ? o.state : bad() };
      break;
    default: return bad();
  }
  return { kind: 'request', requestId: requestId ?? bad(), req };
}

export type State = 'running' | 'exited';
export interface SessionInfo {
  sessionId: Bytes; label: string; state: State; createdAtMs: bigint; lastActivityMs: bigint; cols: number; rows: number; attachedCount: number;
}
export type HostMsg =
  | { t: 'session_create_ok'; requestId: string; sessionId: Bytes }
  | { t: 'session_list_ok'; requestId: string; sessions: SessionInfo[]; profiles: { profileId: string; label: string }[] }
  | { t: 'session_attach_ok'; requestId: string; sessionId: Bytes; state: State; bufferStart: bigint; outputEnd: bigint; lastInputSeq: bigint; exitCode?: number }
  | { t: 'session_detach_ok' | 'session_close_ok' | 'client_focus_ok'; requestId: string }
  /** `fatal`: sem `request_id` (§2) — fechar a PeerConnection. */
  | { t: 'error'; requestId?: string; code: Code; fatal: boolean }
  | { t: 'input_ack'; sessionId: Bytes; seq: bigint }
  | { t: 'replay_begin'; sessionId: Bytes; fromOffset: bigint; toOffset: bigint }
  | { t: 'replay_gap'; sessionId: Bytes; requested: bigint; availableFrom: bigint }
  | { t: 'replay_end'; sessionId: Bytes; offset: bigint }
  | { t: 'session_exited'; sessionId: Bytes; exitCode: number; finalOffset: bigint }
  /** `data`: sequência ANSI que redesenha a tela (base64url sem padding). */
  | { t: 'screen_snapshot'; sessionId: Bytes; offset: bigint; cols: number; rows: number; data: Bytes }
  | { t: 'command_completed'; sessionId: Bytes; exitCode?: number }
  | { t: 'cwd_changed'; sessionId: Bytes; cwd: string | null };

const isObj = (x: unknown): x is Record<string, unknown> => typeof x === 'object' && x !== null && !Array.isArray(x);
const keys = (o: Record<string, unknown>, req: string[], opt: string[] = []) => {
  if (!req.every(k => k in o) || !Object.keys(o).every(k => req.includes(k) || opt.includes(k))) bad();
};
const str = (x: unknown): string => typeof x === 'string' ? x : bad();
const dimv = (x: unknown): number => Number.isInteger(x) ? dim(x as number) : bad();
const i32 = (x: unknown): number => Number.isInteger(x) && (x as number) >= -(2 ** 31) && (x as number) < 2 ** 31 ? x as number : bad();
const state = (x: unknown): State => x === 'running' || x === 'exited' ? x : bad();
const label64 = (x: unknown): string => [...str(x)].length <= 64 ? x as string : bad();

/** Controles host→cliente (§2.1 respostas, §2.2, §2.3). Mesmas regras de `parseControl`; pedidos do cliente aqui são INVALID_MESSAGE. */
export function parseHostControl(text: string, connectionId: string): HostMsg {
  const o = envelope(text, connectionId);
  const E = ['v', 'type', 'connection_id'];
  const reply = (f: string[], opt: string[] = []): string => { keys(o, [...E, 'request_id', ...f], opt); uuidV4Lower(o.request_id); return o.request_id; };
  const live = (f: string[], opt: string[] = []): Bytes => { keys(o, [...E, 'session_id', ...f], opt); return uuidV4Lower(o.session_id); };
  const t = str(o.type);
  switch (t) {
    case 'session_create_ok': { const requestId = reply(['session_id']); return { t, requestId, sessionId: uuidV4Lower(o.session_id) }; }
    case 'session_detach_ok': case 'session_close_ok': case 'client_focus_ok': return { t, requestId: reply([]) };
    case 'session_list_ok': {
      const requestId = reply(['sessions', 'profiles']);
      if (!Array.isArray(o.sessions) || !Array.isArray(o.profiles)) bad();
      const sessions = o.sessions.map((s: unknown): SessionInfo => {
        if (!isObj(s)) bad();
        const x = s as Record<string, unknown>;
        keys(x, ['session_id', 'label', 'state', 'created_at_ms', 'last_activity_ms', 'cols', 'rows', 'attached_count']);
        const n = x.attached_count;
        return { sessionId: uuidV4Lower(x.session_id), label: label64(x.label), state: state(x.state), createdAtMs: parseU64(x.created_at_ms),
          lastActivityMs: parseU64(x.last_activity_ms), cols: dimv(x.cols), rows: dimv(x.rows),
          attachedCount: Number.isInteger(n) && (n as number) >= 0 && (n as number) <= 4 ? n as number : bad() };
      });
      const profiles = o.profiles.map((p: unknown) => {
        if (!isObj(p)) bad();
        const x = p as Record<string, unknown>;
        keys(x, ['profile_id', 'label']);
        return { profileId: str(x.profile_id), label: str(x.label) };
      });
      return { t, requestId, sessions, profiles };
    }
    case 'session_attach_ok': {
      const requestId = reply(['session_id', 'state', 'buffer_start', 'output_end', 'last_input_seq'], ['exit_code']);
      return { t, requestId, sessionId: uuidV4Lower(o.session_id), state: state(o.state), bufferStart: parseU64(o.buffer_start),
        outputEnd: parseU64(o.output_end), lastInputSeq: parseU64(o.last_input_seq), ...('exit_code' in o && { exitCode: i32(o.exit_code) }) };
    }
    case 'error': {
      keys(o, [...E, 'code'], ['request_id']);
      const code: Code = CODES.includes(o.code) ? o.code : bad();
      if (!('request_id' in o)) return { t, code, fatal: true };
      uuidV4Lower(o.request_id);
      return { t, requestId: o.request_id, code, fatal: false };
    }
    case 'input_ack': { const sessionId = live(['seq']); return { t, sessionId, seq: parseU64(o.seq) }; }
    case 'replay_begin': { const sessionId = live(['from_offset', 'to_offset']); return { t, sessionId, fromOffset: parseU64(o.from_offset), toOffset: parseU64(o.to_offset) }; }
    case 'replay_gap': { const sessionId = live(['requested', 'available_from']); return { t, sessionId, requested: parseU64(o.requested), availableFrom: parseU64(o.available_from) }; }
    case 'replay_end': { const sessionId = live(['offset']); return { t, sessionId, offset: parseU64(o.offset) }; }
    case 'session_exited': { const sessionId = live(['exit_code', 'final_offset']); return { t, sessionId, exitCode: i32(o.exit_code), finalOffset: parseU64(o.final_offset) }; }
    case 'screen_snapshot': {
      const sessionId = live(['offset', 'cols', 'rows', 'data']);
      const d = str(o.data);
      return { t, sessionId, offset: parseU64(o.offset), cols: dimv(o.cols), rows: dimv(o.rows), data: b64d(d, Math.floor(d.length * 3 / 4)) };
    }
    case 'command_completed': { const sessionId = live([], ['exit_code']); return { t, sessionId, ...('exit_code' in o && { exitCode: i32(o.exit_code) }) }; }
    case 'cwd_changed': { const sessionId = live(['cwd']); return { t, sessionId, cwd: o.cwd === null ? null : str(o.cwd) }; }
    default: return bad();
  }
}

/** `body` tem `type` e campos (u64 já como string via `u64s`); adiciona `v`, `connection_id`, `request_id?`. */
export function encodeControl(connectionId: string, requestId: string | undefined, body: Record<string, unknown>): string {
  return JSON.stringify({ ...body, v: 1, connection_id: connectionId, ...(requestId !== undefined && { request_id: requestId }) });
}

// ---- janela de input (§4), uma por sessão ----
export const WINDOW_BYTES = 65536;
export const WINDOW_FRAMES = 256;
/** `sent`: enviado nesta conexão; `wire`: já enviado alguma vez. */
interface Pending { seq: bigint; data: Bytes; sent: boolean; wire?: boolean }

/** `type` com conexão ativa numera e enfileira; offline guarda à parte até `confirmOffline` (§9 item 2).
 *  `take()` devolve os frames que cabem na janela (64 KiB / 256 sem ACK); o resto fica retido. */
export class InputWindow {
  online = true;
  private next = 1n;
  private q: Pending[] = [];
  private off: Bytes[] = [];
  readonly sessionId: Bytes;
  constructor(sessionId: Bytes) { this.sessionId = sessionId; }

  type(data: Bytes): void {
    for (let i = 0; i < data.length; i += MAX_IO) {
      const d = data.slice(i, i + MAX_IO);
      if (this.online) this.q.push({ seq: this.next++, data: d, sent: false }); else this.off.push(d);
    }
  }
  take(): Frame[] {
    if (!this.online) return [];
    let bytes = 0, frames = 0;
    const out: Frame[] = [];
    for (const p of this.q) {
      if (frames + 1 > WINDOW_FRAMES || bytes + p.data.length > WINDOW_BYTES) break;
      frames++; bytes += p.data.length;
      if (!p.sent) { p.sent = p.wire = true; out.push({ sessionId: this.sessionId, seq: p.seq, op: { t: 'input', data: p.data } }); }
    }
    return out;
  }
  /** `input_ack` cumulativo. */
  ack(seq: bigint): void { this.q = this.q.filter(p => p.seq > seq); }
  disconnect(): void { this.online = false; for (const p of this.q) p.sent = false; }
  /** `session_attach_ok.last_input_seq`: só reenvia o digitado online com `seq` maior. */
  attached(lastInputSeq: bigint): Frame[] {
    this.online = true;
    // Só o que já foi ao fio pode estar confirmado; o nunca enviado é renumerado após o host (não some).
    this.q = this.q.filter(p => !(p.wire && p.seq <= lastInputSeq));
    let n = lastInputSeq + 1n;
    for (const p of this.q) { if (!p.wire && p.seq < n) p.seq = n; n = p.seq + 1n; }
    if (this.next < n) this.next = n;
    return this.take();
  }
  get offline(): readonly Bytes[] { return this.off; }
  confirmOffline(): void { for (const data of this.off) this.q.push({ seq: this.next++, data, sent: false }); this.off = []; }
  discardOffline(): void { this.off = []; }
  /** `NOT_FOUND`/sessão perdida: descarta tudo; nunca reenviar para outra sessão (§4). */
  lost(): void { this.q = []; this.off = []; }
  get pending(): number { return this.q.length; }
}

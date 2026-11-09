// C2 identidade, lado offerer (docs/planos/SPEC-C2-identidade.md). Puro: sem RN/Expo, sem rede.
// Espelha desktop/src/auth.rs. No app, aleatoriedade vem de crypto.getRandomValues (polyfill expo-crypto carregado antes).
// Nunca logar URI, otk, chaves, nonces, MACs ou assinaturas.
import { ed25519 } from '@noble/curves/ed25519.js';
import { equalBytes } from '@noble/curves/utils.js';
import { hmac } from '@noble/hashes/hmac.js';
import { sha256 } from '@noble/hashes/sha2.js';

export const AUTH_TIMEOUT_MS = 30_000;
const MAX_MSG = 65536;
const MAX_URI = 512;

export type Code = 'INVALID_MESSAGE' | 'VERSION_UNSUPPORTED' | 'UNAUTHORIZED' | 'REVOKED' | 'EXPIRED' | 'LIMIT_EXCEEDED'
  | 'NOT_FOUND' | 'ALREADY_EXISTS' | 'HASH_MISMATCH' | 'IO_ERROR' | 'SERVER_NOT_ALLOWED';
const CODES: readonly Code[] = ['INVALID_MESSAGE', 'VERSION_UNSUPPORTED', 'UNAUTHORIZED', 'REVOKED', 'EXPIRED', 'LIMIT_EXCEEDED',
  'NOT_FOUND', 'ALREADY_EXISTS', 'HASH_MISMATCH', 'IO_ERROR', 'SERVER_NOT_ALLOWED'];

export class AuthError extends Error {
  readonly code: Code;
  constructor(code: Code) { super(code); this.code = code; }
}
const fail = (c: Code): never => { throw new AuthError(c); };

export type Mode = 'pair' | 'reconnect';
export type Role = 'offerer' | 'answerer';
export type Bytes = Uint8Array;

/** O que este lado observou no próprio transporte. Fingerprints por papel, nunca local/remoto (§3). */
export interface Binding { connectionId: Bytes; roomId: Bytes; fpOfferer: Bytes; fpAnswerer: Bytes }

const enc = new TextEncoder();
const len = (x: Bytes, n: number) => x.length === n ? x : fail('INVALID_MESSAGE');

// ---- base64url sem padding, canônico (igual ao crate base64 do host) ----
const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
export function b64e(b: Bytes): string {
  let s = '';
  for (let i = 0; i < b.length; i += 3) {
    const n = (b[i] << 16) | ((b[i + 1] ?? 0) << 8) | (b[i + 2] ?? 0);
    s += A[n >> 18] + A[(n >> 12) & 63] + (i + 1 < b.length ? A[(n >> 6) & 63] : '') + (i + 2 < b.length ? A[n & 63] : '');
  }
  return s;
}
/** Exige exatamente `n` bytes e codificação canônica; senão INVALID_MESSAGE. */
export function b64d(s: unknown, n: number): Bytes {
  if (typeof s !== 'string' || s.length !== Math.ceil(n * 4 / 3) || !/^[A-Za-z0-9_-]*$/.test(s)) fail('INVALID_MESSAGE');
  const str = s as string, out = new Uint8Array(n);
  let bits = 0, acc = 0, j = 0;
  for (const c of str) {
    acc = (acc << 6) | A.indexOf(c); bits += 6;
    if (bits >= 8) { bits -= 8; out[j++] = (acc >> bits) & 255; }
  }
  return b64e(out) === str ? out : fail('INVALID_MESSAGE');
}

export function uuidBytes(s: string): Bytes | null {
  if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(s)) return null;
  const h = s.replaceAll('-', '');
  return Uint8Array.from({ length: 16 }, (_, i) => parseInt(h.slice(2 * i, 2 * i + 2), 16));
}
const uuidV4Lower = (s: string) => /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(s) ? uuidBytes(s) : null;

// ---- transcript e provas (§3) ----
function lp(parts: Bytes[]): Bytes {
  const total = parts.reduce((a, p) => a + 4 + p.length, 0), out = new Uint8Array(total), v = new DataView(out.buffer);
  let o = 0;
  for (const p of parts) { v.setUint32(o, p.length); out.set(p, o + 4); o += 4 + p.length; }
  return out;
}
export function transcript(mode: Mode, b: Binding, hostPk: Bytes, clientPk: Bytes, nonceO: Bytes, nonceA: Bytes): Bytes {
  return lp([enc.encode('live-cmd/auth/v1'), Uint8Array.of(1), enc.encode(mode), enc.encode('offerer'), enc.encode('answerer'),
    len(b.connectionId, 16), len(b.roomId, 16), len(hostPk, 32), len(clientPk, 32), len(nonceO, 32), len(nonceA, 32),
    len(b.fpOfferer, 32), len(b.fpAnswerer, 32)]);
}
const labeled = (kind: 'sig' | 'mac', role: Role, t: Bytes) => { const l = lp([enc.encode(`${kind}/${role}`)]), m = new Uint8Array(l.length + t.length); m.set(l); m.set(t, l.length); return m; };
/** `seed` = semente Ed25519 de 32 bytes. */
export const sign = (seed: Bytes, role: Role, t: Bytes): Bytes => ed25519.sign(labeled('sig', role, t), seed);
/** Ed25519 estrito: `zip215:false` explícito (rejeita não canônico e ordem pequena). */
export const verify = (pk: Bytes, role: Role, t: Bytes, sig: Bytes): void => {
  let ok = false;
  try { ok = ed25519.verify(sig, labeled('sig', role, t), pk, { zip215: false }); } catch { /* entrada malformada */ }
  if (!ok) fail('UNAUTHORIZED');
};
export const mac = (otk: Bytes, role: Role, t: Bytes): Bytes => hmac(sha256, otk, labeled('mac', role, t));
/** Comparação em tempo constante (`equalBytes`). */
export const verifyMac = (otk: Bytes, role: Role, t: Bytes, tag: Bytes): void => { if (!equalBytes(mac(otk, role, t), tag)) fail('UNAUTHORIZED'); };

// ---- QR (§1) ----
/** Conteúdo do QR. Nunca logar. */
export interface Pairing { session: string; roomId: Bytes; otk: Bytes; hostPk: Bytes; server: string }

const validServer = (s: string) => {
  const m = /^([a-z0-9.-]{1,253})(?::([1-9][0-9]{0,4}))?$/.exec(s);
  return !!m && (m[2] === undefined || Number(m[2]) <= 65535);
};
/** `allowlist`: lista fixa embutida no app. Fora dela → SERVER_NOT_ALLOWED, sem conectar. */
export function parseQr(uri: string, allowlist: readonly string[]): Pairing {
  if (enc.encode(uri).length > MAX_URI) fail('INVALID_MESSAGE');
  if (!uri.startsWith('remoteterm://pair?')) fail('INVALID_MESSAGE');
  const f = new Map<string, string>();
  for (const kv of uri.slice('remoteterm://pair?'.length).split('&')) {
    const i = kv.indexOf('=');
    if (i < 0 || f.has(kv.slice(0, i))) fail('INVALID_MESSAGE');
    f.set(kv.slice(0, i), kv.slice(i + 1));
  }
  if (f.has('v') && f.get('v') !== '1') fail('VERSION_UNSUPPORTED');
  if (f.size !== 5 || !['v', 'session', 'otk', 'host_pk', 'server'].every(k => f.has(k))) fail('INVALID_MESSAGE');
  const session = f.get('session')!, server = f.get('server')!;
  if (!validServer(server)) fail('INVALID_MESSAGE');
  const roomId = uuidV4Lower(session) ?? fail('INVALID_MESSAGE');
  const p = { session, roomId, otk: b64d(f.get('otk'), 32), hostPk: b64d(f.get('host_pk'), 32), server };
  if (!allowlist.includes(server)) fail('SERVER_NOT_ALLOWED');
  return p;
}

// ---- mensagens (§2) ----
export type Msg =
  | { type: 'auth_hello'; mode: Mode; clientPk: Bytes; nonceO: Bytes }
  | { type: 'auth_challenge'; hostPk: Bytes; nonceA: Bytes; sigA: Bytes; macA?: Bytes }
  | { type: 'auth_proof'; sigO: Bytes; macO?: Bytes }
  | { type: 'auth_ok' }
  | { type: 'error'; code: Code };

export function toJson(m: Msg, connectionId: string): string {
  const o: Record<string, unknown> = { v: 1, type: m.type, connection_id: connectionId };
  switch (m.type) {
    case 'auth_hello': Object.assign(o, { mode: m.mode, role: 'offerer', client_pk: b64e(m.clientPk), nonce_o: b64e(m.nonceO) }); break;
    case 'auth_challenge': Object.assign(o, { role: 'answerer', host_pk: b64e(m.hostPk), nonce_a: b64e(m.nonceA), sig_a: b64e(m.sigA) }, m.macA && { mac_a: b64e(m.macA) }); break;
    case 'auth_proof': Object.assign(o, { sig_o: b64e(m.sigO) }, m.macO && { mac_o: b64e(m.macO) }); break;
    case 'error': o.code = m.code; break;
  }
  return JSON.stringify(o);
}

export function parseMsg(text: string, connectionId: string): Msg {
  if (enc.encode(text).length > MAX_MSG) fail('INVALID_MESSAGE');
  let o: any;
  try { o = JSON.parse(text); } catch { fail('INVALID_MESSAGE'); }
  if (typeof o !== 'object' || o === null || Array.isArray(o)) fail('INVALID_MESSAGE');
  if (o.v !== 1) fail(typeof o.v === 'number' ? 'VERSION_UNSUPPORTED' : 'INVALID_MESSAGE');
  if (o.connection_id !== connectionId) fail('INVALID_MESSAGE');
  const fields = (req: string[], opt = '') => {
    if (!req.every(k => k in o) || !Object.keys(o).every(k => ['v', 'type', 'connection_id', opt, ...req].includes(k))) fail('INVALID_MESSAGE');
  };
  const role = (want: Role) => { if (o.role !== want) fail('INVALID_MESSAGE'); };
  const opt = (k: string, n: number) => k in o ? b64d(o[k], n) : undefined;
  switch (o.type) {
    case 'auth_hello':
      fields(['mode', 'role', 'client_pk', 'nonce_o']); role('offerer');
      if (o.mode !== 'pair' && o.mode !== 'reconnect') fail('INVALID_MESSAGE');
      return { type: 'auth_hello', mode: o.mode, clientPk: b64d(o.client_pk, 32), nonceO: b64d(o.nonce_o, 32) };
    case 'auth_challenge':
      fields(['role', 'host_pk', 'nonce_a', 'sig_a'], 'mac_a'); role('answerer');
      return { type: 'auth_challenge', hostPk: b64d(o.host_pk, 32), nonceA: b64d(o.nonce_a, 32), sigA: b64d(o.sig_a, 64), macA: opt('mac_a', 32) };
    case 'auth_proof':
      fields(['sig_o'], 'mac_o');
      return { type: 'auth_proof', sigO: b64d(o.sig_o, 64), macO: opt('mac_o', 32) };
    case 'auth_ok': fields([]); return { type: 'auth_ok' };
    case 'error':
      fields(['code']);
      return { type: 'error', code: CODES.includes(o.code) ? o.code : fail('INVALID_MESSAGE') };
  }
  return fail('INVALID_MESSAGE');
}

// ---- handshake do offerer (§2, §2.1) ----
export type State = 'NEW' | 'CHALLENGED' | 'AUTHENTICATED' | 'CLOSED';
export type Random = (n: number) => Bytes;
export interface OffererOpts { otk?: Bytes; seed: Bytes; pin: Bytes; binding: Binding; connectedAt: number; random?: Random }
const defaultRandom: Random = n => globalThis.crypto.getRandomValues(new Uint8Array(n));

/** `otk` presente → pair (pin = host_pk do QR); ausente → reconnect (pin = trust store). `now`/`connectedAt` em ms monotônicos.
 *  Qualquer AuthError: enviar `{type:'error', code}` (exceto SERVER_NOT_ALLOWED) e fechar a PeerConnection inteira. */
export class Offerer {
  state: State = 'NEW';
  readonly hello: Msg;
  private readonly nonceO: Bytes;
  private readonly clientPk: Bytes;
  private readonly opts: OffererOpts;

  constructor(opts: OffererOpts) {
    this.opts = opts;
    this.nonceO = len((opts.random ?? defaultRandom)(32), 32);
    this.clientPk = ed25519.getPublicKey(opts.seed);
    this.hello = { type: 'auth_hello', mode: this.mode, clientPk: this.clientPk, nonceO: this.nonceO };
  }
  private get mode(): Mode { return this.opts.otk ? 'pair' : 'reconnect'; }

  /** Devolve o `auth_proof` após o desafio; `null` ao receber `auth_ok`. */
  handle(m: Msg, now: number): Msg | null {
    try {
      if (now - this.opts.connectedAt >= AUTH_TIMEOUT_MS) fail('EXPIRED');
      return this.step(m);
    } catch (e) { this.state = 'CLOSED'; throw e; }
  }
  private step(m: Msg): Msg | null {
    if (this.state === 'NEW' && m.type === 'auth_challenge') {
      const { otk, seed, pin, binding } = this.opts;
      if (!equalBytes(m.hostPk, pin)) fail('UNAUTHORIZED');
      const t = transcript(this.mode, binding, m.hostPk, this.clientPk, this.nonceO, m.nonceA);
      verify(m.hostPk, 'answerer', t, m.sigA);
      if (!!otk !== !!m.macA) fail('INVALID_MESSAGE');
      if (otk) verifyMac(otk, 'answerer', t, m.macA!);
      this.state = 'CHALLENGED';
      return { type: 'auth_proof', sigO: sign(seed, 'offerer', t), macO: otk && mac(otk, 'offerer', t) };
    }
    if (this.state === 'CHALLENGED' && m.type === 'auth_ok') { this.state = 'AUTHENTICATED'; return null; }
    return fail('INVALID_MESSAGE');
  }
}

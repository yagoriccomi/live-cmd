// Cliente remoto mínimo (espelha desktop/src/client.rs attempt_once): sinalização C1, WebRTC offerer,
// handshake C2 e uma sessão C3. ponytail: sem reconexão automática nem replay; adicionar com M3.
import { RTCPeerConnection, RTCIceCandidate, RTCSessionDescription } from 'react-native-webrtc';
import { randomUUID } from 'expo-crypto';
import { sha256 } from '@noble/hashes/sha2.js';
import { Offerer, b64d, parseMsg, toJson, uuidBytes, type Bytes } from './auth.ts';
import { encodeFrame, parseFrame, parseHostControl, encodeControl } from './pty.ts';
import { capsProbe, isExtraReply, parseExtraReply } from './extras.ts';

export interface Pin { hostPk: string; room: string; server: string }
/** `reset`: a sessão exibida mudou (agente retomado, SSH, troca de aba); limpar a tela. */
export interface Handlers { status(s: string): void; output(b: Bytes): void; closed(code?: string): void; reset?(): void }

const signalUrl = (server: string) => {
  const host = server.split(':')[0];
  return `${host === '127.0.0.1' || host === 'localhost' ? 'ws' : 'wss'}://${server}`;
};
const hex32 = (s: string) => Uint8Array.from(s.split(':').map(h => parseInt(h, 16)));

/** SHA-256 do certificado folha (DER) de cada lado, pelos stats (sem RTCDtlsTransport no RN). */
async function fingerprints(pc: RTCPeerConnection): Promise<[Bytes, Bytes]> {
  const stats: Map<string, any> = await pc.getStats() as any;
  const all = [...stats.values()];
  const t = all.find(s => s.type === 'transport' && s.localCertificateId);
  const fp = (id: string) => {
    const c = stats.get(id) ?? all.find(s => s.id === id);
    if (!c) throw new Error('no certificate');
    return c.base64Certificate ? sha256(Uint8Array.from(atob(c.base64Certificate), ch => ch.charCodeAt(0))) : hex32(c.fingerprint);
  };
  if (!t) throw new Error('no transport stats');
  return [fp(t.localCertificateId), fp(t.remoteCertificateId)];
}

export class Conn {
  private ws?: WebSocket;
  private pc?: RTCPeerConnection;
  private dc?: any;
  private id = randomUUID();
  private sid?: Bytes;
  /** Sessão exibida (texto), para `history_run` e troca de sessão. */
  sessionId?: string;
  private seq = 0n;
  private pending = new Map<string, (m: any) => void>();
  private done = false;

  constructor(private pin: Pin, private seed: Bytes, private otk: Bytes | undefined, private h: Handlers) {}

  start() {
    this.h.status('conectando');
    const ws = this.ws = new WebSocket(`${signalUrl(this.pin.server)}/v1/ws/${this.pin.room}`);
    const send = (type: string, payload: unknown) => ws.send(JSON.stringify({ v: 1, type, connection_id: this.id, request_id: 'host', payload }));
    ws.onopen = () => ws.send(JSON.stringify({ v: 1, type: 'hello', connection_id: this.id, request_id: 'client', payload: { role: 'mobile' } }));
    ws.onerror = () => this.close('NET');
    ws.onclose = () => this.close('NET');
    ws.onmessage = async e => {
      try {
        const m = JSON.parse(e.data);
        if (m.type === 'hello' && m.payload?.status === 'waiting') return this.h.status('aguardando PC');
        if (m.type === 'hello' && m.payload?.status === 'ready' && m.connection_id === this.id) return this.offer(send);
        if (m.connection_id !== this.id) throw new Error('signal');
        if (m.type === 'answer') await this.pc!.setRemoteDescription(new RTCSessionDescription({ type: 'answer', sdp: m.payload.sdp }));
        else if (m.type === 'ice') await this.pc!.addIceCandidate(new RTCIceCandidate(m.payload));
        else throw new Error('signal');
      } catch { this.close('NET'); }
    };
  }

  private async offer(send: (t: string, p: unknown) => void) {
    this.h.status('negociando');
    const pc = this.pc = new RTCPeerConnection({ iceServers: [{ urls: ['stun:stun.cloudflare.com:3478', 'stun:stun.l.google.com:19302'] }] });
    const dc = this.dc = pc.createDataChannel('pty-channel');
    dc.binaryType = 'arraybuffer';
    (pc as any).addEventListener('icecandidate', (e: any) => e.candidate && send('ice', e.candidate.toJSON()));
    (pc as any).addEventListener('connectionstatechange', () => { if (pc.connectionState === 'failed') this.close('NET'); });
    dc.addEventListener('open', () => this.auth().catch(e => this.close(e?.code ?? 'NET')));
    dc.addEventListener('close', () => this.close('NET'));
    const o = await pc.createOffer({});
    await pc.setLocalDescription(o);
    send('offer', { sdp: o.sdp });
  }

  private async auth() {
    this.h.status('autenticando');
    const [fpO, fpA] = await fingerprints(this.pc!);
    const off = new Offerer({ otk: this.otk, seed: this.seed, pin: b64d(this.pin.hostPk, 32),
      binding: { connectionId: uuidBytes(this.id)!, roomId: uuidBytes(this.pin.room)!, fpOfferer: fpO, fpAnswerer: fpA }, connectedAt: Date.now() });
    this.dc.send(toJson(off.hello, this.id));
    await new Promise<void>((ok, ko) => {
      this.dc.onmessage = (e: any) => {
        try {
          const m = parseMsg(e.data, this.id);
          if (m.type === 'error') throw Object.assign(new Error(m.code), { code: m.code });
          const r = off.handle(m, Date.now());
          if (r) this.dc.send(toJson(r, this.id)); else ok();
        } catch (err) { ko(err); }
      };
    });
    this.otk = undefined;
    this.dc.onmessage = (e: any) => this.recv(e.data);
    // Host antigo responde NOT_FOUND (não fatal) à pergunta de capacidades: abas extras pedem atualização.
    this.caps = await this.request(capsProbe()).then((r: any) => r.t === 'caps_ok' ? r.caps as string[] : [], () => []);
    const list = await this.request({ type: 'session_list' });
    const profile = list.profiles[0]?.profile_id;
    const created = await this.request({ type: 'session_create', profile_id: profile, label: 'Celular', cols: 80, rows: 24 });
    const att = await this.request({ type: 'session_attach', session_id: created.session_id, from_offset: null, cols: 80, rows: 24 });
    this.sid = uuidBytes(created.session_id)!;
    this.sessionId = created.session_id;
    this.seq = BigInt(att.last_input_seq);
    this.h.status('conectado');
  }

  /** Telas extras que o host entende (`caps_ok`); vazio = host antigo (abas pedem atualização). */
  caps: string[] = [];

  /** Pedido das abas Agentes/SSH/Histórico (corpo de `extras.ts`); resposta já validada. */
  extra(body: Record<string, unknown>): Promise<any> {
    if (!this.sid || this.done) return Promise.reject(Object.assign(new Error('NOT_CONNECTED'), { code: 'NOT_CONNECTED' }));
    return this.request(body);
  }

  /** Passa a exibir outra sessão do host: solta a atual e anexa a nova (com replay do buffer). */
  async switchTo(sessionId: string) {
    if (!this.sid || this.done || sessionId === this.sessionId) return;
    const old = this.sessionId;
    this.sid = undefined;
    if (old) await this.request({ type: 'session_detach', session_id: old }).catch(() => {});
    this.h.reset?.();
    const att = await this.request({ type: 'session_attach', session_id: sessionId, from_offset: null, cols: 80, rows: 24 });
    this.sid = uuidBytes(sessionId)!;
    this.sessionId = sessionId;
    this.seq = BigInt(att.last_input_seq);
  }

  private request(body: Record<string, unknown>): Promise<any> {
    const rid = randomUUID();
    return new Promise((ok, ko) => {
      const t = setTimeout(() => { this.pending.delete(rid); ko(new Error('EXPIRED')); }, 10_000);
      this.pending.set(rid, m => { clearTimeout(t); m.type === 'error' ? ko(Object.assign(new Error(m.code), { code: m.code })) : ok(m); });
      this.dc.send(encodeControl(this.id, rid, body));
    });
  }

  private recv(data: string | ArrayBuffer) {
    try {
      if (typeof data === 'string') {
        if (isExtraReply(data)) {
          const x = parseExtraReply(data, this.id), w = this.pending.get(x.requestId);
          if (w) { this.pending.delete(x.requestId); w(x); }
          return;
        }
        parseHostControl(data, this.id);
        const m = JSON.parse(data);
        if (m.type === 'error' && !('request_id' in m)) return this.close(m.code);
        const w = m.request_id && this.pending.get(m.request_id);
        if (w) { this.pending.delete(m.request_id); w(m); }
        return;
      }
      const b = new Uint8Array(data), f = parseFrame(b);
      const mine = !!this.sid && f.sessionId.every((x, i) => x === this.sid![i]);
      if (f.op.t === 'heartbeat') this.dc.send(b);
      else if (f.op.t === 'output' && mine) this.h.output(f.op.data);
      else if (f.op.t === 'exited' && mine) this.close(`saiu ${f.op.code}`);
    } catch (e: any) { this.close(e?.code ?? 'INVALID_MESSAGE'); }
  }

  input(data: Bytes) {
    if (!this.sid || this.done) return;
    for (let i = 0; i < data.length; i += 16384) {
      this.seq += 1n;
      this.dc.send(encodeFrame({ sessionId: this.sid, seq: this.seq, op: { t: 'input', data: data.slice(i, i + 16384) } }));
    }
  }

  close(code?: string) {
    if (this.done) return;
    this.done = true;
    try { this.dc?.close(); this.pc?.close(); this.ws?.close(); } catch {}
    this.h.closed(code);
  }
}

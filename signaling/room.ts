export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const ROOM_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
// Admission limits: envelope <= 64 KiB (C1); <= 64 messages per peer per second (ICE bursts fit).
export const MAX_MESSAGE_BYTES = 65_536, MAX_MESSAGES_PER_SECOND = 64;
const bytes = (s: string) => new TextEncoder().encode(s).length;
const object = (v: unknown): v is Record<string, any> => v !== null && typeof v === 'object' && !Array.isArray(v);
type Socket = { send(data: string): void; close(code: number, reason: string): void };
type Peer = { socket: Socket; role?: 'mobile' | 'host'; ended: boolean; window: number; count: number };
type Reason = 'done' | 'timeout' | 'peer_left' | 'protocol_error';

// ponytail: one local attempt in memory; authenticated reconnect/hibernation belongs to M2.
export class Room {
  peers: Peer[] = [];
  started = 0;
  private finished = false;
  phase: 'waiting' | 'ready' | 'offered' | 'answered' = 'waiting';
  candidates = 0;
  readonly connectionId: string;
  readonly now: () => number;
  constructor(connectionId: string, now = () => Date.now()) { this.connectionId = connectionId; this.now = now; }
  send(peer: Peer, type: string, payload: unknown, request_id = crypto.randomUUID() as string) {
    peer.socket.send(JSON.stringify({ v: 1, type, connection_id: this.connectionId, request_id, payload }));
  }
  add(socket: Socket): boolean {
    this.expire();
    if (this.finished || this.peers.length >= 2) return false;
    if (!this.peers.length) this.started = this.now();
    this.peers.push({ socket, ended: false, window: this.now(), count: 0 });
    return true;
  }
  expire() {
    if (this.peers.length && (this.now() - this.started >= 300_000 ||
      (this.phase !== 'answered' && this.now() - this.started >= 30_000))) this.finish('timeout');
  }
  finish(reason: Reason) {
    this.finished = true;
    const peers = this.peers.splice(0);
    this.phase = 'waiting';
    this.candidates = 0;
    for (const peer of peers) {
      try { this.send(peer, 'close', { reason }); } catch { /* Disconnected peer. */ }
      try { peer.socket.close(reason === 'protocol_error' ? 1008 : 1000, reason); } catch { /* Already closed. */ }
    }
  }
  left(socket: Socket) {
    if (this.peers.some(p => p.socket === socket)) this.finish('peer_left');
  }
  receive(socket: Socket, data: unknown) {
    this.expire();
    const peer = this.peers.find(p => p.socket === socket);
    if (!peer) return;
    let requestId: string | undefined;
    try {
      if (this.now() - peer.window >= 1000) { peer.window = this.now(); peer.count = 0; }
      if (++peer.count > MAX_MESSAGES_PER_SECOND) throw Error();
      if (typeof data !== 'string' || bytes(data) > MAX_MESSAGE_BYTES) throw Error();
      const m = JSON.parse(data);
      if (!object(m) || m.v !== 1 || m.connection_id !== this.connectionId ||
        typeof m.request_id !== 'string' || !m.request_id.length || m.request_id.length > 128 ||
        !/^[\w.-]+$/.test(m.request_id) || !object(m.payload)) throw Error();
      requestId = m.request_id;
      const p = m.payload;
      const other = this.peers.find(p => p !== peer);
      if (m.type === 'hello') {
        if (peer.role || (p.role !== 'mobile' && p.role !== 'host') || this.peers.some(x => x.role === p.role)) throw Error();
        peer.role = p.role;
        if (other?.role) {
          this.phase = 'ready';
          for (const target of this.peers) this.send(target, 'hello', { status: 'ready' }, requestId);
        } else this.send(peer, 'hello', { status: 'waiting' }, requestId);
        return;
      }
      if (!peer.role) throw Error();
      if (m.type === 'close') {
        if (!['done', 'timeout', 'peer_left', 'protocol_error'].includes(p.reason)) throw Error();
        this.finish(p.reason);
        return;
      }
      if (!other?.role) throw Error();
      if (m.type === 'offer' || m.type === 'answer') {
        if (typeof p.sdp !== 'string' || !p.sdp.trim()) throw Error();
        if (m.type === 'offer') {
          if (peer.role !== 'mobile' || this.phase !== 'ready') throw Error();
          this.phase = 'offered';
        } else {
          if (peer.role !== 'host' || this.phase !== 'offered') throw Error();
          this.phase = 'answered';
        }
      } else if (m.type === 'ice') {
        if (this.phase === 'waiting' || this.phase === 'ready' || (peer.role === 'host' && this.phase !== 'answered') || peer.ended ||
          typeof p.candidate !== 'string' || bytes(p.candidate) > 4096 ||
          !(p.sdpMid === null || typeof p.sdpMid === 'string') ||
          !(p.usernameFragment === null || typeof p.usernameFragment === 'string') ||
          !(p.sdpMLineIndex === null || (Number.isInteger(p.sdpMLineIndex) && p.sdpMLineIndex >= 0 && p.sdpMLineIndex <= 65535))) throw Error();
        if (p.candidate === '') peer.ended = true;
        else if (++this.candidates > 256) throw Error();
      } else throw Error();
      other.socket.send(data);
    } catch {
      try { this.send(peer, 'error', { code: 'INVALID_MESSAGE', message: 'Invalid signaling message' }, requestId); } catch { /* Disconnected peer. */ }
      this.finish('protocol_error');
    }
  }
}

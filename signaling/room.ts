export const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export const ROOM_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
// Admission limits: envelope <= 64 KiB (C1); <= 64 messages per peer per second (ICE bursts fit).
export const MAX_MESSAGE_BYTES = 65_536, MAX_MESSAGES_PER_SECOND = 64;
// Simultaneous clients per host room (C3 §6: 4 attachments); one spare socket so the excess client hears `busy`.
export const MAX_CLIENTS = 4, MAX_SOCKETS = MAX_CLIENTS + 2;
const bytes = (s: string) => new TextEncoder().encode(s).length;
const object = (v: unknown): v is Record<string, any> => v !== null && typeof v === 'object' && !Array.isArray(v);
type Socket = { send(data: string): void; close(code: number, reason: string): void };
// A client peer carries its own attempt: connection_id, phase, candidate count, ICE end flags and clock.
type Peer = {
  socket: Socket; role?: 'mobile' | 'host'; id: string | null; started: number; window: number; count: number;
  phase: 'waiting' | 'ready' | 'offered' | 'answered'; candidates: number; ended: boolean; hostEnded: boolean;
};
type Reason = 'done' | 'timeout' | 'peer_left' | 'protocol_error';
const REASONS = ['done', 'timeout', 'peer_left', 'protocol_error'];

// Host room (SPEC-C1-presenca): one host socket that stays, up to MAX_CLIENTS client attempts in parallel,
// each routed by the connection_id its client chose. An attempt ending closes only that client socket.
// ponytail: state in memory only; DO eviction drops every attempt and peers reconnect. Hibernation API when exposed publicly.
export class Room {
  peers: Peer[] = [];
  readonly now: () => number;
  constructor(now = () => Date.now()) { this.now = now; }
  get host() { return this.peers.find(p => p.role === 'host'); }
  clients() { return this.peers.filter(p => p.role === 'mobile'); }
  send(peer: Peer, type: string, payload: unknown, request_id = crypto.randomUUID() as string, connection_id = peer.role === 'mobile' ? peer.id : null) {
    peer.socket.send(JSON.stringify({ v: 1, type, connection_id, request_id, payload }));
  }
  add(socket: Socket): boolean {
    this.expire();
    if (this.peers.length >= MAX_SOCKETS) return false;
    this.peers.push({ socket, id: null, started: this.now(), window: this.now(), count: 0, phase: 'waiting', candidates: 0, ended: false, hostEnded: false });
    return true;
  }
  // The host waits indefinitely; any other socket has 30 s to negotiate and 300 s in total.
  expire() {
    for (const p of [...this.peers]) {
      const age = this.now() - p.started;
      if (p.role !== 'host' && (age >= 300_000 || (p.phase !== 'answered' && age >= 30_000))) this.drop(p, 'timeout');
    }
  }
  // Ends one socket. A client's end reaches the host as `close` with that client's id; the host's end ends every client.
  drop(peer: Peer, reason: Reason) {
    const i = this.peers.indexOf(peer);
    if (i < 0) return;
    this.peers.splice(i, 1);
    if (peer.role === 'host') for (const c of this.clients()) this.drop(c, 'peer_left');
    else if (peer.role === 'mobile' && peer.phase !== 'waiting' && this.host) {
      try { this.send(this.host, 'close', { reason }, undefined, peer.id); } catch { /* Disconnected host. */ }
    }
    try { this.send(peer, 'close', { reason }); } catch { /* Disconnected peer. */ }
    try { peer.socket.close(reason === 'protocol_error' ? 1008 : 1000, reason); } catch { /* Already closed. */ }
  }
  // Second host or client over the limit: drop only the newcomer, current attempts continue.
  busy(peer: Peer, requestId?: string) {
    this.peers.splice(this.peers.indexOf(peer), 1);
    const reply = (type: string, payload: unknown) => peer.socket.send(JSON.stringify({ v: 1, type, connection_id: null, request_id: requestId ?? crypto.randomUUID(), payload }));
    try { reply('error', { code: 'LIMIT_EXCEEDED', message: 'Room busy' }); reply('close', { reason: 'busy' }); } catch { /* Disconnected peer. */ }
    try { peer.socket.close(1013, 'busy'); } catch { /* Already closed. */ }
  }
  ready(client: Peer, host: Peer, requestId: string) {
    client.phase = 'ready';
    this.send(client, 'hello', { status: 'ready' }, requestId);
    this.send(host, 'hello', { status: 'ready' }, requestId, client.id);
  }
  left(socket: Socket) {
    const peer = this.peers.find(p => p.socket === socket);
    if (peer) this.drop(peer, 'peer_left');
  }
  receive(socket: Socket, data: unknown) {
    this.expire();
    const peer = this.peers.find(p => p.socket === socket);
    if (!peer) return;
    let requestId: string | undefined;
    // Who pays for an invalid message: the sender, or only the client attempt a host envelope addressed.
    let blame = peer;
    try {
      if (this.now() - peer.window >= 1000) { peer.window = this.now(); peer.count = 0; }
      // The host negotiates for every client at once.
      if (++peer.count > MAX_MESSAGES_PER_SECOND * (peer.role === 'host' ? MAX_CLIENTS : 1)) throw Error();
      if (typeof data !== 'string' || bytes(data) > MAX_MESSAGE_BYTES) throw Error();
      const m = JSON.parse(data);
      if (!object(m) || m.v !== 1 || typeof m.request_id !== 'string' || !m.request_id.length || m.request_id.length > 128 ||
        !/^[\w.-]+$/.test(m.request_id) || !object(m.payload)) throw Error();
      requestId = m.request_id;
      const p = m.payload;
      if (m.type === 'hello') {
        // hello: host sends null (learns each id from its ready); client sends a fresh lowercase UUID v4.
        const id = p.role === 'mobile' && ROOM_UUID.test(m.connection_id) ? m.connection_id : null;
        if (peer.role || m.connection_id !== id || (p.role !== 'mobile' && p.role !== 'host') || (p.role === 'mobile' && id === null) ||
          this.clients().some(c => c.id === id)) throw Error();
        // New host replaces the old one: a host socket that died without a close frame would otherwise answer busy forever.
        // ponytail: two live hosts on one room would take turns; add a host token check if that ever happens.
        if (p.role === 'host' && this.host) this.drop(this.host, 'peer_left');
        else if (p.role === 'mobile' && this.clients().length >= MAX_CLIENTS) { this.busy(peer, requestId); return; }
        peer.role = p.role;
        const host = this.host;
        if (p.role === 'mobile') {
          peer.id = id;
          if (host) this.ready(peer, host, requestId!);
          else this.send(peer, 'hello', { status: 'waiting' }, requestId);
        } else {
          const waiting = this.clients();
          if (!waiting.length) this.send(peer, 'hello', { status: 'waiting' }, requestId);
          // Clients that waited for the host get a fresh negotiation clock.
          for (const c of waiting) { c.started = this.now(); this.ready(c, peer, requestId!); }
        }
        return;
      }
      if (!peer.role) throw Error();
      let client: Peer, target: Peer;
      if (peer.role === 'mobile') {
        if (m.connection_id !== peer.id) throw Error();
        if (m.type === 'close') {
          if (!REASONS.includes(p.reason)) throw Error();
          this.drop(peer, p.reason);
          return;
        }
        if (!this.host || peer.phase === 'waiting') throw Error();
        client = peer; target = this.host;
      } else {
        const found = this.clients().find(c => c.id === m.connection_id && c.phase !== 'waiting');
        if (m.type === 'close') {
          if (!REASONS.includes(p.reason)) throw Error();
          if (found) this.drop(found, p.reason);
          return;
        }
        // Late envelope for an attempt that already ended: dropped, never relayed to another client.
        if (!found) return;
        client = target = blame = found;
      }
      if (m.type === 'offer' || m.type === 'answer') {
        if (typeof p.sdp !== 'string' || !p.sdp.trim()) throw Error();
        if (m.type === 'offer') {
          if (peer.role !== 'mobile' || client.phase !== 'ready') throw Error();
          client.phase = 'offered';
        } else {
          if (peer.role !== 'host' || client.phase !== 'offered') throw Error();
          client.phase = 'answered';
        }
      } else if (m.type === 'ice') {
        const ended = peer.role === 'host' ? client.hostEnded : client.ended;
        if (client.phase === 'ready' || (peer.role === 'host' && client.phase !== 'answered') || ended ||
          typeof p.candidate !== 'string' || bytes(p.candidate) > 4096 ||
          !(p.sdpMid === null || typeof p.sdpMid === 'string') ||
          !(p.usernameFragment === null || typeof p.usernameFragment === 'string') ||
          !(p.sdpMLineIndex === null || (Number.isInteger(p.sdpMLineIndex) && p.sdpMLineIndex >= 0 && p.sdpMLineIndex <= 65535))) throw Error();
        if (p.candidate === '') { if (peer.role === 'host') client.hostEnded = true; else client.ended = true; }
        else if (++client.candidates > 256) throw Error();
      } else throw Error();
      target.socket.send(data);
    } catch {
      try { this.send(peer, 'error', { code: 'INVALID_MESSAGE', message: 'Invalid signaling message' }, requestId, blame.role === 'mobile' ? blame.id : null); } catch { /* Disconnected peer. */ }
      this.drop(blame, 'protocol_error');
    }
  }
}

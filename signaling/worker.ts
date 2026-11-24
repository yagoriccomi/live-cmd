import { Room, UUID, ROOM_UUID, TURN_TTL_S, turnCredential } from './room.ts';
// ponytail: per-isolate memory, not distributed (multiple isolates/colos each count separately); move to a DO or Rate Limiting binding when exposed publicly.
const IP_WINDOW_MS = 60_000, MAX_CONNECTIONS_PER_IP = 20;
export const connectionsByIp = new Map<string, { start: number; count: number }>();
export function admitIp(ip: string, now = Date.now()): boolean {
  for (const [key, entry] of connectionsByIp) if (now - entry.start >= IP_WINDOW_MS) connectionsByIp.delete(key);
  const entry = connectionsByIp.get(ip) ?? { start: now, count: 0 };
  connectionsByIp.set(ip, entry);
  return ++entry.count <= MAX_CONNECTIONS_PER_IP;
}
// ROOM_ID: the one local host room (SPEC-C1-presenca); each client hello brings its own connection_id.
// PUBLIC=1 (deployed): any v4 room; the room id comes from the QR/code and is not authorization (C2 authenticates end to end).
type Env = { ROOM: DurableObjectNamespace; ROOM_ID: string; PUBLIC?: string; TURN_SECRET?: string; TURN_URLS?: string; TURN_SERVICE?: string };
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const open = env.PUBLIC === '1';
    if (!open && !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)) return new Response('Local only', { status: 403 });
    // ROOM_ID: one host room, or a comma-separated list (several local hosts, e.g. PC-to-PC test).
    const rooms = (env.ROOM_ID ?? '').split(',').map((r) => r.trim());
    if (!open && !rooms.every((r) => UUID.test(r))) return new Response('Local configuration required', { status: 503 });
    // Never log request URLs or bodies: only room_id (session) and C1 envelopes may reach the Worker (C2 §1).
    const roomId = url.pathname.startsWith('/v1/ws/') ? url.pathname.slice(7) : '';
    if (!ROOM_UUID.test(roomId) || (!open && !rooms.includes(roomId)) || url.search) return new Response('Not found', { status: 404 });
    if (request.method !== 'GET' || request.headers.get('Upgrade')?.toLowerCase() !== 'websocket') return new Response('WebSocket required', { status: 426 });
    // No browser access: local CLI peers only, not authentication.
    if (request.headers.has('Origin')) return new Response('Origin denied', { status: 403 });
    if (!admitIp(request.headers.get('CF-Connecting-IP') ?? 'local')) return new Response('Too many connections', { status: 429 });
    return env.ROOM.get(env.ROOM.idFromName(roomId)).fetch(request);
  }
};
// Reusable: no consumed marker. Room id is not authorization; C2 authenticates end to end (SPEC-C1-presenca §5).
// Hibernation API: an idle host socket costs no Durable Object duration (the free tier ran out with accept()).
// Peer state lives in each socket's attachment and is rebuilt when the object wakes.
export class SignalingRoom {
  room = new Room();
  ctx: DurableObjectState;
  env: Env;
  constructor(ctx: DurableObjectState, env: Env) {
    this.ctx = ctx;
    this.env = env;
    for (const ws of ctx.getWebSockets()) {
      const peer = ws.deserializeAttachment();
      if (peer) this.room.peers.push({ ...peer, socket: ws });
    }
  }
  async fetch(): Promise<Response> {
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    // Socket cap (MAX_SOCKETS); per-role limits answer busy inside the room.
    if (!this.room.add(server)) return new Response('Room busy', { status: 409 });
    this.ctx.acceptWebSocket(server);
    await this.sync();
    return new Response(null, { status: 101, webSocket: client });
  }
  // Refreshes the TURN credential when under 5 h of life is left, so every hello carries at least 5 h.
  async refreshTurn() {
    const { TURN_SECRET, TURN_URLS, TURN_SERVICE } = this.env;
    if (!TURN_SECRET) return;
    const expiry = Number(this.room.turn?.username.split(':')[0] ?? 0);
    if (expiry - Date.now() / 1000 > TURN_TTL_S - 3600) return;
    this.room.turn = await turnCredential(TURN_SECRET, (TURN_URLS ?? '').split(',').map(u => u.trim()).filter(Boolean), TURN_SERVICE || 'live-cmd');
  }
  async webSocketMessage(ws: WebSocket, data: string | ArrayBuffer) { await this.refreshTurn(); this.room.receive(ws, data); await this.sync(); }
  async webSocketClose(ws: WebSocket) { this.room.left(ws); await this.sync(); }
  async webSocketError(ws: WebSocket) { this.room.left(ws); await this.sync(); }
  async alarm() { this.room.expire(); await this.sync(); }
  // Saves every peer; wakes only while a client attempt can still expire, so a lone host sleeps.
  async sync() {
    for (const { socket, ...peer } of this.room.peers) try { (socket as WebSocket).serializeAttachment(peer); } catch { /* Closed socket. */ }
    if (this.room.peers.some(p => p.role !== 'host')) await this.ctx.storage.setAlarm(Date.now() + 5_000);
    else await this.ctx.storage.deleteAlarm();
  }
}

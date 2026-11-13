import { Room, UUID, ROOM_UUID } from './room.ts';
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
type Env = { ROOM: DurableObjectNamespace; ROOM_ID: string };
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (!['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname)) return new Response('Local only', { status: 403 });
    if (!env.ROOM_ID || !UUID.test(env.ROOM_ID)) return new Response('Local configuration required', { status: 503 });
    // Never log request URLs or bodies: only room_id (session) and C1 envelopes may reach the Worker (C2 §1).
    const roomId = url.pathname.startsWith('/v1/ws/') ? url.pathname.slice(7) : '';
    if (!ROOM_UUID.test(roomId) || roomId !== env.ROOM_ID || url.search) return new Response('Not found', { status: 404 });
    if (request.method !== 'GET' || request.headers.get('Upgrade')?.toLowerCase() !== 'websocket') return new Response('WebSocket required', { status: 426 });
    // No browser access: local CLI peers only, not authentication.
    if (request.headers.has('Origin')) return new Response('Origin denied', { status: 403 });
    if (!admitIp(request.headers.get('CF-Connecting-IP') ?? 'local')) return new Response('Too many connections', { status: 429 });
    return env.ROOM.get(env.ROOM.idFromName(env.ROOM_ID)).fetch(request);
  }
};
// Reusable: no consumed marker. Room id is not authorization; C2 authenticates end to end (SPEC-C1-presenca §5).
export class SignalingRoom {
  room = new Room();
  timer?: ReturnType<typeof setInterval>;
  async fetch(): Promise<Response> {
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);
    // Socket cap (MAX_SOCKETS); per-role limits answer busy inside the room.
    if (!this.room.add(server)) return new Response('Room busy', { status: 409 });
    server.accept();
    const cleanup = () => {
      if (!this.room.peers.length && this.timer !== undefined) { clearInterval(this.timer); this.timer = undefined; }
    };
    server.addEventListener('message', event => { this.room.receive(server, event.data); cleanup(); });
    for (const type of ['close', 'error'] as const) server.addEventListener(type, () => { this.room.left(server); cleanup(); });
    this.timer ??= setInterval(() => { this.room.expire(); cleanup(); }, 100);
    return new Response(null, { status: 101, webSocket: client });
  }
}

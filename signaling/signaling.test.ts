import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Room, MAX_CLIENTS, MAX_SOCKETS } from './room.ts';
import worker, { admitIp, connectionsByIp } from './worker.ts';

// End to end through Miniflare: one host socket, two clients negotiating at once, one leaves, a new one arrives.
test('Miniflare host room serves simultaneous clients by connection_id', async () => {
  const { Miniflare, convertV4MiniflareOptions } = await import('miniflare');
  const { build } = await import('esbuild');
  const { fileURLToPath } = await import('node:url');
  const directory = fileURLToPath(new URL('.', import.meta.url));
  const bundle = await build({ entryPoints: [`${directory}worker.ts`], bundle: true, write: false, format: 'esm', platform: 'browser' });
  const roomId = crypto.randomUUID();
  const mf = new Miniflare(convertV4MiniflareOptions({
    modules: true, script: bundle.outputFiles[0].text, compatibilityDate: '2026-11-06',
    durableObjects: { ROOM: { className: 'SignalingRoom', useSQLite: true } }, name: 'signaling-test', bindings: { ROOM_ID: roomId }
  }));
  const open = () => mf.dispatchFetch(`http://localhost/v1/ws/${roomId}`, { headers: { Upgrade: 'websocket' } });
  const connect = async () => { const ws = (await open()).webSocket!; ws.accept(); return ws; };
  const next = (ws: any, type: string, id?: string) => new Promise<any>((resolve, reject) => {
    const timer = setTimeout(() => reject(Error(`${type} timeout`)), 5000);
    ws.addEventListener('message', (event: any) => { const m = JSON.parse(event.data); if (m.type === type && (id === undefined || m.connection_id === id)) { clearTimeout(timer); resolve(m); } });
  });
  const message = (type: string, connection_id: string | null, payload: unknown) => JSON.stringify({ v: 1, type, connection_id, request_id: type, payload });
  try {
    const host = await connect();
    const waiting = next(host, 'hello');
    host.send(message('hello', null, { role: 'host' }));
    assert.equal((await waiting).payload.status, 'waiting');
    const join = async () => {
      const ws = await connect(), id = crypto.randomUUID();
      const ready = next(host, 'hello', id);
      ws.send(message('hello', id, { role: 'mobile' }));
      assert.equal((await ready).payload.status, 'ready');
      return { ws, id };
    };
    const a = await join(), b = await join();
    const offers = [next(host, 'offer', a.id), next(host, 'offer', b.id)];
    a.ws.send(message('offer', a.id, { sdp: 'offer-a' })); b.ws.send(message('offer', b.id, { sdp: 'offer-b' }));
    assert.deepEqual((await Promise.all(offers)).map(m => m.payload.sdp), ['offer-a', 'offer-b']);
    const answer = next(b.ws, 'answer');
    host.send(message('answer', b.id, { sdp: 'answer-b' }));
    assert.equal((await answer).connection_id, b.id);
    const closed = next(host, 'close', a.id);
    a.ws.send(message('close', a.id, { reason: 'done' }));
    assert.equal((await closed).payload.reason, 'done');
    const c = await join();
    assert.notEqual(c.id, a.id);
  } finally {
    await mf.dispose();
  }
});

function setup() {
  let now = 0;
  const id = crypto.randomUUID();
  const room = new Room(() => now);
  const socket = () => ({ messages: [] as any[], closed: false, send(s: string) { this.messages.push(JSON.parse(s)); }, close() { this.closed = true; } });
  const mobile = socket(), host = socket();
  room.add(mobile); room.add(host);
  const send = (peer: typeof mobile, type: string, payload: any, connection_id: string | null = type === 'hello' && payload.role === 'host' ? null : id) => room.receive(peer, JSON.stringify({ v: 1, type, connection_id, request_id: crypto.randomUUID(), payload }));
  const ready = () => { send(mobile, 'hello', { role: 'mobile' }); send(host, 'hello', { role: 'host' }); };
  const negotiated = () => { ready(); send(mobile, 'offer', { sdp: 'offer' }); send(host, 'answer', { sdp: 'answer' }); };
  // Extra client with its own connection_id, already through hello.
  const client = (cid = crypto.randomUUID()) => { const c = socket(); room.add(c); send(c, 'hello', { role: 'mobile' }, cid); return { c, cid }; };
  return { id, room, socket, mobile, host, send, ready, negotiated, client, advance: (ms: number) => { now += ms; room.expire(); } };
}
const ice = (candidate = 'candidate') => ({ candidate, sdpMid: null, sdpMLineIndex: null, usernameFragment: null });
test('relays offer answer ICE; done closes the client, host stays', () => {
  const s = setup(); s.negotiated();
  s.send(s.mobile, 'ice', ice()); s.send(s.host, 'ice', ice(''));
  assert.equal(s.host.messages.at(-1).type, 'ice');
  assert.equal(s.mobile.messages.at(-1).payload.candidate, '');
  s.send(s.mobile, 'close', { reason: 'done' });
  assert.ok(s.mobile.closed); assert.ok(!s.host.closed); assert.equal(s.room.peers.length, 1);
  const last = s.host.messages.at(-1);
  assert.deepEqual([last.type, last.connection_id, last.payload.reason], ['close', s.id, 'done']);
});
for (const scenario of ['wrong id', 'null client id', 'non-v4 client id', 'answer first', 'wrong offerer', 'binary', 'bad json', 'oversized', 'invalid ice', 'ice ended', 'candidate limit', 'candidate bytes', 'duplicate hello', 'rate limit']) {
  test(`rejects ${scenario}: only that client attempt ends`, () => {
    const s = setup();
    if (scenario === 'wrong id') { s.ready(); s.send(s.mobile, 'offer', { sdp: 'o' }, crypto.randomUUID()); }
    if (scenario === 'null client id') s.send(s.mobile, 'hello', { role: 'mobile' }, null);
    if (scenario === 'non-v4 client id') s.send(s.mobile, 'hello', { role: 'mobile' }, s.id.toUpperCase());
    if (scenario === 'answer first') { s.ready(); s.send(s.host, 'answer', { sdp: 'a' }); }
    if (scenario === 'wrong offerer') { s.ready(); s.send(s.host, 'offer', { sdp: 'a' }); }
    if (scenario === 'binary') s.room.receive(s.mobile, new ArrayBuffer(1));
    if (scenario === 'bad json') s.room.receive(s.mobile, '{');
    if (scenario === 'oversized') s.room.receive(s.mobile, ' '.repeat(65537));
    if (scenario === 'invalid ice') { s.negotiated(); s.send(s.mobile, 'ice', { ...ice(), sdpMLineIndex: -1 }); }
    if (scenario === 'ice ended') { s.negotiated(); s.send(s.mobile, 'ice', ice('')); s.send(s.mobile, 'ice', ice()); }
    if (scenario === 'candidate limit') { s.negotiated(); for (let i = 0; i < 257; i++) { if (i % 50 === 0) s.advance(1000); s.send(s.mobile, 'ice', ice()); } }
    if (scenario === 'candidate bytes') { s.negotiated(); s.send(s.mobile, 'ice', ice('é'.repeat(2049))); }
    if (scenario === 'duplicate hello') { s.ready(); s.send(s.mobile, 'hello', { role: 'mobile' }); }
    if (scenario === 'rate limit') { s.negotiated(); for (let i = 0; i < 64; i++) s.send(s.mobile, 'ice', ice()); }
    assert.ok(s.mobile.closed); assert.equal(s.mobile.messages.at(-1).payload.reason, 'protocol_error');
    assert.ok(!s.room.peers.some(p => p.socket === s.mobile));
    if (!s.room.host) s.send(s.host, 'hello', { role: 'host' });
    assert.ok(!s.host.closed, 'host socket survives a client error');
  });
}
test('invalid host hello ends the host and every client', () => {
  const s = setup(); s.send(s.mobile, 'hello', { role: 'mobile' });
  s.send(s.host, 'hello', { role: 'host' }); s.send(s.host, 'hello', { role: 'host' });
  assert.ok(s.host.closed); assert.ok(s.mobile.closed); assert.equal(s.room.peers.length, 0);
  assert.equal(s.mobile.messages.at(-1).payload.reason, 'peer_left');
});
test('host hello with a connection_id is a protocol error', () => {
  const s = setup(); s.send(s.host, 'hello', { role: 'host' }, s.id);
  assert.ok(s.host.closed); assert.equal(s.host.messages.at(-1).payload.reason, 'protocol_error');
});
test('handshake expires exactly at 30 seconds, host stays', () => {
  const s = setup(); s.ready(); s.advance(29999); assert.equal(s.room.peers.length, 2);
  s.advance(1);
  assert.deepEqual([s.host.messages.at(-1).payload.reason, s.host.messages.at(-1).connection_id], ['timeout', s.id]);
  assert.ok(s.mobile.closed); assert.ok(!s.host.closed);
});
test('answered attempt expires at five minutes', () => {
  const s = setup(); s.negotiated(); s.advance(299999); assert.equal(s.room.peers.length, 2);
  s.advance(1); assert.equal(s.room.peers.length, 1); assert.ok(s.mobile.closed);
});
test('client disconnect tells the host peer_left with its id', () => {
  const s = setup(); s.negotiated(); s.send(s.mobile, 'ice', ice()); s.room.left(s.mobile);
  assert.deepEqual([s.host.messages.at(-1).payload.reason, s.host.messages.at(-1).connection_id], ['peer_left', s.id]);
});
test('host disconnect ends every client attempt', () => {
  const s = setup(); s.negotiated(); const b = s.client();
  s.room.left(s.host);
  assert.ok(s.mobile.closed); assert.ok(b.c.closed); assert.equal(s.room.peers.length, 0);
});
test('two clients negotiate in parallel without leaking connection_ids', () => {
  const s = setup(); s.ready(); const b = s.client();
  assert.deepEqual([b.c.messages.at(-1).payload.status, b.c.messages.at(-1).connection_id], ['ready', b.cid]);
  assert.deepEqual(s.host.messages.filter(m => m.payload.status === 'ready').map(m => m.connection_id), [s.id, b.cid]);
  s.send(s.mobile, 'offer', { sdp: 'offer-a' }); s.send(b.c, 'offer', { sdp: 'offer-b' }, b.cid);
  s.send(s.host, 'answer', { sdp: 'answer-b' }, b.cid); s.send(s.host, 'answer', { sdp: 'answer-a' });
  s.send(s.host, 'ice', ice('cb'), b.cid); s.send(s.mobile, 'ice', ice('ca'));
  assert.deepEqual(s.mobile.messages.filter(m => m.type === 'answer').map(m => m.payload.sdp), ['answer-a']);
  assert.deepEqual(b.c.messages.filter(m => m.type === 'answer').map(m => m.payload.sdp), ['answer-b']);
  assert.ok(s.mobile.messages.every(m => m.connection_id === s.id || m.connection_id === null));
  assert.ok(b.c.messages.every(m => m.connection_id === b.cid || m.connection_id === null));
  assert.deepEqual(s.host.messages.filter(m => m.type === 'offer').map(m => [m.connection_id, m.payload.sdp]), [[s.id, 'offer-a'], [b.cid, 'offer-b']]);
  // Client A leaves: B keeps negotiating.
  s.send(s.mobile, 'close', { reason: 'done' });
  s.send(b.c, 'ice', ice('cb2'), b.cid);
  assert.equal(s.host.messages.at(-1).payload.candidate, 'cb2'); assert.ok(!b.c.closed); assert.ok(!s.host.closed);
});
test('host error addressed to one client ends only that client', () => {
  const s = setup(); s.negotiated(); const b = s.client();
  s.send(s.host, 'answer', { sdp: 'a' }, b.cid);
  assert.ok(b.c.closed); assert.ok(!s.mobile.closed); assert.ok(!s.host.closed);
  assert.equal(s.host.messages.find(m => m.type === 'error').connection_id, b.cid);
});
test('host envelope for an ended attempt is dropped, not relayed', () => {
  const s = setup(); s.negotiated(); s.send(s.mobile, 'close', { reason: 'done' });
  const b = s.client(); s.send(b.c, 'offer', { sdp: 'o' }, b.cid);
  s.send(s.host, 'answer', { sdp: 'stale' });
  assert.ok(!b.c.messages.some(m => m.type === 'answer')); assert.ok(!b.c.closed); assert.ok(!s.host.closed);
});
test('old attempt offer replayed with its stale id is rejected', () => {
  const s = setup(); s.negotiated();
  const offer = JSON.stringify(s.host.messages.find(m => m.type === 'offer'));
  s.send(s.mobile, 'close', { reason: 'done' });
  const b = s.client(); s.room.receive(b.c, offer);
  assert.equal(s.host.messages.filter(m => m.type === 'offer').length, 1, 'old offer must not reach the host again');
  assert.ok(b.c.closed); assert.ok(!s.host.closed);
});
test('reused connection_id of a live client is rejected', () => {
  const s = setup(); s.ready(); const dup = s.socket(); s.room.add(dup);
  s.send(dup, 'hello', { role: 'mobile' }, s.id);
  assert.ok(dup.closed); assert.ok(!s.mobile.closed);
});
test(`client over ${MAX_CLIENTS} gets busy; the others continue`, () => {
  const s = setup(); s.ready();
  const others = Array.from({ length: MAX_CLIENTS - 1 }, () => s.client());
  const extra = s.client();
  assert.deepEqual(extra.c.messages.map(m => [m.type, m.payload.code ?? m.payload.reason]), [['error', 'LIMIT_EXCEEDED'], ['close', 'busy']]);
  assert.ok(extra.c.closed); assert.ok(others.every(o => !o.c.closed)); assert.equal(s.room.clients().length, MAX_CLIENTS);
  s.send(s.mobile, 'close', { reason: 'done' });
  assert.equal(s.client().c.messages.at(-1).payload.status, 'ready', 'a freed slot admits a new client');
});
// Regression: host socket died without a close frame (sleep, network drop); the restarted host got busy forever.
test('new host replaces a stale host; clients reconnect to the new one', () => {
  const s = setup(); s.negotiated();
  const second = s.socket(); s.room.add(second); s.send(second, 'hello', { role: 'host' });
  assert.ok(s.host.closed, 'stale host dropped'); assert.ok(s.mobile.closed, 'its attempts end with peer_left');
  assert.ok(!second.closed); assert.equal(second.messages.at(-1).payload.status, 'waiting');
  assert.equal(s.room.host?.socket, second);
  s.room.left(s.host); assert.equal(s.room.host?.socket, second, 'late close of the stale socket is ignored');
  assert.equal(s.client().c.messages.at(-1).payload.status, 'ready', 'a new client reaches the new host');
});
test('clients waiting for the host all get ready when it arrives', () => {
  const s = setup(); s.send(s.mobile, 'hello', { role: 'mobile' }); const b = s.client();
  assert.equal(b.c.messages.at(-1).payload.status, 'waiting');
  s.advance(29_000);
  s.send(s.host, 'hello', { role: 'host' });
  assert.deepEqual(s.host.messages.map(m => m.connection_id), [s.id, b.cid]);
  s.advance(29_999); assert.ok(!s.mobile.closed, 'clock restarts when the host arrives');
  s.advance(1); assert.ok(s.mobile.closed); assert.ok(b.c.closed); assert.ok(!s.host.closed);
});
test('idle host waits past both timeouts', () => {
  const s = setup(); s.room.left(s.mobile); s.send(s.host, 'hello', { role: 'host' });
  s.advance(300_001); assert.equal(s.room.peers.length, 1); assert.ok(!s.host.closed);
  const mobile = s.socket(); s.room.add(mobile); s.advance(29_999); s.send(mobile, 'hello', { role: 'mobile' });
  assert.equal(s.host.messages.at(-1).payload.status, 'ready');
  s.advance(1); assert.equal(s.host.messages.at(-1).payload.reason, 'timeout', 'attempt clock starts at client socket');
});
test('unidentified socket expires at 30 seconds', () => {
  const s = setup(); s.room.left(s.mobile);
  const lone = s.socket(); s.room.add(lone); s.advance(30_000); assert.ok(lone.closed);
});
test('socket over the room limit rejected without disconnecting peers', () => {
  const s = setup();
  for (let i = 2; i < MAX_SOCKETS; i++) assert.ok(s.room.add({ send() {}, close() {} }));
  assert.equal(s.room.add({ send() {}, close() {} }), false); assert.equal(s.room.peers.length, MAX_SOCKETS);
});
test('rate window resets after one second', () => {
  const s = setup(); s.negotiated();
  for (let i = 0; i < 61; i++) s.send(s.mobile, 'ice', ice());
  s.advance(1000);
  for (let i = 0; i < 64; i++) s.send(s.mobile, 'ice', ice());
  assert.equal(s.room.peers.length, 2);
});
test('worker keeps local-only checks before the Durable Object', async () => {
  let reached = 0;
  const env = (ROOM_ID: string) => ({ ROOM_ID, ROOM: { idFromName() {}, get() { reached++; return { fetch: () => new Response(null) }; } } }) as any;
  const ws = { headers: { Upgrade: 'websocket' } };
  for (const id of ['00000000-0000-1000-8000-000000000000', crypto.randomUUID().toUpperCase(), '00000000-0000-4000-c000-000000000000']) {
    assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, ws), env(id))).status, 404);
  }
  const id = crypto.randomUUID();
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}?otk=x`, ws), env(id))).status, 404);
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${crypto.randomUUID()}`, ws), env(id))).status, 404, 'other room');
  assert.equal((await worker.fetch(new Request(`http://example.com/v1/ws/${id}`, ws), env(id))).status, 403);
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, { headers: { Upgrade: 'websocket', Origin: 'http://x' } }), env(id))).status, 403);
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`), env(id))).status, 426);
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, ws), env(''))).status, 503);
  assert.equal(reached, 0);
  connectionsByIp.clear();
  assert.notEqual((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, ws), env(id))).status, 404);
  assert.equal(reached, 1);
  const other = crypto.randomUUID();
  assert.notEqual((await worker.fetch(new Request(`http://localhost/v1/ws/${other}`, ws), env(`${id}, ${other}`))).status, 404, 'room list');
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, ws), env(`${id},bad`))).status, 503);
  connectionsByIp.clear();
});
test('per-IP connection limit with window reset', () => {
  connectionsByIp.clear();
  for (let i = 0; i < 20; i++) assert.ok(admitIp('1.2.3.4', 0));
  assert.equal(admitIp('1.2.3.4', 59_999), false);
  assert.ok(admitIp('5.6.7.8', 59_999), 'other IPs unaffected');
  assert.ok(admitIp('1.2.3.4', 60_000), 'window resets');
  connectionsByIp.clear();
});

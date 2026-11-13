import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Room } from './room.ts';
import worker, { admitIp, connectionsByIp } from './worker.ts';

// End to end through Miniflare: same room, two attempts, busy third socket, host rejoin.
test('Miniflare reuses host room with per-attempt connection_id', async () => {
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
  const next = (ws: any, type: string) => new Promise<any>((resolve, reject) => {
    const timer = setTimeout(() => reject(Error(`${type} timeout`)), 5000);
    ws.addEventListener('message', (event: any) => { const m = JSON.parse(event.data); if (m.type === type) { clearTimeout(timer); resolve(m); } });
  });
  const message = (type: string, connection_id: string | null, payload: unknown) => JSON.stringify({ v: 1, type, connection_id, request_id: type, payload });
  try {
    for (const attempt of [1, 2]) {
      const host = await connect();
      const waiting = next(host, 'hello');
      host.send(message('hello', null, { role: 'host' }));
      assert.equal((await waiting).payload.status, 'waiting');
      const mobile = await connect();
      const id = crypto.randomUUID();
      const ready = next(host, 'hello');
      mobile.send(message('hello', id, { role: 'mobile' }));
      const got = await ready;
      assert.deepEqual([got.payload.status, got.connection_id], ['ready', id], `attempt ${attempt}`);
      assert.equal((await open()).status, 409, 'third socket busy');
      const offer = next(host, 'offer');
      mobile.send(message('offer', id, { sdp: 'offer' }));
      assert.equal((await offer).connection_id, id);
      const closed = next(host, 'close');
      mobile.send(message('close', id, { reason: 'done' }));
      assert.equal((await closed).payload.reason, 'done');
    }
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
  return { id, room, socket, mobile, host, send, ready, negotiated, advance: (ms: number) => { now += ms; room.expire(); } };
}
const ice = (candidate = 'candidate') => ({ candidate, sdpMid: null, sdpMLineIndex: null, usernameFragment: null });
test('relays offer answer ICE and clears on done', () => {
  const s = setup(); s.negotiated();
  s.send(s.mobile, 'ice', ice()); s.send(s.host, 'ice', ice(''));
  assert.equal(s.host.messages.at(-1).type, 'ice');
  assert.equal(s.mobile.messages.at(-1).payload.candidate, '');
  s.send(s.mobile, 'close', { reason: 'done' });
  assert.equal(s.room.peers.length, 0); assert.ok(s.host.closed);
});
for (const scenario of ['wrong id', 'host id', 'null client id', 'non-v4 client id', 'answer first', 'wrong offerer', 'binary', 'bad json', 'oversized', 'invalid ice', 'ice ended', 'candidate limit', 'candidate bytes', 'duplicate hello', 'rate limit']) {
  test(`rejects ${scenario} and clears both peers`, () => {
    const s = setup();
    if (scenario === 'wrong id') { s.ready(); s.send(s.mobile, 'offer', { sdp: 'o' }, crypto.randomUUID()); }
    if (scenario === 'host id') s.send(s.host, 'hello', { role: 'host' }, s.id);
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
    assert.ok(s.mobile.closed); assert.ok(s.host.closed); assert.equal(s.room.peers.length, 0);
    assert.equal(s.mobile.messages.at(-1).payload.reason, 'protocol_error');
  });
}
test('handshake expires exactly at 30 seconds', () => {
  const s = setup(); s.ready(); s.advance(29999); assert.equal(s.room.peers.length, 2);
  s.advance(1); assert.equal(s.host.messages.at(-1).payload.reason, 'timeout');
});
test('answered room expires at five minutes', () => {
  const s = setup(); s.negotiated(); s.advance(299999); assert.equal(s.room.peers.length, 2);
  s.advance(1); assert.equal(s.room.peers.length, 0);
});
test('disconnect clears peer and candidate state', () => {
  const s = setup(); s.negotiated(); s.send(s.mobile, 'ice', ice()); s.room.left(s.mobile);
  assert.equal(s.room.candidates, 0); assert.equal(s.host.messages.at(-1).payload.reason, 'peer_left');
});
for (const reason of ['done', 'timeout', 'peer_left', 'protocol_error'] as const) {
  test(`host rejoins after ${reason}; old attempt replay rejected`, () => {
    const s = setup(); s.negotiated();
    const offer = JSON.stringify(s.host.messages.find(m => m.type === 'offer'));
    s.room.finish(reason);
    assert.equal(s.room.connectionId, null);
    s.advance(300001);
    const mobile = s.socket(), host = s.socket();
    assert.deepEqual([s.room.add(host), s.room.add(mobile)], [true, true]);
    s.send(host, 'hello', { role: 'host' });
    s.room.receive(mobile, offer);
    assert.equal(host.messages.filter(m => m.type === 'offer').length, 0, 'old offer must not reach a new peer');
    assert.equal(s.room.peers.length, 0, 'replay ends the new attempt');
    const fresh = crypto.randomUUID(), h = s.socket(), m = s.socket();
    s.room.add(h); s.room.add(m);
    s.send(h, 'hello', { role: 'host' }); s.send(m, 'hello', { role: 'mobile' }, fresh);
    assert.deepEqual([h.messages.at(-1).payload.status, h.messages.at(-1).connection_id], ['ready', fresh]);
    s.send(m, 'offer', { sdp: 'o' }, fresh);
    assert.equal(h.messages.at(-1).type, 'offer');
  });
}
test('idle host waits past both timeouts', () => {
  const s = setup(); s.room.left(s.mobile);
  const host = s.socket(); s.room.add(host); s.send(host, 'hello', { role: 'host' });
  s.advance(300_001); assert.equal(s.room.peers.length, 1); assert.ok(!host.closed);
  const mobile = s.socket(); s.room.add(mobile); s.advance(29_999); s.send(mobile, 'hello', { role: 'mobile' });
  assert.equal(host.messages.at(-1).payload.status, 'ready');
  s.advance(1); assert.equal(host.messages.at(-1).payload.reason, 'timeout', 'attempt clock starts at client socket');
});
test('unidentified socket expires at 30 seconds', () => {
  const s = setup(); s.room.left(s.mobile);
  const lone = s.socket(); s.room.add(lone); s.advance(30_000); assert.ok(lone.closed);
});
for (const role of ['mobile', 'host']) {
  test(`duplicate ${role} gets busy without disturbing the attempt`, () => {
    const s = setup();
    const first = s.socket(), second = s.socket();
    s.room.finish('done'); s.room.add(first); s.room.add(second);
    s.send(first, 'hello', { role }); s.send(second, 'hello', { role }, role === 'mobile' ? crypto.randomUUID() : null);
    assert.deepEqual(second.messages.map(m => [m.type, m.payload.code ?? m.payload.reason]), [['error', 'LIMIT_EXCEEDED'], ['close', 'busy']]);
    assert.ok(second.closed); assert.ok(!first.closed); assert.equal(s.room.peers.length, 1);
    s.room.left(second); assert.equal(s.room.peers.length, 1, 'late close of busy socket is ignored');
    if (role === 'mobile') assert.equal(s.room.connectionId, s.id, 'first client keeps its id');
  });
}
test('third participant rejected without disconnecting pair', () => {
  const s = setup(); assert.equal(s.room.add({ send() {}, close() {} }), false); assert.equal(s.room.peers.length, 2);
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
});
test('per-IP connection limit with window reset', () => {
  connectionsByIp.clear();
  for (let i = 0; i < 20; i++) assert.ok(admitIp('1.2.3.4', 0));
  assert.equal(admitIp('1.2.3.4', 59_999), false);
  assert.ok(admitIp('5.6.7.8', 59_999), 'other IPs unaffected');
  assert.ok(admitIp('1.2.3.4', 60_000), 'window resets');
  connectionsByIp.clear();
});

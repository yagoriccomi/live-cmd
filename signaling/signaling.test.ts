import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Room } from './room.ts';
import worker, { SignalingRoom, admitIp, connectionsByIp } from './worker.ts';

// Exercise the DO wrapper with shared durable data and serialized input gates.
test('durable admission survives reconstruction, serializes peers and fails closed', async () => {
  const originalResponse = globalThis.Response;
  const originalPair = (globalThis as any).WebSocketPair;
  let upgrades = 0;
  (globalThis as any).Response = class {
    status: number;
    constructor(_body: unknown, init: any) { this.status = init.status; if (init.status === 101) upgrades++; }
  };
  (globalThis as any).WebSocketPair = class {
    0 = {};
    1 = { accept() {}, send() {}, close() {}, addEventListener() {} };
  };
  const instances: SignalingRoom[] = [];
  try {
    const data = new Map<string, unknown>();
    let writes = 0, fail = '';
    let release: (() => void) | undefined;
    let pending: Promise<void> = Promise.resolve();
    const storage = {
      async get(key: string) { if (fail === 'get') throw Error('storage read'); return data.get(key); },
      async put(key: string, value: unknown) {
        writes++; await pending;
        if (fail === 'put') throw Error('storage write');
        data.set(key, value);
      }
    };
    const create = () => {
      let gate = Promise.resolve();
      const state = { storage, blockConcurrencyWhile(fn: () => Promise<any>) {
        const result = gate.then(fn); gate = result.catch(() => {}); return result;
      } };
      const instance = new SignalingRoom(state as any, { CONNECTION_ID: '00000000-0000-0000-0000-000000000001' } as any);
      instances.push(instance); return instance;
    };
    const first = create();
    pending = new Promise(resolve => { release = resolve; });
    const admissions = [first.fetch(), first.fetch(), first.fetch()];
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(upgrades, 0, 'no upgrade before durable commit');
    release!();
    assert.deepEqual((await Promise.all(admissions)).map(r => r.status), [101, 101, 409]);
    assert.equal(writes, 1);
    assert.equal(first.room.peers.length, 2);
    const stored = [...data.entries()];
    assert.equal((await create().fetch()).status, 409, 'crash before finish must not reopen');
    first.room.finish('done');
    assert.equal((await create().fetch()).status, 409);
    assert.deepEqual([...data.entries()], stored, 'restart must not renew attempt');
    for (const operation of ['get', 'put']) {
      data.clear(); fail = operation;
      const broken = create();
      const before = upgrades;
      assert.equal((await broken.fetch()).status, 503);
      fail = '';
      assert.notEqual((await broken.fetch()).status, 101, 'failed instance remains closed');
      assert.equal(upgrades, before);
    }
  } finally {
    for (const instance of instances) if (instance.timer !== undefined) clearInterval(instance.timer);
    globalThis.Response = originalResponse;
    (globalThis as any).WebSocketPair = originalPair;
  }
});
test('Miniflare preserves consumption across restart and relays two peers', async () => {
  const { Miniflare, convertV4MiniflareOptions } = await import('miniflare');
  const { build } = await import('esbuild');
  const { mkdtemp, rm } = await import('node:fs/promises');
  const { fileURLToPath } = await import('node:url');
  const directory = fileURLToPath(new URL('.', import.meta.url));
  const persistence = await mkdtemp(`${directory}.test-storage-`);
  const bundle = await build({ entryPoints: [`${directory}worker.ts`], bundle: true, write: false, format: 'esm', platform: 'browser' });
  const roomId = crypto.randomUUID(), connectionId = crypto.randomUUID();
  const options = convertV4MiniflareOptions({
    modules: true, script: bundle.outputFiles[0].text, compatibilityDate: '2026-11-06',
    durableObjects: { ROOM: { className: 'SignalingRoom', useSQLite: true } },
    name: 'signaling-test', bindings: { ROOM_ID: roomId, CONNECTION_ID: connectionId }
  });
  // Installed converter drops v4 durableObjectsPersist; use the native option.
  options.resourcePersistencePath = persistence;
  let mf = new Miniflare(options);
  const connect = () => mf.dispatchFetch(`http://localhost/v1/ws/${roomId}`, { headers: { Upgrade: 'websocket' } });
  try {
    const mobile = (await connect()).webSocket!; mobile.accept();
    const host = (await connect()).webSocket!; host.accept();
    assert.equal((await connect()).status, 409);
    const message = (type: string, payload: unknown) => JSON.stringify({ v: 1, type, connection_id: connectionId, request_id: type, payload });
    const received = new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => reject(Error('relay timeout')), 5000);
      host.addEventListener('message', event => {
        if (JSON.parse(event.data as string).type === 'offer') { clearTimeout(timer); resolve(event.data as string); }
      });
    });
    mobile.send(message('hello', { role: 'mobile' }));
    const ready = new Promise<void>(resolve => mobile.addEventListener('message', event => {
      if (JSON.parse(event.data as string).payload.status === 'ready') resolve();
    }));
    host.send(message('hello', { role: 'host' }));
    await ready;
    const offer = message('offer', { sdp: 'offer' }); mobile.send(offer);
    assert.equal(await received, offer);
    // Dispose while active: consumption cannot depend on finish/close callbacks.
    await mf.dispose();
    mf = new Miniflare(options);
    assert.equal((await connect()).status, 409);
  } finally {
    await mf.dispose();
    await rm(persistence, { recursive: true, force: true });
  }
});

function setup() {
  let now = 0;
  const id = crypto.randomUUID();
  const room = new Room(id, () => now);
  const socket = () => ({ messages: [] as any[], closed: false, send(s: string) { this.messages.push(JSON.parse(s)); }, close() { this.closed = true; } });
  const mobile = socket(), host = socket();
  room.add(mobile); room.add(host);
  const send = (peer: typeof mobile, type: string, payload: any, connection_id = id) => room.receive(peer, JSON.stringify({ v: 1, type, connection_id, request_id: crypto.randomUUID(), payload }));
  const ready = () => { send(mobile, 'hello', { role: 'mobile' }); send(host, 'hello', { role: 'host' }); };
  const negotiated = () => { ready(); send(mobile, 'offer', { sdp: 'offer' }); send(host, 'answer', { sdp: 'answer' }); };
  return { room, mobile, host, send, ready, negotiated, advance: (ms: number) => { now += ms; room.expire(); } };
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
for (const scenario of ['wrong id', 'duplicate role', 'answer first', 'wrong offerer', 'binary', 'bad json', 'oversized', 'invalid ice', 'ice ended', 'candidate limit', 'candidate bytes', 'duplicate hello', 'rate limit']) {
  test(`rejects ${scenario} and clears both peers`, () => {
    const s = setup();
    if (scenario === 'wrong id') s.send(s.mobile, 'hello', { role: 'mobile' }, crypto.randomUUID());
    if (scenario === 'duplicate role') { s.send(s.mobile, 'hello', { role: 'host' }); s.send(s.host, 'hello', { role: 'host' }); }
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
  test(`finished room rejects new peers and replay after ${reason}`, () => {
    const s = setup(); s.negotiated();
    const offer = JSON.stringify(s.host.messages.find(m => m.type === 'offer'));
    s.room.finish(reason);
    const started = s.room.started;
    s.advance(300001);
    const messages: string[] = [];
    const mobile = { send() {}, close() {} };
    const host = { send(data: string) { messages.push(data); }, close() {} };
    const accepted = [s.room.add(mobile), s.room.add(host)];
    for (const [peer, role] of [[mobile, 'mobile'], [host, 'host']] as const) {
      s.room.receive(peer, JSON.stringify({ v: 1, type: 'hello', connection_id: s.room.connectionId, request_id: role, payload: { role } }));
    }
    s.room.receive(mobile, offer);
    assert.deepEqual(messages, [], 'old offer must not reach a new peer');
    assert.deepEqual(accepted, [false, false]);
    assert.equal(s.room.started, started);
    assert.equal(s.room.peers.length, 0);
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
test('worker rejects non-v4 or uppercase room_id before the Durable Object', async () => {
  let reached = 0;
  const env = (ROOM_ID: string) => ({ ROOM_ID, CONNECTION_ID: crypto.randomUUID(), ROOM: { idFromName() {}, get() { reached++; return { fetch: () => new Response(null) }; } } }) as any;
  const ws = { headers: { Upgrade: 'websocket' } };
  for (const id of ['00000000-0000-1000-8000-000000000000', crypto.randomUUID().toUpperCase(), '00000000-0000-4000-c000-000000000000']) {
    assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}`, ws), env(id))).status, 404);
  }
  const id = crypto.randomUUID();
  assert.equal((await worker.fetch(new Request(`http://localhost/v1/ws/${id}?otk=x`, ws), env(id))).status, 404);
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

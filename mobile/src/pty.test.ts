import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { AuthError } from './auth.ts';
import { parseFrame, encodeFrame, parseControl, parseHostControl, encodeControl, parseU64, uuidStr, InputWindow, MAX_IO } from './pty.ts';

const fx = (n: string) => JSON.parse(readFileSync(new URL(`../../desktop/tests/fixtures/${n}`, import.meta.url), 'utf8'));
const res = (f: () => unknown) => { try { f(); return 'ok'; } catch (e) { if (e instanceof AuthError) return e.code; throw e; } };
const unhex = (h: string) => Uint8Array.from(h.match(/../g) ?? [], x => parseInt(x, 16));
const SID = '0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e';
const sid = unhex(SID.replaceAll('-', ''));

test('frames: mesmo resultado do host e round-trip', () => {
  for (const c of fx('c3-frames.json').cases) {
    const b = unhex(c.hex);
    assert.equal(res(() => parseFrame(b)), c.result, c.name);
    if (c.result === 'ok') assert.deepEqual(encodeFrame(parseFrame(b)), b, c.name);
  }
});

test('controles: mesmo resultado do host', () => {
  const { connection_id, cases } = fx('c3-controls.json');
  let n = 0;
  for (const c of cases) {
    const t = c.text ?? JSON.stringify({ type: 'session_create', profile_id: 'p'.repeat(65536), cols: 80, rows: 24, v: 1, connection_id });
    assert.equal(res(() => parseControl(t, connection_id)), c.result, c.name); n++;
  }
  assert.ok(n >= 30);
});

test('host→cliente: mesmo resultado da spec nos corpos reais do host', () => {
  const { connection_id, cases } = fx('c3-host-controls.json');
  for (const c of cases) assert.equal(res(() => parseHostControl(c.text, connection_id)), c.result, c.name);
  assert.ok(cases.length >= 35);
  const by = (n: string) => parseHostControl(cases.find((c: any) => c.name === n).text, connection_id);
  assert.deepEqual([by('error_fatal'), by('error_nao_fatal')].map(m => m.t === 'error' && m.fatal), [true, false]);
  const s = by('screen_snapshot');
  assert.ok(s.t === 'screen_snapshot' && new TextDecoder().decode(s.data).includes('hi'));
  const cwd = by('evento_1'), cc = by('evento_0');
  assert.ok(cwd.t === 'cwd_changed' && cwd.cwd === 'C:\\Users\\a b');
  assert.ok(cc.t === 'command_completed' && cc.exitCode === 2);
  const a = by('replay_end');
  assert.ok(a.t === 'replay_end' && uuidStr(a.sessionId).length === 36 && typeof a.offset === 'bigint');
});

test('valores', () => {
  assert.equal(parseU64('18446744073709551615'), 2n ** 64n - 1n);
  assert.equal(uuidStr(sid), SID);
  const f = parseFrame(encodeFrame({ sessionId: sid, seq: 42n, op: { t: 'exited', code: -1 } }));
  assert.deepEqual(f.op, { t: 'exited', code: -1 });
  const CID = '9b2f6c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d', RID = '1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d';
  assert.deepEqual(JSON.parse(encodeControl(CID, RID, { type: 'session_list' })), { type: 'session_list', v: 1, connection_id: CID, request_id: RID });
  const a = parseControl(encodeControl(CID, RID, { type: 'session_attach', session_id: SID, from_offset: '5', cols: 80, rows: 24 }), CID);
  assert.ok(a.kind === 'request' && a.req.t === 'attach' && a.req.fromOffset === 5n && a.req.size?.[0] === 80);
});

test('janela: 256 frames / 64 KiB, ack cumulativo', () => {
  const w = new InputWindow(sid);
  for (let i = 0; i < 300; i++) w.type(Uint8Array.of(65));
  const f = w.take();
  assert.equal(f.length, 256);
  assert.deepEqual([f[0].seq, f[255].seq], [1n, 256n]);
  assert.equal(w.take().length, 0);
  w.ack(10n);
  assert.deepEqual(w.take().map(x => x.seq), Array.from({ length: 10 }, (_, i) => 257n + BigInt(i)));
  const b = new InputWindow(sid);
  b.type(new Uint8Array(MAX_IO * 5));
  assert.equal(b.take().length, 4);
});

test('reconexão: reenvia só seq > last_input_seq; offline só com confirmação', () => {
  const w = new InputWindow(sid);
  w.type(Uint8Array.of(1)); w.type(Uint8Array.of(2)); w.type(Uint8Array.of(3));
  assert.equal(w.take().length, 3);
  w.disconnect();
  w.type(Uint8Array.of(9));
  assert.equal(w.take().length, 0);
  assert.deepEqual(w.attached(1n).map(x => x.seq), [2n, 3n]);
  assert.equal(w.offline.length, 1);
  assert.equal(w.take().length, 0);
  w.confirmOffline();
  assert.deepEqual(w.take().map(x => [x.seq, (x.op as any).data[0]]), [[4n, 9]]);
  w.lost();
  assert.equal(w.pending, 0);
  assert.equal(w.take().length, 0);
});

test('reconexão: host à frente não descarta o nunca enviado', () => {
  const w = new InputWindow(sid);
  w.disconnect();
  w.type(Uint8Array.of(9));
  w.confirmOffline();
  assert.deepEqual(w.attached(5n).map(x => [x.seq, (x.op as any).data[0]]), [[6n, 9]]);
  w.type(Uint8Array.of(8));
  assert.deepEqual(w.take().map(x => x.seq), [7n]);
});

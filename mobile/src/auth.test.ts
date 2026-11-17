import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { ed25519 } from '@noble/curves/ed25519.js';
import { bytesToHex, hexToBytes } from '@noble/hashes/utils.js';
import { AuthError, Offerer, b64e, mac, parseMsg, parseQr, sign, toJson, transcript, uuidBytes, verify, verifyMac, type Binding, type Code, type Msg } from './auth.ts';

const CID = '00010203-0405-0607-0809-0a0b0c0d0e0f';
const fill = (b: number, n = 32) => new Uint8Array(n).fill(b);
const binding = (): Binding => ({ connectionId: uuidBytes(CID)!, roomId: Uint8Array.from({ length: 16 }, (_, i) => 0x10 + i), fpOfferer: fill(0x40), fpAnswerer: fill(0x41) });
const vecT = () => transcript('pair', binding(), fill(0x20), fill(0x21), fill(0x30), fill(0x31));
const code = (c: Code) => (e: unknown) => e instanceof AuthError && e.code === c;
const wire = (m: Msg) => parseMsg(toJson(m, CID), CID);
const fixture = JSON.parse(readFileSync(new URL('../../desktop/tests/fixtures/c2-vector-3.1.json', import.meta.url), 'utf8'));

test('A3: T, sig_a, sig_o, mac_a, mac_o idênticos à fixture Rust', () => {
  const t = vecT(), otk = fill(0x50);
  assert.equal(t.length, 312);
  assert.equal(bytesToHex(t), fixture.T);
  assert.equal(bytesToHex(ed25519.getPublicKey(fill(0x60))), fixture.host_vk);
  assert.equal(bytesToHex(ed25519.getPublicKey(fill(0x61))), fixture.client_vk);
  assert.equal(bytesToHex(sign(fill(0x60), 'answerer', t)), fixture.sig_a);
  assert.equal(bytesToHex(sign(fill(0x61), 'offerer', t)), fixture.sig_o);
  assert.equal(bytesToHex(mac(otk, 'answerer', t)), fixture.mac_a);
  assert.equal(bytesToHex(mac(otk, 'offerer', t)), fixture.mac_o);
  verify(hexToBytes(fixture.host_vk), 'answerer', t, hexToBytes(fixture.sig_a));
  verifyMac(otk, 'offerer', t, hexToBytes(fixture.mac_o));
});

test('§3.1 negativos e N13: troca de fps, rótulo do outro papel, reconnect', () => {
  const t = vecT(), b = binding();
  [b.fpOfferer, b.fpAnswerer] = [b.fpAnswerer, b.fpOfferer];
  const swapped = transcript('pair', b, fill(0x20), fill(0x21), fill(0x30), fill(0x31));
  const reconn = transcript('reconnect', binding(), fill(0x20), fill(0x21), fill(0x30), fill(0x31));
  const pk = hexToBytes(fixture.host_vk), sig = hexToBytes(fixture.sig_a);
  assert.throws(() => verify(pk, 'answerer', swapped, sig), code('UNAUTHORIZED'));
  assert.throws(() => verify(pk, 'offerer', t, sig), code('UNAUTHORIZED'));
  assert.throws(() => verify(pk, 'answerer', reconn, sig), code('UNAUTHORIZED'));
  assert.throws(() => verifyMac(fill(0x50), 'offerer', t, hexToBytes(fixture.mac_a)), code('UNAUTHORIZED'));
  assert.throws(() => verify(fill(0), 'answerer', t, sig), code('UNAUTHORIZED'), 'chave de ordem pequena');
});

/** Host simulado (seed 60×32) respondendo a um hello. */
function challenge(hello: Msg, opts: { otk?: Uint8Array; macA?: boolean; hostSeed?: Uint8Array; role?: 'answerer' | 'offerer' } = {}): Msg {
  assert.equal(hello.type, 'auth_hello');
  if (hello.type !== 'auth_hello') throw 0;
  const seed = opts.hostSeed ?? fill(0x60), hostPk = ed25519.getPublicKey(seed), nonceA = fill(0x77);
  const t = transcript(hello.mode, binding(), hostPk, hello.clientPk, hello.nonceO, nonceA);
  const withMac = opts.macA ?? !!opts.otk;
  return { type: 'auth_challenge', hostPk, nonceA, sigA: sign(seed, opts.role ?? 'answerer', t), macA: withMac ? mac(opts.otk ?? fill(1), 'answerer', t) : undefined };
}
const offerer = (otk?: Uint8Array, pin = ed25519.getPublicKey(fill(0x60))) =>
  new Offerer({ otk, seed: fill(0x61), pin, binding: binding(), connectedAt: 0, random: n => fill(0x30, n) });

test('pair feliz pelo JSON: prova verifica no host, auth_ok autentica', () => {
  const otk = fill(0x50), o = offerer(otk), ch = wire(challenge(wire(o.hello), { otk }));
  const proof = wire(o.handle(ch, 1)!);
  assert.equal(proof.type, 'auth_proof');
  if (proof.type !== 'auth_proof' || ch.type !== 'auth_challenge') throw 0;
  const t = transcript('pair', binding(), ch.hostPk, ed25519.getPublicKey(fill(0x61)), fill(0x30), ch.nonceA);
  verify(ed25519.getPublicKey(fill(0x61)), 'offerer', t, proof.sigO);
  verifyMac(otk, 'offerer', t, proof.macO!);
  assert.equal(o.handle(wire({ type: 'auth_ok' }), 2), null);
  assert.equal(o.state, 'AUTHENTICATED');
});

test('reconnect: sem MAC; MAC presente é recusado', () => {
  const o = offerer();
  const proof = o.handle(challenge(o.hello), 1)!;
  assert.ok(proof.type === 'auth_proof' && proof.macO === undefined);
  const o2 = offerer();
  assert.throws(() => o2.handle(challenge(o2.hello, { macA: true }), 1), code('INVALID_MESSAGE'));
  const o3 = offerer(fill(0x50));
  assert.throws(() => o3.handle(challenge(o3.hello, { otk: fill(0x50), macA: false }), 1), code('INVALID_MESSAGE'));
});

test('N8: host_pk ≠ pin fecha com UNAUTHORIZED', () => {
  const o = offerer(fill(0x50), ed25519.getPublicKey(fill(9)));
  assert.throws(() => o.handle(challenge(o.hello, { otk: fill(0x50) }), 1), code('UNAUTHORIZED'));
  assert.equal(o.state, 'CLOSED');
});

test('N13: host assinando com rótulo offerer; MAC com OTK errada; timeout; fora de ordem', () => {
  const o = offerer(fill(0x50));
  assert.throws(() => o.handle(challenge(o.hello, { otk: fill(0x50), role: 'offerer' }), 1), code('UNAUTHORIZED'));
  const o2 = offerer(fill(0x50));
  assert.throws(() => o2.handle(challenge(o2.hello, { otk: fill(0x51) }), 1), code('UNAUTHORIZED'));
  const o3 = offerer(fill(0x50));
  assert.throws(() => o3.handle(challenge(o3.hello, { otk: fill(0x50) }), 30_000), code('EXPIRED'));
  const o4 = offerer(fill(0x50));
  assert.throws(() => o4.handle({ type: 'auth_ok' }, 1), code('INVALID_MESSAGE'));
});

test('N7/N7b: mensagens inválidas e versão', () => {
  const ok = toJson({ type: 'auth_hello', mode: 'pair', clientPk: fill(1), nonceO: fill(2) }, CID);
  assert.equal(parseMsg(ok, CID).type, 'auth_hello');
  const edit = (f: (v: any) => void) => { const v = JSON.parse(ok); f(v); return () => parseMsg(JSON.stringify(v), CID); };
  assert.throws(edit(v => { v.role = 'answerer'; }), code('INVALID_MESSAGE'));
  assert.throws(edit(v => { v.mode = 'other'; }), code('INVALID_MESSAGE'));
  assert.throws(edit(v => { v.extra = 1; }), code('INVALID_MESSAGE'));
  assert.throws(edit(v => { v.nonce_o = 'AAAA'; }), code('INVALID_MESSAGE'));
  assert.throws(edit(v => { v.nonce_o = b64e(fill(2)) + '='; }), code('INVALID_MESSAGE'));
  assert.throws(edit(v => { v.v = 2; }), code('VERSION_UNSUPPORTED'));
  assert.throws(edit(v => { v.v = '1'; }), code('INVALID_MESSAGE'));
  assert.throws(() => parseMsg(ok, 'ffffffff-ffff-4fff-bfff-ffffffffffff'), code('INVALID_MESSAGE'));
  assert.throws(() => parseMsg(' '.repeat(65537), CID), code('INVALID_MESSAGE'));
  assert.throws(() => parseMsg('[]', CID), code('INVALID_MESSAGE'));
  assert.deepEqual(wire({ type: 'error', code: 'REVOKED' }), { type: 'error', code: 'REVOKED' });
  assert.throws(() => parseMsg(toJson({ type: 'error', code: 'NOPE' as Code }, CID), CID), code('INVALID_MESSAGE'));
});

test('N16 e QR: parse, allowlist, duplicado, desconhecido, > 512 bytes', () => {
  const otk = b64e(fill(5)), pk = b64e(fill(6)), allow = ['sig.example.com:443'];
  const good = `remoteterm://pair?v=1&session=0f0e0d0c-0b0a-4908-8706-050403020100&otk=${otk}&host_pk=${pk}&server=sig.example.com:443`;
  const p = parseQr(good, allow);
  assert.deepEqual([p.otk, p.hostPk, p.roomId[0], p.server], [fill(5), fill(6), 0x0f, 'sig.example.com:443']);
  assert.throws(() => parseQr(good, ['other.example.com']), code('SERVER_NOT_ALLOWED'));
  assert.throws(() => parseQr(good.replace('v=1', 'v=2'), allow), code('VERSION_UNSUPPORTED'));
  for (const bad of [
    good.replace('remoteterm://pair?', 'remoteterm://other?'),
    `${good}&x=1`, `${good}&v=1`, good.replace('&server=sig.example.com:443', ''),
    good.replace('0f0e0d0c', '0F0E0D0C'), good.replace('-4908-', '-1908-'),
    good.replace(otk, `${otk}=`), good.replace(otk, otk.slice(0, 42)), good.replace(pk, pk.replaceAll('G', '+')),
    good.replace(':443', ':0'), good.replace(':443', ':70000'), good.replace('sig.', 'SIG.'),
    `${good}&pad=${'a'.repeat(600)}`,
  ]) assert.throws(() => parseQr(bad, allow), code('INVALID_MESSAGE'), bad.slice(0, 80));
  assert.ok(good.length <= 512);
});

test('auth_ok: formato do host 0.1.8, estrito (capacidades vão por pedido próprio, ver extras.ts capsProbe)', () => {
  const fx = JSON.parse(readFileSync(new URL('../../desktop/tests/fixtures/c3-extras.json', import.meta.url), 'utf8'));
  const old = fx.caps_probe.auth_ok_host_antigo;
  assert.deepEqual(parseMsg(old, fx.connection_id), { type: 'auth_ok' });
  assert.throws(() => parseMsg(JSON.stringify({ ...JSON.parse(old), caps: ['ssh'] }), fx.connection_id), code('INVALID_MESSAGE'));
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { KEY_SEQ, ctrl, lineToSend } from './keys.ts';
import { groupByDay, summarizeSessions, type HistoryEntry } from './extras.ts';

test('teclas: sequências xterm', () => {
  assert.equal(KEY_SEQ.esc, '\x1b');
  assert.equal(KEY_SEQ.tab, '\t');
  assert.equal(KEY_SEQ.up, '\x1b[A');
  assert.equal(KEY_SEQ.down, '\x1b[B');
});

test('Ctrl: letras viram bytes de controle', () => {
  assert.equal(ctrl('c'), '\x03');
  assert.equal(ctrl('D'), '\x04');
  assert.equal(ctrl('['), '\x1b');
  assert.equal(ctrl('1'), null);
  assert.equal(lineToSend('c', true), '\x03');
  assert.equal(lineToSend('ls', false), 'ls\r');
  assert.equal(lineToSend('', true), null);
});

const e = (d: Date, label: string, kind = 'pwsh'): HistoryEntry =>
  ({ entryId: String(+d) + label, command: 'git push', cwd: null, atMs: BigInt(+d), exitCode: 0, kind, label, pc: 'PC' });

test('dias antigos no formato do mockup: "15 NOV"', () => {
  const now = new Date(2026, 10, 17, 12);
  const g = groupByDay([e(new Date(2026, 10, 15, 9), 'a'), e(new Date(2025, 0, 3), 'a')], now);
  assert.deepEqual(g.map(x => x.day), ['15 NOV', '03 JAN 2025']);
});

test('resumo de sessões: conta por sessão, a mais recente primeiro', () => {
  const s = summarizeSessions([e(new Date(2026, 10, 16), 'prod-01', 'ssh'), e(new Date(2026, 10, 17), 'pwsh'), e(new Date(2026, 10, 15), 'prod-01', 'ssh')]);
  assert.deepEqual(s.map(x => [x.label, x.count]), [['pwsh', 1], ['prod-01', 2]]);
});

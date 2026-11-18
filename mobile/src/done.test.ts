import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LONG_MS, longRun } from './done.ts';

test('vibra só acima de 10 s', () => {
  assert.equal(longRun(0, LONG_MS), false);
  assert.equal(longRun(0, LONG_MS + 1), true);
  assert.equal(longRun(5_000, 9_000), false);
  assert.equal(longRun(undefined, 99_999), false);
});

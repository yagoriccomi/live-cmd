'use strict';

const assert = require('node:assert/strict');

if (process.env.HIDDEN_RUN_FIXTURE === '1') {
  const test = require('node:test');
  test('controle simples', () => process.stdout.write('BODY:controle\n'));
  test('Miniflare ficticio', () => {
    process.stdout.write('BODY:Miniflare\n');
    assert.fail('Miniflare fixture must not execute');
  });
} else {
  const { spawnSync } = require('node:child_process');
  const path = require('node:path');
  const preload = path.join(__dirname, 'hidden-processes.cjs');
  const launcher = process.argv[2] || path.join(__dirname, 'hidden-run.cjs');
  const env = { ...process.env };
  const key = Object.keys(env).find(name => name.toUpperCase() === 'NODE_OPTIONS') || 'NODE_OPTIONS';
  const existing = `${env[key] || ''} --trace-warnings`.trim();
  const inherited = `${existing} --require=${JSON.stringify(preload.replace(/\\/g, '/'))}`;
  const expected = { stdout: 'ação: saída\nsegunda linha\n', stderr: 'é falha: conexão\ndetalhes\n' };
  const probe = `
    require('node:assert/strict').ok(process.env.NODE_OPTIONS.includes(${JSON.stringify(existing)}));
    for (const [name, text] of Object.entries(${JSON.stringify(expected)})) {
      const bytes = Buffer.from(text, 'utf8');
      process[name].write(bytes.subarray(0, 2));
      process[name].write(bytes.subarray(2));
    }
    process.exitCode = 23;
  `;
  for (const mode of ['explicit', 'inherited', 'both']) {
    const flags = mode === 'inherited' ? [] : ['--require', preload];
    const options = mode === 'explicit' ? existing : inherited;
    function execute(args, extraEnv = {}) {
      const result = spawnSync(process.execPath, [...flags, launcher, ...args], {
        env: { ...env, [key]: options, ...extraEnv },
        cwd: __dirname, shell: false, windowsHide: true,
        stdio: ['ignore', 'pipe', 'pipe'], timeout: 10000,
      });
      assert.ifError(result.error);
      assert.equal(result.signal, null, mode);
      return result;
    }
    const bytes = execute([process.execPath, '-e', probe]);
    assert.equal(bytes.status, 23, `${mode}: child exit code`);
    for (const name of ['stdout', 'stderr']) {
      assert.deepEqual(bytes[name], Buffer.from(expected[name], 'utf8'), `${mode}: ${name}`);
    }
    const tests = execute([
      process.execPath, '--require', preload, '--test', '--test-reporter=tap',
      '--test-timeout=5000', '--test-skip-pattern=^Miniflare', __filename,
    ], { HIDDEN_RUN_FIXTURE: '1' });
    assert.equal(tests.status, 0, tests.stderr.toString());
    const report = tests.stdout.toString('utf8');
    assert.match(report, /BODY:controle/);
    assert.doesNotMatch(report, /BODY:Miniflare/);
    assert.match(report, /# pass 1\b/);
    assert.match(report, /# fail 0\b/);
    const self = execute(['--self-test']);
    assert.equal(self.status, 0, self.stderr.toString());
    assert.match(self.stdout.toString(), /hidden-processes self-test passed/);
    assert.equal(self.stderr.length, 0);
  }
  console.log('hidden-run regression passed (explicit/inherited preload, UTF-8, exit 23, skip, self-test)');
}

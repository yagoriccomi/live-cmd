'use strict';

const childProcess = require('node:child_process');
const { constants } = require('node:os');

function nodeOptions(value = '', filename = __filename) {
  const file = filename.replace(/\\/g, '/');
  const tokens = (value.match(/(?:[^\s"\\]|\\.|"(?:\\.|[^"\\])*")+/g) || [])
    .map(token => token.replace(/"((?:\\.|[^"\\])*)"/g,
      (_, text) => text.replace(/\\(.)/g, '$1')));
  const sameFile = candidate => candidate?.replace(/\\/g, '/').toLowerCase() === file.toLowerCase();
  if (tokens.some((token, i) =>
    ((token === '--require' || token === '-r') && sameFile(tokens[i + 1]))
    || (token.startsWith('--require=') && sameFile(token.slice(10))))) return value;
  return `${value}${value ? ' ' : ''}--require=${JSON.stringify(file)}`;
}

function install(cp, env, filename, platform) {
  if (platform !== 'win32') return;
  function environment(source) {
    const result = { ...source };
    const key = Object.keys(result).find(name => name.toUpperCase() === 'NODE_OPTIONS') || 'NODE_OPTIONS';
    result[key] = nodeOptions(result[key], filename);
    return result;
  }
  Object.assign(env, environment(env));
  // All asynchronous APIs reach this method, including Node's lexical spawn calls.
  const originalSpawn = cp.ChildProcess.prototype.spawn;
  cp.ChildProcess.prototype.spawn = function (options) {
    if (options === null || typeof options !== 'object') return originalSpawn.call(this, options);
    if (options.envPairs != null && !Array.isArray(options.envPairs)) return originalSpawn.call(this, options);
    const pairs = [...(options.envPairs || [])];
    const index = pairs.findIndex(pair => /^NODE_OPTIONS=/i.test(pair));
    const value = index < 0 ? '' : pairs[index].slice(pairs[index].indexOf('=') + 1);
    const pair = `NODE_OPTIONS=${nodeOptions(value, filename)}`;
    if (index < 0) pairs.push(pair);
    else pairs[index] = pair;
    return originalSpawn.call(this, { ...options, windowsHide: true, envPairs: pairs });
  };
  // exec*Sync use internal bindings, not the exported spawnSync function.
  for (const name of ['spawnSync', 'execFileSync', 'execSync']) {
    const original = cp[name];
    cp[name] = function (...args) {
      const index = name !== 'execSync'
        && (Array.isArray(args[1]) || (args[1] == null && args.length > 2)) ? 2 : 1;
      const options = args[index];
      // Leave invalid options to Node's own validation/error handling.
      if (options != null && (typeof options !== 'object' || Array.isArray(options))) return original.apply(this, args);
      if (options?.env != null && typeof options.env !== 'object') return original.apply(this, args);
      args[index] = { ...options, windowsHide: true, env: environment(options?.env ?? env) };
      return original.apply(this, args);
    };
  }
}

function run(args, cp = childProcess, runtime = process) {
  const fail = error => {
    runtime.stderr.write(`${error.message}\n`);
    runtime.exitCode = error.code === 'ENOENT' ? 127 : error.code === 'EACCES' ? 126 : 1;
  };
  if (!args.length) {
    runtime.stderr.write('Usage: node hidden-run.cjs <executable> [args...]\n');
    runtime.exitCode = 2;
    return;
  }
  let child;
  try {
    child = cp.spawn(args[0], args.slice(1), {
      shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
    });
  } catch (error) { fail(error); return; }
  child.stdout.pipe(runtime.stdout);
  child.stderr.pipe(runtime.stderr);
  const interrupt = () => child.kill('SIGINT');
  const terminate = () => child.kill('SIGTERM');
  runtime.on('SIGINT', interrupt);
  runtime.on('SIGTERM', terminate);
  let failed = false;
  child.once('error', error => { failed = true; fail(error); });
  child.once('close', (code, signal) => {
    runtime.removeListener('SIGINT', interrupt);
    runtime.removeListener('SIGTERM', terminate);
    if (!failed) runtime.exitCode = code ?? (signal ? 128 + (constants.signals[signal] || 1) : 1);
  });
}

async function selfTest() {
  const assert = require('node:assert/strict');
  const { EventEmitter } = require('node:events');
  const { PassThrough, Writable } = require('node:stream');
  const { finished } = require('node:stream/promises');
  const file = 'C:/test folder/hidden-processes.cjs';
  const nodeArgs = Object.freeze([
    '--require', file, '--experimental-strip-types', '--test',
    '--test-skip-pattern=^Miniflare', 'C:/test folder/signaling.test.ts',
  ]);
  const existing = '--trace-warnings --require="C:/other preload.cjs"';
  const value = nodeOptions(existing, file);
  assert.ok(value.startsWith(existing + ' '));
  assert.ok(value.endsWith('--require="C:/test folder/hidden-processes.cjs"'));
  assert.equal(nodeOptions(value, file), value);
  assert.equal(nodeOptions(`-r "${file}"`, file), `-r "${file}"`);
  assert.equal(nodeOptions(`--require "${file}"`, file), `--require "${file}"`);
  const result = {}, receiver = {}, failure = new Error('original failure');
  let calls = [], throws = false;
  function original(...args) {
    calls.push({ receiver: this, args });
    if (throws) throw failure;
    return result;
  }
  const fake = {
    ChildProcess: function () {}, spawnSync: original, execFileSync: original, execSync: original,
  };
  fake.ChildProcess.prototype.spawn = original;
  const env = { NODE_OPTIONS: existing, KEEP: 'yes' };
  install(fake, env, file, 'linux');
  assert.equal(fake.spawnSync, original);
  assert.equal(fake.ChildProcess.prototype.spawn, original);
  assert.equal(env.NODE_OPTIONS, existing);
  install(fake, env, file, 'win32');
  assert.equal(env.NODE_OPTIONS, value);
  const options = Object.freeze({
    file: 'node', args: Object.freeze(['node', ...nodeArgs]), windowsHide: false,
    stdio: Object.freeze([
      Object.freeze({ type: 'ignore' }),
      Object.freeze({ type: 'pipe', readable: false, writable: true }),
      Object.freeze({ type: 'fd', fd: 2 }),
    ]),
    envPairs: Object.freeze(['KEEP=yes', `NODE_OPTIONS=${existing}`]),
  });
  assert.equal(fake.ChildProcess.prototype.spawn.call(receiver, options), result);
  assert.equal(calls.at(-1).receiver, receiver);
  assert.deepEqual(calls.at(-1).args, [{
    ...options, windowsHide: true, envPairs: ['KEEP=yes', `NODE_OPTIONS=${value}`],
  }]);
  assert.equal(calls.at(-1).args[0].args, options.args);
  assert.equal(calls.at(-1).args[0].stdio, options.stdio);
  assert.equal(options.windowsHide, false);
  assert.equal(options.envPairs[1], `NODE_OPTIONS=${existing}`);
  fake.ChildProcess.prototype.spawn.call(receiver, { envPairs: [] });
  assert.deepEqual(calls.at(-1).args[0].envPairs, [`NODE_OPTIONS=${nodeOptions('', file)}`]);
  for (const name of ['spawnSync', 'execFileSync', 'execSync']) {
    for (const withArgs of name === 'execSync' ? [false] : [false, true]) {
      const opts = { windowsHide: false, stdio: 'pipe', env: { node_options: existing, KEEP: 'yes' } };
      const args = ['program', ...(withArgs ? [['space argument']] : []), opts];
      assert.equal(fake[name].apply(receiver, args), result);
      const call = calls.at(-1);
      assert.equal(call.receiver, receiver);
      assert.deepEqual(call.args.slice(0, -1), args.slice(0, -1));
      assert.deepEqual(call.args.at(-1), { ...opts, windowsHide: true, env: { node_options: value, KEEP: 'yes' } });
      assert.equal(opts.env.node_options, existing);
    }
    if (name !== 'execSync') {
      fake[name]('program', undefined, { windowsHide: false, env: {} });
      assert.equal(calls.at(-1).args[2].windowsHide, true);
      assert.equal(calls.at(-1).args[2].env.NODE_OPTIONS, nodeOptions('', file));
    }
    fake[name]('program', 'invalid options');
    assert.deepEqual(calls.at(-1).args, ['program', 'invalid options']);
    fake[name]('program');
    assert.equal(calls.at(-1).args[1].env.NODE_OPTIONS, value);
    throws = true;
    assert.throws(() => fake[name]('program'), error => error === failure);
    throws = false;
  }
  throws = true;
  assert.throws(() => fake.ChildProcess.prototype.spawn.call(receiver, options), error => error === failure);
  throws = false;
  function cli(args, outcome, synchronous = false) {
    const runtime = new EventEmitter();
    const messages = [];
    runtime.stdout = {};
    runtime.stderr = { write: text => messages.push(text) };
    const child = new EventEmitter();
    child.stdout = { pipe: target => assert.equal(target, runtime.stdout) };
    child.stderr = { pipe: target => assert.equal(target, runtime.stderr) };
    child.kill = signal => { child.signal = signal; };
    run(args, { spawn(command, argv, opts) {
      assert.equal(command, 'program');
      assert.deepEqual(argv, ['space argument', ';not-shell']);
      assert.deepEqual(opts, { shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
      if (synchronous) throw outcome;
      return child;
    } }, runtime);
    if (args.length && !synchronous) {
      runtime.emit('SIGINT');
      assert.equal(child.signal, 'SIGINT');
      runtime.emit('SIGTERM');
      assert.equal(child.signal, 'SIGTERM');
      if (outcome instanceof Error) child.emit('error', outcome);
      child.emit('close', ...(outcome instanceof Error ? [-2, null] : outcome));
      assert.equal(runtime.listenerCount('SIGINT'), 0);
      assert.equal(runtime.listenerCount('SIGTERM'), 0);
    }
    return { code: runtime.exitCode, messages };
  }
  const args = ['program', 'space argument', ';not-shell'];
  assert.equal(cli([], null).code, 2);
  assert.equal(cli(args, [0, null]).code, 0);
  assert.equal(cli(args, [23, null]).code, 23);
  assert.equal(cli(args, [null, 'SIGTERM']).code, 128 + constants.signals.SIGTERM);
  const missing = Object.assign(new Error('missing executable'), { code: 'ENOENT' });
  assert.deepEqual(cli(args, missing), { code: 127, messages: ['missing executable\n'] });
  assert.equal(cli(args, missing, true).code, 127);
  assert.equal(cli(args, new Error('failure'), true).code, 1);
  assert.equal(cli(args, Object.assign(new Error('denied'), { code: 'EACCES' })).code, 126);

  const runtime = new EventEmitter(), child = new EventEmitter();
  const output = { stdout: [], stderr: [] };
  const expected = {
    stdout: Buffer.from('ação: saída\nsegunda linha\n', 'utf8'),
    stderr: Buffer.from('é falha: conexão\ndetalhes\n', 'utf8'),
  };
  for (const name of ['stdout', 'stderr']) {
    child[name] = new PassThrough();
    runtime[name] = new Writable({
      write(chunk, encoding, callback) {
        output[name].push(Buffer.from(chunk));
        callback();
      },
    });
  }
  let spawnCalls = 0;
  run(['node', ...nodeArgs], { spawn(command, argv, opts) {
    spawnCalls++;
    assert.equal(command, 'node');
    assert.deepEqual(argv, [...nodeArgs]);
    assert.deepEqual(opts, { shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    return child;
  } }, runtime);
  assert.equal(spawnCalls, 1);
  assert.equal(runtime.listenerCount('SIGINT'), 1);
  assert.equal(runtime.listenerCount('SIGTERM'), 1);
  const drained = Promise.all(['stdout', 'stderr'].map(name => finished(runtime[name], { cleanup: true })));
  // Split inside UTF-8 characters, interleaving both channels to catch decoding/mixing.
  for (const [start, end] of [[0, 1], [1, 2], [2, undefined]]) {
    for (const name of ['stdout', 'stderr']) child[name].write(expected[name].subarray(start, end));
  }
  child.stdout.end();
  child.stderr.end();
  await drained;
  child.emit('close', 1, null);
  assert.equal(runtime.exitCode, 1);
  assert.equal(runtime.listenerCount('SIGINT'), 0);
  assert.equal(runtime.listenerCount('SIGTERM'), 0);
  assert.equal(child.listenerCount('close'), 0);
  for (const name of ['stdout', 'stderr']) {
    assert.ok(output[name].length > 1);
    assert.deepEqual(Buffer.concat(output[name]), expected[name]);
    assert.equal(child[name].listenerCount('data'), 0);
  }
  console.log('hidden-processes self-test passed (mocked; no subprocesses)');
}

// ponytail: test-tree preload only; native programs allocating their own console need native fixes.
install(childProcess, process.env, __filename, process.platform);
if (process.platform === 'win32') require('node:module').syncBuiltinESMExports();
module.exports = { run, selfTest };

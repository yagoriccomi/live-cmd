'use strict';

require('./hidden-processes.cjs');

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { randomUUID } = require('node:crypto');
const net = require('node:net');
const path = require('node:path');
const { StringDecoder } = require('node:string_decoder');
const { stripVTControlCharacters } = require('node:util');

const TESTS = [
  'wrangler_echo', 'wrangler_retransmits_zero', 'wrangler_retransmits_one',
  'wrangler_lifetime_zero', 'wrangler_lifetime_one', 'wrangler_unordered',
  'wrangler_wrong_label', 'wrangler_second_channel',
];
const DESKTOP = path.resolve(__dirname, '..');
const SIGNALING = path.resolve(DESKTOP, '../signaling');
const STATE = path.join(SIGNALING, '.wrangler');
const PORT = 18849;
const INSPECTOR_PORT = 19349;
const CAPTURE_LIMIT = 64 * 1024;

function argumentsFor(args) {
  if (args.length === 1 && args[0] === '--self-test') return null;
  for (const arg of args) {
    if (!TESTS.includes(arg)) throw new Error(`Invalid argument: ${arg}`);
  }
  return args.length ? [...new Set(args)] : [...TESTS];
}

// Separate decoders/tails per stream: neither UTF-8 nor URL fragments cross streams.
function capture(onOutput) {
  const decoder = new StringDecoder('utf8');
  let text = '';
  function append(value) {
    // Scan before trimming so markers survive large chunks and history boundaries.
    for (let i = 0; i < value.length; i += 4096) {
      text += value.slice(i, i + 4096);
      onOutput?.(stripVTControlCharacters(text));
      text = text.slice(-CAPTURE_LIMIT);
    }
  }
  return {
    push(chunk) { append(decoder.write(chunk)); },
    end() { append(decoder.end()); },
    get text() { return stripVTControlCharacters(text); },
  };
}

function isReady(text) {
  return /Ready on http:\/\/127\.0\.0\.1:18849\s/.test(text);
}

function cargoPassed(name, result, text) {
  return !result.error && result.code === 0 && result.signal === null
    && new RegExp(`^test tests::${name} \\.\\.\\. ok\\r?$`, 'm').test(text)
    && /^test result: ok\. 1 passed; 0 failed; 0 ignored;[^\r\n]*$/m.test(text);
}

async function waitFor(promise, ms, signal, label) {
  let timer, abort;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      abort = () => reject(signal.reason);
      timer = setTimeout(() => reject(new Error(`${label} timeout (${ms}ms)`)), ms);
      signal?.addEventListener('abort', abort, { once: true });
      if (signal?.aborted) abort();
    })]);
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
  }
}

async function checkPort(port) {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once('error', error => reject(new Error(`Port ${port} unavailable: ${error.message}`)));
    server.listen({ host: '127.0.0.1', port, exclusive: true }, resolve);
  });
  await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
}

function start(command, args, env, onOutput, onFailure) {
  const child = spawn(command, args, {
    cwd: SIGNALING, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  });
  const record = { child, stdout: capture(onOutput), stderr: capture(onOutput), exited: false, closed: null };
  let error;
  record.closed = new Promise(resolve => {
    child.on('error', cause => { error = cause; onFailure?.(cause); });
    child.once('exit', (code, signal) => {
      record.exited = true;
      onFailure?.(new Error(`Process ${child.pid} exited: code=${code} signal=${signal}`));
    });
    child.once('close', (code, signal) => resolve({ code, signal, error }));
  });
  for (const key of ['stdout', 'stderr']) {
    child[key].on('data', chunk => record[key].push(chunk));
    child[key].on('end', () => record[key].end());
    child[key].on('error', cause => { error = cause; onFailure?.(cause); });
  }
  return record;
}

async function stop(record) {
  if (!record) return;
  const child = record.child;
  if (child.pid && !record.exited && child.exitCode === null && child.signalCode === null) {
    // Harness cleanup only, NOT evidence of application Ctrl+C handling.
    const killer = start(path.join(process.env.SystemRoot || 'C:/Windows', 'System32/taskkill.exe'),
      ['/PID', String(child.pid), '/T', '/F'], process.env);
    let result;
    try {
      result = await waitFor(killer.closed, 15000, null, 'taskkill');
    } catch (error) {
      if (!killer.exited) killer.child.kill();
      await waitFor(killer.closed, 5000, null, 'taskkill cleanup');
      throw error;
    }
    if (result.error || result.code !== 0) {
      throw new Error(`taskkill PID ${child.pid} failed: ${result.error || result.code}\n${killer.stdout.text}${killer.stderr.text}`);
    }
  }
  await waitFor(record.closed, 10000, null, `Process ${child.pid} cleanup`);
}

async function runCase(name, interruption) {
  interruption.throwIfAborted();
  await checkPort(PORT);
  await checkPort(INSPECTOR_PORT);
  interruption.throwIfAborted();
  const room = randomUUID(), connection = randomUUID();
  const env = {
    ...process.env, ROOM_ID: room, CONNECTION_ID: connection, PORT: String(PORT),
    CI: 'true', NO_COLOR: '1', CARGO_TERM_COLOR: 'never', WRANGLER_SEND_METRICS: 'false',
    WRANGLER_LOG_PATH: path.join(STATE, 'd1-logs'),
    WRANGLER_CACHE_DIR: path.join(STATE, 'd1-cache'),
    WRANGLER_REGISTRY_PATH: path.join(STATE, 'd1-registry'),
  };
  console.log(`CASE ${name} ROOM_ID=${room} CONNECTION_ID=${connection}`);
  const failed = new AbortController();
  const signal = AbortSignal.any([interruption, failed.signal]);
  let worker, cargo, closing = false, failure;
  let ready;
  const readiness = new Promise(resolve => { ready = resolve; });
  try {
    worker = start(process.execPath, [
      path.join(SIGNALING, 'node_modules/wrangler/bin/wrangler.js'), 'dev',
      '--config', path.join(SIGNALING, 'wrangler.jsonc'), '--local',
      '--ip', '127.0.0.1', '--port', String(PORT),
      '--inspector-ip', '127.0.0.1', '--inspector-port', String(INSPECTOR_PORT),
      '--persist-to', path.join(STATE, 'd1'),
      '--var', `ROOM_ID:${room}`, '--var', `CONNECTION_ID:${connection}`,
      '--no-show-interactive-dev-session',
    ], env, text => {
      if (/\[ERROR\]|(?:^|\n)\s*(?:Error|[A-Za-z]+Error):/.test(text)) {
        failed.abort(new Error('Wrangler reported an error'));
      }
      if (isReady(text)) ready();
    }, error => { if (!closing) failed.abort(error); });
    await waitFor(readiness, 30000, signal, 'Wrangler readiness');
    signal.throwIfAborted();
    cargo = start('cargo', [
      'test', '--manifest-path', path.join(DESKTOP, 'Cargo.toml'), '--locked', '--offline',
      `tests::${name}`, '--', '--ignored', '--exact', '--nocapture',
    ], env);
    const result = await waitFor(cargo.closed, 120000, signal, 'Cargo');
    signal.throwIfAborted();
    if (!cargoPassed(name, result, cargo.stdout.text)) {
      throw new Error(`Cargo did not prove exactly one passing test: code=${result.code} signal=${result.signal} error=${result.error || 'none'}`);
    }
  } catch (error) {
    failure = error;
  } finally {
    // Stop Cargo first; keep detecting unexpected Wrangler failure until its cleanup.
    const cleanupErrors = [];
    try { await stop(cargo); } catch (error) { cleanupErrors.push(error); }
    if (signal.aborted && !failure) failure = signal.reason;
    closing = true;
    try { await stop(worker); } catch (error) { cleanupErrors.push(error); }
    if (cleanupErrors.length) {
      failure = new AggregateError([...(failure ? [failure] : []), ...cleanupErrors],
        'Case/cleanup failed: ' + cleanupErrors.map(error => error.message).join('; '));
    }
    if (failure) {
      for (const [label, record] of [['Wrangler', worker], ['Cargo', cargo]]) {
        if (record) console.error(`${label} bounded output:\n${record.stdout.text}\n${record.stderr.text}`);
      }
    }
  }
  if (failure) throw failure;
  console.log(`PASS ${name}: 1 passed; 0 failed; 0 ignored; cleanup complete`);
}

async function selfTest() {
  const abort = new AbortController();
  abort.abort(new Error('test interruption'));
  await assert.rejects(waitFor(new Promise(() => {}), 1000, abort.signal, 'test'), /test interruption/);
  await assert.rejects(waitFor(new Promise(() => {}), 1, null, 'test'), /timeout/);
  await assert.rejects(waitFor(Promise.reject(new Error('test failure')), 1000, null, 'test'), /test failure/);
  assert.equal(await waitFor(Promise.resolve('done'), 1000, null, 'test'), 'done');
  const ok = { code: 0, signal: null };
  const name = TESTS[0];
  const output = `test tests::${name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 7 filtered out; finished in 0.1s\n`;
  assert.deepEqual(argumentsFor([]), TESTS);
  assert.deepEqual(argumentsFor([name]), [name]);
  assert.equal(argumentsFor(['--self-test']), null);
  for (const args of [['bad'], ['--help'], [name, '--self-test'], ['wrangler_echo;exit'], ['']]) {
    assert.throws(() => argumentsFor(args), /Invalid argument/);
  }
  const bytes = Buffer.from('ação 日本語\n[wrangler:info] Ready on http://127.0.0.1:18849\n');
  for (let split = 0; split <= bytes.length; split++) {
    const stream = capture();
    stream.push(bytes.subarray(0, split));
    stream.push(bytes.subarray(split));
    stream.end();
    assert.equal(stream.text, bytes.toString());
    assert.equal(isReady(stream.text), true);
  }
  const partial = capture();
  for (const byte of bytes) partial.push(Buffer.from([byte]));
  assert.equal(partial.text, bytes.toString());
  assert.equal(isReady('Ready on http://127.0.0.1:1884'), false);
  assert.equal(isReady('Ready on http://127.0.0.1:188490\n'), false);
  assert.equal(isReady('Ready on http://127.0.0.1:18849/other\n'), false);
  assert.equal(isReady('Ready on http://0.0.0.0:18849\n'), false);
  let detected = false;
  const large = capture(text => { detected ||= isReady(text); });
  large.push(Buffer.concat([bytes, Buffer.alloc(CAPTURE_LIMIT * 2, 120)]));
  assert.equal(detected, true);
  assert.equal(large.text.length, CAPTURE_LIMIT);
  partial.push(Buffer.alloc(CAPTURE_LIMIT * 2, 120));
  assert.equal(partial.text.length, CAPTURE_LIMIT);
  assert.equal(cargoPassed(name, ok, output), true);
  for (const bad of ['', 'test result: ok. 0 passed; 0 failed; 0 ignored;',
    output.replace('1 passed', '0 passed'), output.replace('0 ignored', '1 ignored'),
    output.replace('0 failed', '1 failed'), output.replace(name, `${name}_other`),
    output.replace('... ok', '... FAILED')]) {
    assert.equal(cargoPassed(name, ok, bad), false);
  }
  assert.equal(cargoPassed(name, { code: 1, signal: null }, output), false);
  assert.equal(cargoPassed(name, { code: 0, signal: 'SIGTERM' }, output), false);
  assert.equal(cargoPassed(name, { ...ok, error: new Error('spawn') }, output), false);
  console.log('SELF-TEST PASS: arguments, chunk readiness, UTF-8, capture bound, Cargo verdict');
}

async function main() {
  const tests = argumentsFor(process.argv.slice(2));
  if (tests === null) return selfTest();
  // ponytail: tree cleanup is Windows-only; add/test process groups before POSIX support.
  if (process.platform !== 'win32') throw new Error('Integration runner supports Windows only');
  const interruption = new AbortController();
  const handlers = new Map(['SIGINT', 'SIGTERM', 'SIGBREAK'].map(signal => [signal, () => {
    process.exitCode = signal === 'SIGINT' ? 130 : 143;
    interruption.abort(new Error(`Interrupted: ${signal}`));
  }]));
  for (const [signal, handler] of handlers) process.on(signal, handler);
  try {
    // Fail fast: never start another case following a failed cleanup.
    for (const name of tests) await runCase(name, interruption.signal);
    interruption.signal.throwIfAborted();
    console.log(`PASS ${tests.length}/${tests.length} cases`);
  } finally {
    for (const [signal, handler] of handlers) process.removeListener(signal, handler);
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode ||= 1;
});

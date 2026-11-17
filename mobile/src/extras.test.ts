import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { AuthError } from './auth.ts';
import { agentList, agentResume, sshList, sshConnect, historyQuery, historyRun, parseExtraReply, isExtraReply, groupByFolder, baseName, type Agent } from './extras.ts';

const fx = JSON.parse(readFileSync(new URL('../../desktop/tests/fixtures/c3-extras.json', import.meta.url), 'utf8'));
const res = (f: () => unknown) => { try { f(); return 'ok'; } catch (e) { if (e instanceof AuthError) return e.code; throw e; } };
const SID = '0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e';
const body = (n: string) => fx.requests.find((c: any) => c.name === n).body;

test('pedidos: os construtores montam o mesmo corpo que o host aceita', () => {
  assert.deepEqual(agentList(), body('agent_list'));
  assert.deepEqual(agentResume(SID, 80, 24), body('agent_resume'));
  assert.deepEqual(sshList(), body('ssh_list'));
  assert.deepEqual(sshConnect('srv-1', 80, 24), body('ssh_connect'));
  assert.deepEqual(historyQuery('git', 50), body('history_query'));
  assert.deepEqual(historyRun('h_42', SID), body('history_run'));
  for (const n of ['agent_list', 'agent_resume', 'ssh_list', 'ssh_connect', 'history_query', 'history_run'])
    assert.equal(fx.requests.find((c: any) => c.name === n).result, 'ok', n);
});

test('respostas: mesmo resultado esperado pelo host', () => {
  let n = 0;
  for (const c of fx.replies) { assert.equal(res(() => parseExtraReply(c.text, fx.connection_id)), c.result, c.name); n++; }
  assert.ok(n >= 19);
});

test('respostas: valores', () => {
  const t = (n: string) => parseExtraReply(fx.replies.find((c: any) => c.name === n).text, fx.connection_id);
  const al = t('agent_list_ok');
  assert.ok(al.t === 'agent_list_ok');
  assert.equal(al.agents[0].sessionId, SID);
  assert.equal(al.agents[0].lastActivityMs, 1_700_000_000_000n);
  assert.equal(al.agents[1].cwd, null);
  const sl = t('ssh_list_ok');
  assert.ok(sl.t === 'ssh_list_ok');
  assert.deepEqual(sl.servers.map(s => [s.port, s.latencyMs]), [[22, 38], [2222, null]]);
  const hq = t('history_query_ok');
  assert.ok(hq.t === 'history_query_ok');
  assert.deepEqual(hq.entries.map(e => [e.command, e.exitCode]), [['git status', 0], ['npm test', null]]);
  const r = t('ssh_connect_ok');
  assert.ok(r.t === 'ssh_connect_ok' && r.sessionId === SID);
});

test('isExtraReply separa respostas extras das C3', () => {
  assert.equal(isExtraReply(fx.replies[0].text), true);
  assert.equal(isExtraReply('{"type":"session_list_ok"}'), false);
  assert.equal(isExtraReply('nada'), false);
});

test('agrupa agentes por pasta, mais recente primeiro', () => {
  const a = (cwd: string | null, ms: bigint): Agent => ({ sessionId: SID, agent: 'claude', label: '', cwd, state: 'running', lastActivityMs: ms });
  const g = groupByFolder([a('C:/x', 1n), a(null, 5n), a('C:/x', 9n)]);
  assert.deepEqual(g.map(x => [x.folder, x.agents.map(y => y.lastActivityMs)]), [['C:/x', [9n, 1n]], ['Sem pasta', [5n]]]);
  assert.equal(baseName('C:\\proj\\api\\'), 'api');
});

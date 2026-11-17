import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { AuthError } from './auth.ts';
import {
  agentList, agentResume, agentCreate, agentRemove, sshList, sshConnect, sshSave, sshRemove, sshProbe, sshReconnect,
  historyQuery, historyRun, historyClear, parseExtraReply, isExtraReply, groupByFolder, groupByDay, baseName, sshFormError, validArgs,
  type Agent, type HistoryEntry,
} from './extras.ts';

const fx = JSON.parse(readFileSync(new URL('../../desktop/tests/fixtures/c3-extras.json', import.meta.url), 'utf8'));
const res = (f: () => unknown) => { try { f(); return 'ok'; } catch (e) { if (e instanceof AuthError) return e.code; throw e; } };
const SID = '0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e';
const body = (n: string) => fx.requests.find((c: any) => c.name === n).body;

test('pedidos: os construtores montam o mesmo corpo que o host aceita', () => {
  assert.deepEqual(agentList(), body('agent_list'));
  assert.deepEqual(agentResume(SID, 80, 24), body('agent_resume'));
  assert.deepEqual(sshList(), body('ssh_list'));
  assert.deepEqual(sshConnect('srv-1', 80, 24), body('ssh_connect'));
  assert.deepEqual(historyQuery({ query: 'git' }), body('history_query'));
  assert.deepEqual(historyQuery({ days: 7, kind: 'pwsh', errors: true, limit: 200 }), body('history_query_filtros'));
  assert.deepEqual(historyRun('h_42', SID), body('history_run'));
  assert.deepEqual(historyClear(), body('history_clear'));
  assert.deepEqual(agentCreate('claude', 'C:\\proj\\api', '--model opus', 80, 24), body('agent_create'));
  assert.deepEqual(agentCreate('codex', null, '', 80, 24), body('agent_create_minimo'));
  assert.deepEqual(agentRemove(SID), body('agent_remove'));
  assert.deepEqual(sshSave({ label: 'Produção', user: 'deploy', host: '10.0.0.5', port: 22, key: null }), body('ssh_save'));
  assert.deepEqual(sshSave({ serverId: 'srv-1', label: 'NAS', user: 'admin', host: 'nas.local', port: 2222, key: 'C:\\Users\\eu\\.ssh\\id_ed25519' }), body('ssh_save_editar'));
  assert.deepEqual(sshRemove('srv-1'), body('ssh_remove'));
  assert.deepEqual(sshProbe('srv-1'), body('ssh_probe'));
  assert.deepEqual(sshReconnect(SID, 80, 24), body('ssh_reconnect'));
  for (const n of ['agent_list', 'agent_resume', 'agent_create', 'agent_create_minimo', 'agent_remove', 'ssh_list', 'ssh_connect', 'ssh_save',
    'ssh_save_editar', 'ssh_remove', 'ssh_probe', 'ssh_reconnect', 'history_query', 'history_query_filtros', 'history_run', 'history_clear'])
    assert.equal(fx.requests.find((c: any) => c.name === n).result, 'ok', n);
});

test('respostas: mesmo resultado esperado pelo host', () => {
  let n = 0;
  for (const c of fx.replies) { assert.equal(res(() => parseExtraReply(c.text, fx.connection_id)), c.result, c.name); n++; }
  assert.ok(n >= 33);
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
  assert.deepEqual(sl.servers.map(s => [s.port, s.latencyMs, s.status]), [[22, 38, 'ok'], [2222, null, 'timeout']]);
  assert.equal(sl.servers[1].key, 'C:\\k');
  assert.deepEqual(sl.sessions.map(x => [x.sessionId, x.connected]), [[SID, false]]);
  const hq = t('history_query_ok');
  assert.ok(hq.t === 'history_query_ok');
  assert.deepEqual(hq.entries.map(e => [e.command, e.exitCode]), [['git status', 0], ['npm test', null]]);
  assert.ok(hq.entries.every(e => e.kind && e.label && e.pc));
  const sv = t('ssh_save_ok');
  assert.ok(sv.t === 'ssh_save_ok' && sv.serverId.length > 0);
  const r = t('ssh_connect_ok');
  assert.ok(r.t === 'ssh_connect_ok' && r.sessionId === SID);
});

test('isExtraReply separa respostas extras das C3', () => {
  assert.equal(isExtraReply(fx.replies[0].text), true);
  assert.equal(isExtraReply('{"type":"session_list_ok"}'), false);
  assert.equal(isExtraReply('nada'), false);
});

test('agrupa agentes por pasta, mais recente primeiro', () => {
  const a = (cwd: string | null, ms: bigint): Agent => ({ sessionId: SID, agent: 'claude', args: '', label: '', cwd, state: 'running', lastActivityMs: ms });
  const g = groupByFolder([a('C:/x', 1n), a(null, 5n), a('C:/x', 9n)]);
  assert.deepEqual(g.map(x => [x.folder, x.agents.map(y => y.lastActivityMs)]), [['C:/x', [9n, 1n]], ['Sem pasta', [5n]]]);
  assert.equal(baseName('C:\\proj\\api\\'), 'api');
});

test('agrupa histórico por dia como a tela desktop', () => {
  const now = new Date(2026, 10, 17, 12);
  const e = (d: Date): HistoryEntry => ({ entryId: '1', command: 'x', cwd: null, atMs: BigInt(d.getTime()), exitCode: null, kind: 'pwsh', label: 'L', pc: 'Este PC' });
  const g = groupByDay([e(new Date(2026, 10, 17, 9)), e(new Date(2026, 10, 17, 8)), e(new Date(2026, 10, 16, 23)), e(new Date(2026, 10, 1))], now);
  assert.deepEqual(g.map(x => [x.day, x.entries.length]).slice(0, 2), [['Hoje', 2], ['Ontem', 1]]);
  assert.equal(g.length, 3);
});

test('formulário SSH e args do agente: mesmas regras do host', () => {
  const f = { label: 'srv', user: 'deploy', host: 'h.exemplo.com', port: 22, key: null };
  assert.equal(sshFormError(f), null);
  assert.equal(sshFormError({ ...f, label: ' ' }), 'INVALID_SSH_NAME');
  assert.equal(sshFormError({ ...f, user: '-oProxyCommand=calc' }), 'INVALID_SSH_USER');
  assert.equal(sshFormError({ ...f, host: 'a b' }), 'INVALID_SSH_HOST');
  assert.equal(sshFormError({ ...f, port: 70000 }), 'INVALID_SSH_PORT');
  assert.equal(sshFormError({ ...f, key: 'id_rsa' }), 'INVALID_SSH_KEY');
  assert.equal(sshFormError({ ...f, key: 'C:\\k\\id' }), null);
  assert.ok(!('password' in sshSave({ ...f, password: 'x' } as any)), 'senha nunca vai no pedido');
  assert.equal(validArgs('--model opus'), true);
  assert.equal(validArgs('a; rm -rf'), false);
});

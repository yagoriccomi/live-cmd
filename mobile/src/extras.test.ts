import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { AuthError } from './auth.ts';
import { parseHostControl } from './pty.ts';
import {
  capsProbe, agentList, agentResume, agentCreate, agentRemove, sshList, sshConnect, sshSave, sshRemove, sshProbe, sshReconnect,
  historyQuery, historyRun, historyClear, parseExtraReply, isExtraReply, groupByFolder, groupByDay, baseName, sshFormError, validArgs,
  folderList, sshKeyList, agentDefaultGet, agentDefaultSet, joinPath, parentPath, settingsGet, settingsSet,
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
  assert.deepEqual(folderList('C:\\proj'), body('folder_list'));
  assert.deepEqual(folderList(''), body('folder_list_unidades'));
  assert.deepEqual(sshKeyList(), body('ssh_key_list'));
  assert.deepEqual(sshKeyList('C:\\Users\\eu\\.ssh\\work'), body('ssh_key_list_pasta'));
  assert.deepEqual(agentDefaultGet(), body('agent_default_get'));
  assert.deepEqual(agentDefaultSet('claude', 'C:\\proj\\api', '--model opus'), body('agent_default_set'));
  assert.deepEqual(agentDefaultSet('codex', null, ''), body('agent_default_set_minimo'));
  for (const n of ['agent_list', 'agent_resume', 'agent_create', 'agent_create_minimo', 'agent_remove', 'ssh_list', 'ssh_connect', 'ssh_save',
    'ssh_save_editar', 'ssh_remove', 'ssh_probe', 'ssh_reconnect', 'history_query', 'history_query_filtros', 'history_run', 'history_clear',
    'folder_list', 'folder_list_unidades', 'ssh_key_list', 'ssh_key_list_pasta', 'agent_default_get', 'agent_default_set', 'agent_default_set_minimo'])
    assert.equal(fx.requests.find((c: any) => c.name === n).result, 'ok', n);
});

test('respostas: mesmo resultado esperado pelo host', () => {
  let n = 0;
  for (const c of fx.replies) { assert.equal(res(() => parseExtraReply(c.text, fx.connection_id)), c.result, c.name); n++; }
  assert.ok(n >= 47);
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
  const fl = t('folder_list_ok');
  assert.ok(fl.t === 'folder_list_ok');
  assert.deepEqual([fl.path, fl.dirs], ['C:\\proj', ['api', 'web']]);
  const un = t('folder_list_unidades');
  assert.ok(un.t === 'folder_list_ok' && un.path === '' && un.dirs[0] === 'C:\\');
  const kl = t('ssh_key_list_ok');
  assert.ok(kl.t === 'ssh_key_list_ok');
  assert.deepEqual([kl.root, kl.path, kl.dirs, kl.keys], ['C:\\Users\\eu\\.ssh', 'C:\\Users\\eu\\.ssh', ['work'], ['id_ed25519', 'id_rsa']]);
  const ad = t('agent_default_ok');
  assert.ok(ad.t === 'agent_default_ok');
  assert.deepEqual(ad.value, { agent: 'claude', cwd: 'C:\\proj\\api', args: '--model opus' });
  const an = t('agent_default_nunca_salvo');
  assert.ok(an.t === 'agent_default_ok' && an.value === null);
  assert.equal(t('agent_default_set_ok').t, 'agent_default_set_ok');
});

test('caminhos: mesmo navegador do desktop (subir até as unidades)', () => {
  assert.equal(joinPath('', 'C:\\'), 'C:\\');
  assert.equal(joinPath('C:\\', 'proj'), 'C:\\proj');
  assert.equal(joinPath('C:\\proj\\', 'api'), 'C:\\proj\\api');
  assert.equal(parentPath('C:\\proj\\api'), 'C:\\proj\\');
  assert.equal(parentPath('C:\\proj\\'), 'C:\\');
  assert.equal(parentPath('C:\\'), '');
});

test('funções dos mockups: pedidos com ext e aparência', () => {
  assert.deepEqual(agentList(true), body('agent_list_ext'));
  assert.deepEqual(sshList(true), body('ssh_list_ext'));
  assert.deepEqual(historyQuery({ ext: true }), body('history_query_ext'));
  assert.deepEqual(settingsGet(), body('settings_get'));
  assert.deepEqual(settingsSet({ theme: 'cosmo', fontSize: 14, glow: false }), body('settings_set'));
  assert.deepEqual(settingsSet({ theme: 'magenta', fontFamily: 'Consolas', fontSize: 10, opacity: 60, blur: 0, glow: true, animations: false }), body('settings_set_tudo'));
  for (const n of ['agent_list_ext', 'ssh_list_ext', 'history_query_ext', 'settings_get', 'settings_set', 'settings_set_tudo'])
    assert.equal(fx.requests.find((c: any) => c.name === n).result, 'ok', n);
  const s = sshSave({ label: 'x', user: 'u', host: 'h', port: 22, key: null, keepaliveS: 30, autoReconnect: true });
  assert.deepEqual([s.keepalive_s, s.auto_reconnect], [30, true]);
  assert.ok(!('keepalive_s' in sshSave({ label: 'x', user: 'u', host: 'h', port: 22, key: null })), 'sem o cap, corpo antigo');
});

test('funções dos mockups: respostas', () => {
  const t = (n: string) => parseExtraReply(fx.replies.find((c: any) => c.name === n).text, fx.connection_id);
  const al = t('agent_list_ext');
  assert.ok(al.t === 'agent_list_ok' && al.agents[0].state === 'waiting');
  const sl = t('ssh_list_ext');
  assert.ok(sl.t === 'ssh_list_ok' && sl.servers[0].keepaliveS === 30 && sl.servers[0].autoReconnect === true);
  const hq = t('history_query_ext');
  assert.ok(hq.t === 'history_query_ok');
  assert.deepEqual(hq.sessions?.map(g => [g.label, g.day, g.count, g.lastMs]), [['PowerShell', '2023-11-14', 1, 1_700_000_000_000n], ['srv', '1969-12-31', 1, 7n]]);
  const sg = t('settings_set_ok');
  assert.ok(sg.t === 'settings_set_ok' && sg.settings.theme === 'cosmo' && sg.settings.fontFamily === 'Fira Code');
  const w = t('agent_waiting');
  assert.deepEqual(w, { t: 'agent_waiting', sessionId: SID, label: 'api', waiting: true });
  assert.equal(isExtraReply(fx.replies.find((c: any) => c.name === 'agent_waiting').text), true);
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

test('capacidades: pergunta que o host 0.1.8 entende; host novo responde caps_ok, antigo NOT_FOUND não fatal', () => {
  const p = fx.caps_probe;
  assert.deepEqual(capsProbe(), p.request);
  assert.equal(isExtraReply(p.reply), true);
  const r = parseExtraReply(p.reply, fx.connection_id);
  assert.ok(r.t === 'caps_ok');
  assert.deepEqual(r.caps, fx.caps);
  for (const c of ['agents', 'ssh', 'history', 'folders', 'ssh_keys', 'agent_default']) assert.ok(r.caps.includes(c), c);
  // Host antigo: `error` com request_id, resposta comum de pedido (pty.ts), não fecha a conexão.
  assert.equal(isExtraReply(p.reply_host_antigo), false);
  const old = JSON.parse(p.reply_host_antigo);
  assert.deepEqual([old.type, old.code, typeof old.request_id], ['error', 'NOT_FOUND', 'string']);
  assert.doesNotThrow(() => parseHostControl(p.reply_host_antigo, fx.connection_id));
  for (const bad of [['SSH'], 'ssh', [1]]) assert.equal(res(() => parseExtraReply(JSON.stringify({ ...JSON.parse(p.reply), caps: bad }), fx.connection_id)), 'INVALID_MESSAGE');
});

import { useCallback, useState } from 'react';
import { Pressable, ScrollView, Text, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { AGENTS, agentCreate, agentDefaultGet, agentDefaultSet, agentList, agentRemove, agentResume, baseName, groupByFolder, validArgs, type Agent, type AgentDefault, type AgentState } from '../extras.ts';
import { Browser } from './Browser.tsx';
import { Dot, Field, Glass, Header, Notice, OFFLINE, OLD_HOST, Pill, confirm, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, ago, errText } from './theme.ts';

const STATE: Record<AgentState, { color: string; text: string }> = {
  running: { color: C.ok, text: 'em execução' },
  waiting: { color: C.warn, text: 'esperando confirmação' },
  saved: { color: C.warn, text: 'gravado' },
  exited: { color: C.err, text: 'encerrado' },
};

type Form = { agent: string; cwd: string; args: string; save: boolean; browse: boolean };

/** Sessões de IA do PC agrupadas por pasta; mesmas ações da tela desktop (agents.js): nova sessão (com navegador de pastas
 *  do PC e "Salvar como padrão", o mesmo padrão do diálogo desktop), abrir/retomar, remover. */
export function AgentsScreen({ conn, onOpen }: { conn: Conn | null; onOpen: (sessionId: string) => Promise<void> }) {
  const ok = !!conn && conn.caps.includes('agents');
  const load = useCallback(() => conn!.extra(agentList()).then(r => r.agents as Agent[]), [conn]);
  const { data, err, busy, reload } = useRemote(ok ? load : null);
  const [acting, setActing] = useState('');
  const [msg, setMsg] = useState('');
  const [form, setForm] = useState<Form | null>(null);
  const canBrowse = !!conn && conn.caps.includes('folders');
  const canDefault = !!conn && conn.caps.includes('agent_default');

  // Como o desktop: o formulário abre com o padrão salvo no PC (se houver).
  async function open() {
    setMsg('');
    let d: AgentDefault | null = null;
    if (canDefault) { try { d = (await conn!.extra(agentDefaultGet())).value; } catch (e) { setMsg(errText(e)); } }
    setForm({ agent: d?.agent ?? 'claude', cwd: d?.cwd ?? '', args: d?.args ?? '', save: false, browse: false });
  }

  async function resume(a: Agent) {
    setActing(a.sessionId); setMsg('');
    try { const r = await conn!.extra(agentResume(a.sessionId, 80, 24)); await onOpen(r.sessionId); }
    catch (e) { setMsg(errText(e)); } finally { setActing(''); }
  }

  async function remove(a: Agent) {
    const name = a.label || a.agent;
    const live = a.state === 'running';
    if (!await confirm(`Remover ${name}?`, live
      ? 'A sessão está aberta: o terminal será encerrado e ela sai da lista.'
      : 'A sessão sai da lista e não poderá mais ser retomada por aqui.', 'Remover')) return;
    setActing(a.sessionId); setMsg('');
    try { await conn!.extra(agentRemove(a.sessionId)); } catch (e) { setMsg(errText(e)); } finally { setActing(''); }
    reload();
  }

  async function create() {
    if (!form) return;
    const cwd = form.cwd.trim(), args = form.args.trim();
    if (!validArgs(args)) return setMsg('Comandos extras inválidos: use só letras, números, espaço e . _ = / : , -');
    if (cwd && !/^([A-Za-z]:[\\/]|\\\\)/.test(cwd)) return setMsg('Informe o caminho completo da pasta no PC (ex.: C:\\projetos\\app).');
    setActing('new'); setMsg('');
    try {
      if (form.save) await conn!.extra(agentDefaultSet(form.agent, cwd || null, args));
      const r = await conn!.extra(agentCreate(form.agent, cwd || null, args, 80, 24));
      setForm(null);
      await onOpen(r.sessionId);
    }
    catch (e) { setMsg(errText(e) === 'Não encontrado no PC.' ? 'Pasta não encontrada no PC.' : errText(e)); }
    finally { setActing(''); }
  }

  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }} keyboardShouldPersistTaps="handled">
      <Header title="Agentes" subtitle="Sessões de IA no PC, por pasta" onReload={ok ? reload : undefined} busy={busy} />
      {!conn && <Notice text={OFFLINE} />}
      {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
      {ok && !form && <View style={[s.wrap, { justifyContent: 'flex-end' }]}><Pill title="Nova sessão de agente" primary onPress={open} /></View>}
      {ok && form && (
        <Glass>
          <Text style={s.name} accessibilityRole="header">Nova sessão de agente</Text>
          <Text style={[s.meta, { marginTop: 8 }]}>Agente</Text>
          <View style={[s.wrap, { marginTop: 4 }]} accessibilityRole="radiogroup">
            {AGENTS.map(a => <Pill key={a} title={a} primary={form.agent === a} onPress={() => setForm({ ...form, agent: a })} label={`Agente ${a}`} />)}
          </View>
          <Field label="Pasta no PC (opcional)" value={form.cwd} onChangeText={cwd => setForm({ ...form, cwd })} placeholder="C:\projetos\app" />
          {canBrowse && (
            <View style={[s.wrap, { marginTop: -4 }]}>
              <Pill title={form.browse ? 'Fechar pastas' : 'Procurar pasta no PC'} onPress={() => setForm({ ...form, browse: !form.browse })} />
            </View>
          )}
          {canBrowse && form.browse && <Browser conn={conn!} mode="folders" start={form.cwd.trim()} onPick={cwd => setForm(f => f && { ...f, cwd })} />}
          <Field label="Comandos extras (opcional)" value={form.args} onChangeText={args => setForm({ ...form, args })} placeholder="--model opus" />
          {canDefault && (
            <Pressable onPress={() => setForm({ ...form, save: !form.save })} accessibilityRole="checkbox" accessibilityState={{ checked: form.save }}
              accessibilityLabel="Salvar como padrão" style={[s.row, { minHeight: 44, marginBottom: 6 }]}>
              <View style={{ width: 22, height: 22, borderRadius: 6, borderWidth: 2, borderColor: C.accent2, backgroundColor: form.save ? C.accent : 'transparent', alignItems: 'center', justifyContent: 'center' }}>
                {form.save && <Text style={{ color: '#fff', fontWeight: '700' }}>✓</Text>}
              </View>
              <Text style={{ color: C.text }}>Salvar como padrão</Text>
            </Pressable>
          )}
          <View style={[s.wrap, { justifyContent: 'flex-end' }]}>
            <Pill title="Cancelar" onPress={() => { setForm(null); setMsg(''); }} />
            <Pill title={acting === 'new' ? 'Iniciando…' : 'Iniciar'} primary disabled={!!acting} onPress={create} />
          </View>
        </Glass>
      )}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {ok && data?.length === 0 && <Notice text="Nenhuma sessão de agente no PC." />}
      {ok && data && groupByFolder(data).map(g => (
        <View key={g.folder}>
          <Text style={s.group} accessibilityRole="header">{g.folder === 'Sem pasta' ? g.folder : baseName(g.folder)}</Text>
          {g.agents.map(a => {
            const st = STATE[a.state];
            const name = a.label || a.agent;
            const go = a.state === 'running' ? 'Abrir' : 'Retomar';
            return (
              <Glass key={a.sessionId}>
                <View style={s.row}>
                  <Dot color={st.color} label={st.text} />
                  <View style={{ flex: 1 }}>
                    <Text style={s.name} numberOfLines={1}>{name}</Text>
                    <Text style={s.meta} numberOfLines={1}>{[a.agent + (a.args ? ` ${a.args}` : ''), st.text, ago(a.lastActivityMs)].join(' · ')}</Text>
                  </View>
                </View>
                <View style={[s.wrap, { justifyContent: 'flex-end', marginTop: 8, marginBottom: 0 }]}>
                  <Pill title="Remover" disabled={!!acting} onPress={() => remove(a)} label={`Remover ${name}`} />
                  <Pill title={go} primary disabled={!!acting} onPress={() => resume(a)} label={`${go} ${name}`} />
                </View>
              </Glass>
            );
          })}
        </View>
      ))}
    </ScrollView>
  );
}

import { useCallback, useState } from 'react';
import { Pressable, ScrollView, Text, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { AGENTS, agentCreate, agentList, agentRemove, agentResume, baseName, groupByFolder, validArgs, type Agent, type AgentState } from '../extras.ts';
import { Dot, Field, Notice, OFFLINE, OLD_HOST, confirm, s, useRemote } from './common.tsx';
import { Chip, GroupLabel, Hero, HeroBtn, IconBtn, ListPanel, Panel, Row, Section, TopBar, screen, tl as t } from './aurora.tsx';
import { C, TAB_SPACE, ago, errText } from './theme.ts';

const STATE: Record<AgentState, { color: string; text: string }> = {
  running: { color: C.ok, text: 'em execução' },
  saved: { color: C.ok, text: 'gravado' },
  exited: { color: C.err, text: 'encerrado' },
};

/** Sessões de IA do PC agrupadas por pasta; mesmas ações da tela desktop (agents.js): nova sessão, abrir/retomar, remover. */
export function AgentsScreen({ conn, onOpen }: { conn: Conn | null; onOpen: (sessionId: string) => Promise<void> }) {
  const ok = !!conn && conn.caps.includes('agents');
  const load = useCallback(() => conn!.extra(agentList()).then(r => r.agents as Agent[]), [conn]);
  const { data, err, busy, reload } = useRemote(ok ? load : null);
  const [acting, setActing] = useState('');
  const [msg, setMsg] = useState('');
  const [form, setForm] = useState<{ agent: string; cwd: string; args: string } | null>(null);

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
    try { const r = await conn!.extra(agentCreate(form.agent, cwd || null, args, 80, 24)); setForm(null); await onOpen(r.sessionId); }
    catch (e) { setMsg(errText(e) === 'Não encontrado no PC.' ? 'Pasta não encontrada no PC.' : errText(e)); }
    finally { setActing(''); }
  }

  const running = (data ?? []).filter(a => a.state === 'running');
  const others = (data ?? []).filter(a => a.state !== 'running');
  const hhmm = (ms: bigint) => new Date(Number(ms)).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
  const sub = !conn ? 'sem conexão com o PC' : !ok ? 'atualize o live-cmd no PC' : data ? `${running.length} rodando · ${data.length} no PC` : 'carregando…';
  return (
    <ScrollView contentContainerStyle={[screen, { flexGrow: 1, paddingBottom: TAB_SPACE }]} keyboardShouldPersistTaps="handled">
      <TopBar title="Agentes" label="Nova sessão de agente" disabled={!ok || !!form}
        onPress={() => { setMsg(''); setForm({ agent: 'claude', cwd: '', args: '' }); }} />
      <Hero icon="✦" title="Sessões de IA no PC" sub={sub} subLive badge={running.length ? { text: 'AO VIVO', color: C.ok } : null}>
        {ok && <HeroBtn title={busy ? 'Atualizando…' : 'Atualizar'} primary disabled={busy} onPress={reload} label="Atualizar Agentes" />}
      </Hero>
      {!conn && <Notice text={OFFLINE} />}
      {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
      {ok && form && (
        <Panel title="Nova sessão de agente">
          <Text style={s.meta}>Agente</Text>
          <View style={s.wrap} accessibilityRole="radiogroup">
            {AGENTS.map(a => <Chip key={a} title={a} on={form.agent === a} onPress={() => setForm({ ...form, agent: a })} label={`Agente ${a}`} />)}
          </View>
          <Field label="Pasta no PC (opcional)" value={form.cwd} onChangeText={cwd => setForm({ ...form, cwd })} placeholder="C:\projetos\app" />
          <Field label="Comandos extras (opcional)" value={form.args} onChangeText={args => setForm({ ...form, args })} placeholder="--model opus" />
          <View style={[s.wrap, { justifyContent: 'flex-end', marginBottom: 0 }]}>
            <HeroBtn title="Cancelar" onPress={() => { setForm(null); setMsg(''); }} />
            <HeroBtn title={acting === 'new' ? 'Iniciando…' : 'Iniciar'} primary disabled={!!acting} onPress={create} />
          </View>
        </Panel>
      )}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {ok && data?.length === 0 && <Notice text="Nenhuma sessão de agente no PC." />}
      {ok && running.length > 0 && (
        <>
          <Section text="Rodando" />
          <ListPanel>
            {running.map(a => {
              const name = a.label || a.agent;
              return (
                <Row key={a.sessionId} icon="✦" title={name} sub={a.cwd ?? [a.agent, a.args].filter(Boolean).join(' ')} status={{ color: STATE.running.color, text: 'rodando' }}
                  onPress={() => resume(a)} disabled={!!acting} label={`Abrir ${name}, ${STATE.running.text}, ${ago(a.lastActivityMs)}`}
                  right={<IconBtn glyph="✕" label={`Remover ${name}`} disabled={!!acting} onPress={() => remove(a)} />} />
              );
            })}
          </ListPanel>
        </>
      )}
      {ok && others.length > 0 && (
        <Panel title="Retomar" style={{ flexGrow: 1 }}>
          {groupByFolder(others).map(g => (
            <View key={g.folder}>
              <GroupLabel text={g.folder === 'Sem pasta' ? g.folder : baseName(g.folder)} />
              {g.agents.map(a => {
                const st = STATE[a.state];
                const name = a.label || a.agent;
                return (
                  <View key={a.sessionId} style={t.line}>
                    <Pressable onPress={() => resume(a)} disabled={!!acting} style={({ pressed }) => [t.main, pressed && { opacity: 0.7 }]}
                      accessibilityRole="button" accessibilityLabel={`Retomar ${name}, ${st.text}, ${ago(a.lastActivityMs)}`}>
                      <Text style={t.time}>{hhmm(a.lastActivityMs)}</Text>
                      <Dot color={st.color} label={st.text} />
                      <Text style={t.cmd} numberOfLines={1}>{name}</Text>
                      <Text style={t.kind} numberOfLines={1}>{a.agent}</Text>
                    </Pressable>
                    <IconBtn glyph="✕" label={`Remover ${name}`} disabled={!!acting} onPress={() => remove(a)} />
                  </View>
                );
              })}
            </View>
          ))}
        </Panel>
      )}
    </ScrollView>
  );
}

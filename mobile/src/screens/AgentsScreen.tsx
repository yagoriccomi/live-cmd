import { useCallback, useState } from 'react';
import { ScrollView, Text, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { agentList, agentResume, baseName, groupByFolder, type Agent, type AgentState } from '../extras.ts';
import { Dot, Glass, Header, Notice, OFFLINE, Pill, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, ago, errText } from './theme.ts';

const STATE: Record<AgentState, { color: string; text: string }> = {
  running: { color: C.ok, text: 'em execução' },
  saved: { color: C.warn, text: 'gravado' },
  exited: { color: C.err, text: 'encerrado' },
};

/** Sessões de IA do PC agrupadas por pasta; "Retomar" abre a conversa (`--resume`) na aba Sessões. */
export function AgentsScreen({ conn, onOpen }: { conn: Conn | null; onOpen: (sessionId: string) => Promise<void> }) {
  const load = useCallback(() => conn!.extra(agentList()).then(r => r.agents as Agent[]), [conn]);
  const { data, err, busy, reload } = useRemote(conn ? load : null);
  const [acting, setActing] = useState('');
  const [msg, setMsg] = useState('');

  async function resume(a: Agent) {
    setActing(a.sessionId); setMsg('');
    try { const r = await conn!.extra(agentResume(a.sessionId, 80, 24)); await onOpen(r.sessionId); }
    catch (e) { setMsg(errText(e)); } finally { setActing(''); }
  }

  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }}>
      <Header title="Agentes" subtitle="Sessões de IA no PC, por pasta" onReload={conn ? reload : undefined} busy={busy} />
      {!conn && <Notice text={OFFLINE} />}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {conn && data?.length === 0 && <Notice text="Nenhuma sessão de agente no PC." />}
      {data && groupByFolder(data).map(g => (
        <View key={g.folder}>
          <Text style={s.group} accessibilityRole="header">{g.folder === 'Sem pasta' ? g.folder : baseName(g.folder)}</Text>
          {g.agents.map(a => {
            const st = STATE[a.state];
            return (
              <Glass key={a.sessionId}>
                <View style={s.row}>
                  <Dot color={st.color} label={st.text} />
                  <View style={{ flex: 1 }}>
                    <Text style={s.name} numberOfLines={1}>{a.label || a.agent}</Text>
                    <Text style={s.meta} numberOfLines={1}>{a.agent} · {st.text} · {ago(a.lastActivityMs)}</Text>
                  </View>
                  <Pill title={a.state === 'running' ? 'Abrir' : 'Retomar'} primary disabled={!!acting} onPress={() => resume(a)}
                    label={`${a.state === 'running' ? 'Abrir' : 'Retomar'} ${a.label || a.agent}`} />
                </View>
              </Glass>
            );
          })}
        </View>
      ))}
    </ScrollView>
  );
}

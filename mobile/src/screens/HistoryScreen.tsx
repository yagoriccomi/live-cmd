import { useCallback, useState } from 'react';
import { ScrollView, Text, TextInput, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { historyQuery, historyRun, type HistoryEntry } from '../extras.ts';
import { Dot, Glass, Header, Notice, OFFLINE, Pill, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, ago, errText } from './theme.ts';

function exit(code: number | null): { color: string; text: string } {
  if (code === null) return { color: C.muted, text: 'sem código de saída' };
  return code === 0 ? { color: C.ok, text: 'concluído' } : { color: C.err, text: `saiu com ${code}` };
}

/** Comandos gravados no PC; "Reexecutar" digita o comando na sessão aberta na aba Sessões. */
export function HistoryScreen({ conn, onRan }: { conn: Conn | null; onRan: () => void }) {
  const [query, setQuery] = useState('');
  const [applied, setApplied] = useState('');
  const load = useCallback(() => conn!.extra(historyQuery(applied, 100)).then(r => r.entries as HistoryEntry[]), [conn, applied]);
  const { data, err, busy, reload } = useRemote(conn ? load : null);
  const [msg, setMsg] = useState('');

  async function run(e: HistoryEntry) {
    setMsg('');
    if (!conn?.sessionId) return setMsg('Abra uma sessão na aba Sessões primeiro.');
    try { await conn.extra(historyRun(e.entryId, conn.sessionId)); onRan(); } catch (x) { setMsg(errText(x)); }
  }

  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }} keyboardShouldPersistTaps="handled">
      <Header title="Histórico" subtitle="Comandos gravados no PC" onReload={conn ? reload : undefined} busy={busy} />
      {conn && (
        <TextInput value={query} onChangeText={setQuery} onSubmitEditing={() => setApplied(query.trim())} returnKeyType="search"
          placeholder="Buscar comando" placeholderTextColor={C.muted} autoCapitalize="none" autoCorrect={false}
          accessibilityLabel="Buscar no histórico de comandos"
          style={{ borderWidth: 1, borderColor: C.border, borderRadius: 18, backgroundColor: C.glass, color: C.text, paddingHorizontal: 14, minHeight: 44, marginBottom: 12 }} />
      )}
      {!conn && <Notice text={OFFLINE} />}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {conn && data?.length === 0 && <Notice text={applied ? 'Nada encontrado.' : 'Nenhum comando gravado no PC.'} />}
      {data?.map(e => {
        const x = exit(e.exitCode);
        return (
          <Glass key={e.entryId}>
            <View style={s.row}>
              <Dot color={x.color} label={x.text} />
              <View style={{ flex: 1 }}>
                <Text style={s.mono} numberOfLines={2} selectable>{e.command}</Text>
                <Text style={s.meta} numberOfLines={1}>{e.cwd ? `${e.cwd} · ` : ''}{ago(e.atMs)}</Text>
              </View>
              <Pill title="Reexecutar" onPress={() => run(e)} label={`Reexecutar ${e.command}`} />
            </View>
          </Glass>
        );
      })}
    </ScrollView>
  );
}

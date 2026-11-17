import { useCallback, useState } from 'react';
import { ScrollView, Text, TextInput, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { HISTORY_KINDS, groupByDay, historyClear, historyQuery, historyRun, type HistoryEntry } from '../extras.ts';
import { Dot, Glass, Header, Notice, OFFLINE, OLD_HOST, Pill, confirm, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, errText } from './theme.ts';

function exit(code: number | null): { color: string; text: string } {
  if (code === null) return { color: C.muted, text: 'sem código de saída' };
  return code === 0 ? { color: C.ok, text: 'ok' } : { color: C.err, text: `erro ${code}` };
}
const LIMIT = 200;

/** Comandos gravados no PC, com os mesmos filtros e ações da tela desktop (historico.js): busca, últimos 7 dias,
 *  tipo de sessão, só erros, limpar; "Reexecutar" digita o comando na sessão aberta na aba Sessões. */
export function HistoryScreen({ conn, onRan }: { conn: Conn | null; onRan: () => void }) {
  const ok = !!conn && conn.caps.includes('history');
  const [query, setQuery] = useState('');
  const [applied, setApplied] = useState('');
  const [week, setWeek] = useState(false);
  const [kind, setKind] = useState<string | null>(null);
  const [errors, setErrors] = useState(false);
  const load = useCallback(() => conn!.extra(historyQuery({ query: applied, limit: LIMIT, days: week ? 7 : null, kind, errors }))
    .then(r => r.entries as HistoryEntry[]), [conn, applied, week, kind, errors]);
  const { data, err, busy, reload } = useRemote(ok ? load : null);
  const [msg, setMsg] = useState('');
  const filtered = !!(applied || week || kind || errors);

  async function run(e: HistoryEntry) {
    setMsg('');
    if (!conn?.sessionId) return setMsg('Abra uma sessão na aba Sessões primeiro.');
    try { await conn.extra(historyRun(e.entryId, conn.sessionId)); onRan(); } catch (x) { setMsg(errText(x)); }
  }
  async function clear() {
    if (!await confirm('Limpar histórico?', 'Todos os comandos gravados no PC serão apagados. Não dá para desfazer.', 'Limpar histórico')) return;
    setMsg('');
    try { await conn!.extra(historyClear()); } catch (x) { setMsg(errText(x)); }
    reload();
  }

  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }} keyboardShouldPersistTaps="handled">
      <Header title="Histórico" subtitle="Comandos gravados no PC" onReload={ok ? reload : undefined} busy={busy} />
      {!conn && <Notice text={OFFLINE} />}
      {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
      {ok && (
        <>
          <TextInput value={query} onChangeText={setQuery} onSubmitEditing={() => setApplied(query.trim())} returnKeyType="search"
            placeholder="Buscar comandos" placeholderTextColor={C.muted} autoCapitalize="none" autoCorrect={false}
            accessibilityLabel="Buscar no histórico de comandos" style={[s.input, { marginBottom: 10 }]} />
          <View style={s.wrap}>
            <Pill title="Últimos 7 dias" primary={week} onPress={() => setWeek(!week)} label={`Últimos 7 dias${week ? ', ligado' : ''}`} />
            <Pill title="Só erros" primary={errors} onPress={() => setErrors(!errors)} label={`Só erros${errors ? ', ligado' : ''}`} />
          </View>
          <View style={s.wrap} accessibilityLabel="Tipo de sessão">
            <Pill title="Todas" primary={!kind} onPress={() => setKind(null)} label="Todas as sessões" />
            {Object.entries(HISTORY_KINDS).map(([k, t]) => <Pill key={k} title={t} primary={kind === k} onPress={() => setKind(kind === k ? null : k)} label={`Só ${t}`} />)}
          </View>
          <View style={[s.row, { justifyContent: 'space-between', marginBottom: 6 }]}>
            <Text style={s.meta} accessibilityLiveRegion="polite">
              {data?.length ? `${data.length} comando${data.length > 1 ? 's' : ''}${data.length >= LIMIT ? ` (os ${LIMIT} mais recentes)` : ''}` : ''}
            </Text>
            <Pill title="Limpar histórico" disabled={!data?.length && !filtered} onPress={clear} />
          </View>
        </>
      )}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {ok && data?.length === 0 && <Notice text={filtered ? 'Nenhum comando encontrado com esses filtros.' : 'Nenhum comando gravado no PC.'} />}
      {ok && data && groupByDay(data).map(g => (
        <View key={g.day}>
          <Text style={s.group} accessibilityRole="header">{g.day}</Text>
          {g.entries.map(e => {
            const x = exit(e.exitCode);
            const time = new Date(Number(e.atMs)).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
            return (
              <Glass key={e.entryId}>
                <View style={s.row}>
                  <Dot color={x.color} label={x.text} />
                  <View style={{ flex: 1 }}>
                    <Text style={s.mono} numberOfLines={2} selectable>{e.command}</Text>
                    <Text style={s.meta} numberOfLines={2}>{[time, e.pc, e.label, HISTORY_KINDS[e.kind] ?? e.kind, e.cwd, x.text].filter(Boolean).join(' · ')}</Text>
                  </View>
                  <Pill title="Reexecutar" onPress={() => run(e)} label={`Reexecutar ${e.command}`} />
                </View>
              </Glass>
            );
          })}
        </View>
      ))}
    </ScrollView>
  );
}

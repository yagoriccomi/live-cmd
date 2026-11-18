import { useCallback, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import type { Conn } from '../conn.ts';
import * as Clipboard from 'expo-clipboard';
import { HISTORY_KINDS, groupByDay, historyClear, historyQuery, summarizeSessions, type HistoryEntry, type HistorySession } from '../extras.ts';
import { Dot, Notice, OFFLINE, OLD_HOST, confirm, useRemote } from './common.tsx';
import { Chip, GroupLabel, Hero, HeroBtn, IconBtn, ListPanel, Panel, Row, Section, TopBar, screen, tl as t } from './aurora.tsx';
import { C, F, TAB_SPACE, errText } from './theme.ts';

function exit(code: number | null): { color: string; text: string } {
  if (code === null) return { color: C.muted, text: 'sem código de saída' };
  return code === 0 ? { color: C.ok, text: 'ok' } : { color: C.err, text: `erro ${code}` };
}
const LIMIT = 200;

/** Comandos gravados no PC, com os mesmos filtros e ações da tela desktop (historico.js): busca, últimos 7 dias,
 *  tipo de sessão, só erros, limpar; "Copiar" põe o comando na área de transferência (não executa). */
export function HistoryScreen({ conn }: { conn: Conn | null }) {
  const ok = !!conn && conn.caps.includes('history');
  const [query, setQuery] = useState('');
  const [applied, setApplied] = useState('');
  const [week, setWeek] = useState(false);
  const [kind, setKind] = useState<string | null>(null);
  const [errors, setErrors] = useState(false);
  // Com o cap `history_sessions` o resumo "Sessões" vem do host (por sessão, PC e dia, sobre o histórico inteiro do filtro);
  // sem ele, é calculado aqui sobre os comandos recebidos.
  const ext = !!conn && conn.caps.includes('history_sessions');
  const load = useCallback(() => conn!.extra(historyQuery({ query: applied, limit: LIMIT, days: week ? 7 : null, kind, errors, ext }))
    .then(r => ({ entries: r.entries as HistoryEntry[], sessions: r.sessions as HistorySession[] | undefined })), [conn, applied, week, kind, errors, ext]);
  const { data: res, err, busy, reload } = useRemote(ok ? load : null);
  const data = res?.entries;
  const sessions = !data ? [] : res?.sessions
    ? res.sessions.map(x => ({ key: `${x.kind}|${x.label}|${x.pc}|${x.day}`, label: x.label || x.kind, kind: x.kind, count: x.count, errors: x.errors, lastMs: x.lastMs }))
    : summarizeSessions(data).map(x => ({ key: `${x.kind}|${x.label}`, ...x, errors: 0 }));
  const [msg, setMsg] = useState('');
  const filtered = !!(applied || week || kind || errors);

  // Histórico só consulta: copia o comando, nunca executa.
  async function copy(e: HistoryEntry) {
    try { await Clipboard.setStringAsync(e.command); setMsg('Comando copiado.'); } catch { setMsg('Não foi possível copiar.'); }
  }
  async function clear() {
    if (!await confirm('Limpar histórico?', 'Todos os comandos gravados no PC serão apagados. Não dá para desfazer.', 'Limpar histórico')) return;
    setMsg('');
    try { await conn!.extra(historyClear()); } catch (x) { setMsg(errText(x)); }
    reload();
  }

  return (
    <View style={[screen, { flex: 1, paddingBottom: TAB_SPACE }]}>
      <TopBar title="Histórico" glyph="↻" label="Atualizar Histórico" onPress={ok ? reload : undefined} busy={busy} />
      <Hero icon="⌕" badge={ok ? { text: 'BUSCA', color: '#fff' } : null}
        sub={!conn ? 'sem conexão com o PC' : !ok ? 'atualize o live-cmd no PC'
          : data?.length ? `${data.length} comando${data.length > 1 ? 's' : ''}${data.length >= LIMIT ? ` (os ${LIMIT} mais recentes)` : ''}` : busy ? 'carregando…' : 'nenhum comando'}
        subLive
        top={<TextInput value={query} onChangeText={setQuery} onSubmitEditing={() => setApplied(query.trim())} returnKeyType="search" editable={ok}
          placeholder="Buscar comandos" placeholderTextColor="rgba(244,239,255,0.75)" autoCapitalize="none" autoCorrect={false}
          accessibilityLabel="Buscar no histórico de comandos" style={st.query} />}>
        {ok && <HeroBtn title="Filtrar" primary onPress={() => setApplied(query.trim())} label="Buscar no histórico" />}
        {ok && <HeroBtn title="Limpar histórico" disabled={!data?.length && !filtered} onPress={clear} />}
      </Hero>
      {ok && !!data?.length && sessions.length > 0 && (
        <>
          <Section text="Sessões" />
          <ListPanel style={{ maxHeight: 132 }}>
            <ScrollView nestedScrollEnabled contentContainerStyle={{ gap: 4 }}>
              {sessions.map(x => {
                const when = `${groupByDay([{ ...data[0], atMs: x.lastMs }])[0].day.toLowerCase()} ${new Date(Number(x.lastMs)).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })}`;
                const on = kind === x.kind;
                return (
                  <Row key={x.key} icon={x.kind === 'ssh' ? '⇄' : '›_'} title={x.label} sub={when} selected={on}
                    status={{ color: x.errors ? C.err : C.ok, text: String(x.count) }} onPress={() => setKind(on ? null : x.kind)}
                    label={`${x.label}: ${x.count} comando${x.count > 1 ? 's' : ''}${x.errors ? `, ${x.errors} com erro` : ''}, último ${when}. ${on ? 'Mostrar todas' : `Só ${HISTORY_KINDS[x.kind] ?? x.kind}`}`} />
                );
              })}
            </ScrollView>
          </ListPanel>
        </>
      )}
      {ok && (
        <>
          <Section text="Filtros" />
          <ScrollView horizontal showsHorizontalScrollIndicator={false} style={{ flexGrow: 0 }} contentContainerStyle={{ gap: 6 }} accessibilityLabel="Tipo de sessão">
            <Chip title="Últimos 7 dias" on={week} onPress={() => setWeek(!week)} label={`Últimos 7 dias${week ? ', ligado' : ''}`} />
            <Chip title="Só erros" on={errors} onPress={() => setErrors(!errors)} label={`Só erros${errors ? ', ligado' : ''}`} />
            <Chip title="Todas" on={!kind} onPress={() => setKind(null)} label="Todas as sessões" />
            {Object.entries(HISTORY_KINDS).map(([k, t]) => <Chip key={k} title={t} on={kind === k} onPress={() => setKind(kind === k ? null : k)} label={`Só ${t}`} />)}
          </ScrollView>
        </>
      )}
      <Panel title="Linha do tempo" style={{ flex: 1 }}>
        <ScrollView style={{ flex: 1 }} keyboardShouldPersistTaps="handled">
          {!conn && <Notice text={OFFLINE} />}
          {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
          {!!err && <Notice text={err} color={C.err} />}
          {!!msg && <Notice text={msg} color={C.err} />}
          {ok && data?.length === 0 && <Notice text={filtered ? 'Nenhum comando encontrado com esses filtros.' : 'Nenhum comando gravado no PC.'} />}
          {ok && data && groupByDay(data).map(g => (
            <View key={g.day}>
              <GroupLabel text={g.day} />
              {g.entries.map(e => {
                const x = exit(e.exitCode);
                const time = new Date(Number(e.atMs)).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' });
                const meta = [time, e.pc, e.label, HISTORY_KINDS[e.kind] ?? e.kind, e.cwd, x.text].filter(Boolean).join(' · ');
                return (
                  <View key={e.entryId} style={t.line} accessible={false}>
                    <View style={t.main} accessible accessibilityLabel={`${e.command}. ${meta}`}>
                      <Text style={t.time}>{time}</Text>
                      <Dot color={x.color} label={x.text} />
                      <Text style={t.cmd} numberOfLines={1} selectable>{mark(e.command, applied)}</Text>
                      <Text style={t.kind} numberOfLines={1}>{e.exitCode ? `saída ${e.exitCode}` : e.label || (HISTORY_KINDS[e.kind] ?? e.kind)}</Text>
                    </View>
                    <IconBtn glyph="⧉" label={`Copiar ${e.command}`} onPress={() => copy(e)} />
                  </View>
                );
              })}
            </View>
          ))}
        </ScrollView>
      </Panel>
    </View>
  );
}

/** Destaca o termo buscado no comando (fundo rosa, como no mockup). */
function mark(cmd: string, q: string) {
  const i = q ? cmd.toLowerCase().indexOf(q.toLowerCase()) : -1;
  if (i < 0) return cmd;
  return <>{cmd.slice(0, i)}<Text style={st.hit}>{cmd.slice(i, i + q.length)}</Text>{cmd.slice(i + q.length)}</>;
}

const st = StyleSheet.create({
  // 44 px de alvo de toque sem empurrar o cartão (margem negativa; o Android aceita toque fora do pai com overflow visível).
  query: { color: C.text, fontFamily: F.xbold, fontSize: 16, padding: 0, minHeight: 44, marginVertical: -10, includeFontPadding: false },
  hit: { backgroundColor: 'rgba(255,79,216,0.45)', borderRadius: 3 },
});

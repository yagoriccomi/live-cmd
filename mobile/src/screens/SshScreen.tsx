import { useCallback, useState } from 'react';
import { ScrollView, Text, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { sshConnect, sshList, type SshServer } from '../extras.ts';
import { Dot, Glass, Header, Notice, OFFLINE, Pill, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, errText } from './theme.ts';

function latency(ms: number | null): { color: string; text: string } {
  if (ms === null) return { color: C.muted, text: 'latência desconhecida' };
  return { color: ms < 100 ? C.ok : ms < 300 ? C.warn : C.err, text: `${ms} ms` };
}

/** Servidores SSH salvos no PC; "Conectar" abre uma aba com ssh.exe no PC e a mostra aqui. */
export function SshScreen({ conn, onOpen }: { conn: Conn | null; onOpen: (sessionId: string) => Promise<void> }) {
  const load = useCallback(() => conn!.extra(sshList()).then(r => r.servers as SshServer[]), [conn]);
  const { data, err, busy, reload } = useRemote(conn ? load : null);
  const [acting, setActing] = useState('');
  const [msg, setMsg] = useState('');

  async function connect(x: SshServer) {
    setActing(x.serverId); setMsg('');
    try { const r = await conn!.extra(sshConnect(x.serverId, 80, 24)); await onOpen(r.sessionId); }
    catch (e) { setMsg(errText(e)); } finally { setActing(''); }
  }

  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }}>
      <Header title="SSH" subtitle="Servidores salvos no PC" onReload={conn ? reload : undefined} busy={busy} />
      {!conn && <Notice text={OFFLINE} />}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {conn && data?.length === 0 && <Notice text="Nenhum servidor salvo. Cadastre na tela SSH do PC." />}
      {data?.map(x => {
        const l = latency(x.latencyMs);
        return (
          <Glass key={x.serverId}>
            <View style={s.row}>
              <Dot color={l.color} label={l.text} />
              <View style={{ flex: 1 }}>
                <Text style={s.name} numberOfLines={1}>{x.label || x.host}</Text>
                <Text style={s.meta} numberOfLines={1}>{x.user}@{x.host}:{x.port} · {l.text}</Text>
              </View>
              <Pill title={acting === x.serverId ? 'Abrindo…' : 'Conectar'} primary disabled={!!acting} onPress={() => connect(x)}
                label={`Conectar em ${x.label || x.host}`} />
            </View>
          </Glass>
        );
      })}
    </ScrollView>
  );
}

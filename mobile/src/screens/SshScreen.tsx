import { useCallback, useEffect, useRef, useState } from 'react';
import { ScrollView, Text, TextInput, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { SSH_MSG, sshConnect, sshFormError, sshList, sshProbe, sshReconnect, sshRemove, sshSave, type SshServer, type SshSession, type SshStatus } from '../extras.ts';
import { Dot, Field, Glass, Header, Notice, OFFLINE, OLD_HOST, Pill, confirm, s, useRemote } from './common.tsx';
import { C, TAB_SPACE, errText } from './theme.ts';

/** Mesmos estados da tela desktop (ssh.js `statusText`). */
function status(st: SshStatus, ms: number | null): { color: string; text: string } {
  switch (st) {
    case 'ok': return { color: ms !== null && ms >= 300 ? C.warn : C.ok, text: ms === null ? 'alcançável' : `${ms} ms` };
    case 'testing': return { color: C.warn, text: 'testando…' };
    case 'dns': return { color: C.err, text: 'host não encontrado' };
    case 'timeout': return { color: C.err, text: 'sem resposta' };
    case 'offline': return { color: C.err, text: 'offline' };
    default: return { color: C.muted, text: 'não testado' };
  }
}
const sshErr = (e: any) => SSH_MSG[e?.code ?? e?.message] ?? errText(e);

type Form = { serverId?: string; label: string; user: string; host: string; port: string; key: string };
const EMPTY: Form = { label: '', user: '', host: '', port: '22', key: '' };

/** Servidores SSH do PC com as mesmas ações da tela desktop (ssh.js): buscar, conectar, testar, adicionar, editar,
 *  remover, e as sessões SSH abertas no PC (as restauradas voltam desconectadas, com Reconectar). Nunca pede senha. */
export function SshScreen({ conn, onOpen }: { conn: Conn | null; onOpen: (sessionId: string) => Promise<void> }) {
  const ok = !!conn && conn.caps.includes('ssh');
  const load = useCallback(() => conn!.extra(sshList()).then(r => ({ servers: r.servers as SshServer[], sessions: r.sessions as SshSession[] })), [conn]);
  const { data, err, busy, reload } = useRemote(ok ? load : null);
  const [acting, setActing] = useState('');
  const [msg, setMsg] = useState('');
  const [q, setQ] = useState('');
  const [form, setForm] = useState<Form | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => { if (timer.current) clearTimeout(timer.current); }, []);

  async function act(id: string, f: () => Promise<void>) {
    setActing(id); setMsg('');
    try { await f(); } catch (e) { setMsg(sshErr(e)); } finally { setActing(''); }
  }
  const connect = (x: SshServer) => act(x.serverId, async () => { const r = await conn!.extra(sshConnect(x.serverId, 80, 24)); await onOpen(r.sessionId); });
  const reconnect = (x: SshSession) => act(x.sessionId, async () => { const r = await conn!.extra(sshReconnect(x.sessionId, 80, 24)); await onOpen(r.sessionId); });
  // O teste roda no PC em segundo plano: recarrega logo depois para ver o resultado.
  const probe = (x: SshServer) => act(x.serverId, async () => {
    await conn!.extra(sshProbe(x.serverId));
    reload();
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(reload, 1600);
  });
  async function remove(x: SshServer) {
    if (!await confirm('Remover servidor?', `“${x.label}” (${x.user}@${x.host}) sai da lista. Abas já abertas continuam.`, 'Remover')) return;
    await act(x.serverId, async () => { await conn!.extra(sshRemove(x.serverId)); reload(); });
  }
  async function save() {
    if (!form) return;
    const f = { serverId: form.serverId, label: form.label.trim(), user: form.user.trim(), host: form.host.trim(), port: Number(form.port), key: form.key.trim() || null };
    const bad = sshFormError(f);
    if (bad) return setMsg(SSH_MSG[bad]);
    await act('form', async () => { await conn!.extra(sshSave(f)); setForm(null); reload(); });
  }

  const shown = (data?.servers ?? []).filter(x => !q.trim() || `${x.label} ${x.user}@${x.host}:${x.port}`.toLowerCase().includes(q.trim().toLowerCase()));
  return (
    <ScrollView contentContainerStyle={{ padding: 16, paddingTop: 40, paddingBottom: TAB_SPACE }} keyboardShouldPersistTaps="handled">
      <Header title="SSH" subtitle="Servidores salvos no PC" onReload={ok ? reload : undefined} busy={busy} />
      {!conn && <Notice text={OFFLINE} />}
      {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
      {ok && !form && (
        <>
          <TextInput value={q} onChangeText={setQ} placeholder="Buscar por nome, usuário ou host" placeholderTextColor={C.muted}
            autoCapitalize="none" autoCorrect={false} accessibilityLabel="Buscar servidor" style={[s.input, { marginBottom: 10 }]} />
          <View style={[s.wrap, { justifyContent: 'flex-end' }]}><Pill title="+ Adicionar servidor" primary onPress={() => { setMsg(''); setForm(EMPTY); }} /></View>
        </>
      )}
      {ok && form && (
        <Glass>
          <Text style={s.name} accessibilityRole="header">{form.serverId ? 'Editar servidor' : 'Adicionar servidor'}</Text>
          <Field label="Nome" value={form.label} maxLength={40} onChangeText={label => setForm({ ...form, label })} placeholder="Produção" autoCapitalize="sentences" />
          <Field label="Usuário" value={form.user} maxLength={64} onChangeText={user => setForm({ ...form, user })} placeholder="deploy" />
          <Field label="Host" value={form.host} maxLength={253} onChangeText={host => setForm({ ...form, host })} placeholder="srv.exemplo.com" />
          <Field label="Porta" value={form.port} keyboardType="number-pad" maxLength={5} onChangeText={port => setForm({ ...form, port })} />
          <Field label="Chave privada no PC (opcional)" value={form.key} onChangeText={key => setForm({ ...form, key })} placeholder="C:\Users\voce\.ssh\id_ed25519" />
          <Text style={s.meta}>A senha é pedida no terminal e nunca é salva.</Text>
          <View style={[s.wrap, { justifyContent: 'flex-end', marginTop: 10 }]}>
            <Pill title="Cancelar" onPress={() => { setForm(null); setMsg(''); }} />
            <Pill title={acting === 'form' ? 'Salvando…' : 'Salvar'} primary disabled={!!acting} onPress={save} />
          </View>
        </Glass>
      )}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {ok && data && !form && shown.length === 0 && <Notice text={data.servers.length ? 'Nenhum servidor encontrado para essa busca.' : 'Nenhum servidor salvo. Toque em “+ Adicionar servidor”.'} />}
      {ok && !form && shown.map(x => {
        const l = status(x.status, x.latencyMs);
        return (
          <Glass key={x.serverId}>
            <View style={s.row}>
              <Dot color={l.color} label={l.text} />
              <View style={{ flex: 1 }}>
                <Text style={s.name} numberOfLines={1}>{x.label || x.host}</Text>
                <Text style={s.meta} numberOfLines={1}>{x.user}@{x.host}:{x.port}{x.key ? ' · chave' : ''} · {l.text}</Text>
              </View>
            </View>
            <View style={[s.wrap, { justifyContent: 'flex-end', marginTop: 8, marginBottom: 0 }]}>
              <Pill title="Remover" disabled={!!acting} onPress={() => remove(x)} label={`Remover ${x.label}`} />
              <Pill title="Editar" disabled={!!acting} onPress={() => { setMsg(''); setForm({ serverId: x.serverId, label: x.label, user: x.user, host: x.host, port: String(x.port), key: x.key ?? '' }); }} label={`Editar ${x.label}`} />
              <Pill title="Testar" disabled={!!acting} onPress={() => probe(x)} label={`Testar alcance de ${x.label}`} />
              <Pill title={acting === x.serverId ? 'Abrindo…' : 'Conectar'} primary disabled={!!acting} onPress={() => connect(x)} label={`Conectar a ${x.label || x.host}`} />
            </View>
          </Glass>
        );
      })}
      {ok && !form && !!data?.sessions.length && (
        <View>
          <Text style={s.group} accessibilityRole="header">Sessões SSH no PC</Text>
          {data.sessions.map(x => (
            <Glass key={x.sessionId}>
              <View style={s.row}>
                <Dot color={x.connected ? C.ok : C.warn} label={x.connected ? 'conectada' : 'desconectada'} />
                <View style={{ flex: 1 }}>
                  <Text style={s.name} numberOfLines={1}>{x.label || 'SSH'}</Text>
                  <Text style={s.meta}>{x.connected ? 'conectada' : 'desconectada'}</Text>
                </View>
                {x.connected
                  ? <Pill title="Abrir" disabled={!!acting} onPress={() => act(x.sessionId, () => onOpen(x.sessionId))} label={`Abrir ${x.label}`} />
                  : <Pill title={acting === x.sessionId ? 'Abrindo…' : 'Reconectar'} primary disabled={!!acting} onPress={() => reconnect(x)} label={`Reconectar ${x.label}`} />}
              </View>
            </Glass>
          ))}
        </View>
      )}
    </ScrollView>
  );
}

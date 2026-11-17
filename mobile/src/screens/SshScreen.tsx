import { useCallback, useEffect, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { SSH_MSG, baseName, sshConnect, sshFormError, sshList, sshProbe, sshReconnect, sshRemove, sshSave, type SshServer, type SshSession, type SshStatus } from '../extras.ts';
import { Field, Notice, OFFLINE, OLD_HOST, confirm, s, useRemote } from './common.tsx';
import { Hero, HeroBtn, IconBtn, ListPanel, Panel, Row, Section, SettingRow, TopBar, screen } from './aurora.tsx';
import { C, F, TAB_SPACE, errText } from './theme.ts';

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
  // Servidor em destaque no cartão do topo e no painel de detalhes (mockup VM-MSSH): o tocado na lista, ou o primeiro.
  const [selId, setSelId] = useState<string | null>(null);
  const sel = shown.find(x => x.serverId === selId) ?? shown[0];
  const selSt = sel ? status(sel.status, sel.latencyMs) : null;
  const add = () => { setMsg(''); setForm(EMPTY); };
  const edit = (x: SshServer) => { setMsg(''); setForm({ serverId: x.serverId, label: x.label, user: x.user, host: x.host, port: String(x.port), key: x.key ?? '' }); };
  return (
    <ScrollView contentContainerStyle={[screen, { flexGrow: 1, paddingBottom: TAB_SPACE }]} keyboardShouldPersistTaps="handled">
      <TopBar title="Servidores" label="Adicionar servidor" onPress={add} disabled={!ok || !!form} />
      {!form && (sel && selSt
        ? (
          <Hero icon="⇄" title={sel.label || sel.host} sub={`${sel.user}@${sel.host}${sel.port !== 22 ? `:${sel.port}` : ''}`}
            badge={{ text: selSt.text.toUpperCase(), color: selSt.color === C.muted ? 'rgba(255,255,255,0.7)' : selSt.color }}>
            <HeroBtn title={acting === sel.serverId ? 'Abrindo…' : 'Conectar'} primary disabled={!!acting} onPress={() => connect(sel)} label={`Conectar a ${sel.label || sel.host}`} />
            <HeroBtn title="Editar" disabled={!!acting} onPress={() => edit(sel)} label={`Editar ${sel.label}`} />
          </Hero>
        )
        : (
          <Hero icon="⇄" title="Servidores SSH do PC" sub={!conn ? 'sem conexão com o PC' : !ok ? 'atualize o live-cmd no PC' : busy ? 'carregando…' : 'nenhum servidor'}>
            {ok && <HeroBtn title="+ Adicionar servidor" primary onPress={add} />}
            {ok && <HeroBtn title={busy ? 'Atualizando…' : 'Atualizar'} disabled={busy} onPress={reload} label="Atualizar Servidores" />}
          </Hero>
        ))}
      {!conn && <Notice text={OFFLINE} />}
      {conn && !ok && <Notice text={OLD_HOST} color={C.warn} />}
      {ok && form && (
        <Panel title={form.serverId ? 'Editar servidor' : 'Adicionar servidor'}>
          <Field label="Nome" value={form.label} maxLength={40} onChangeText={label => setForm({ ...form, label })} placeholder="Produção" autoCapitalize="sentences" />
          <Field label="Usuário" value={form.user} maxLength={64} onChangeText={user => setForm({ ...form, user })} placeholder="deploy" />
          <Field label="Host" value={form.host} maxLength={253} onChangeText={host => setForm({ ...form, host })} placeholder="srv.exemplo.com" />
          <Field label="Porta" value={form.port} keyboardType="number-pad" maxLength={5} onChangeText={port => setForm({ ...form, port })} />
          <Field label="Chave privada no PC (opcional)" value={form.key} onChangeText={key => setForm({ ...form, key })} placeholder="C:\Users\voce\.ssh\id_ed25519" />
          <Text style={s.meta}>A senha é pedida no terminal e nunca é salva.</Text>
          <View style={[s.wrap, { justifyContent: 'flex-end', marginTop: 10, marginBottom: 0 }]}>
            <HeroBtn title="Cancelar" onPress={() => { setForm(null); setMsg(''); }} />
            <HeroBtn title={acting === 'form' ? 'Salvando…' : 'Salvar'} primary disabled={!!acting} onPress={save} />
          </View>
        </Panel>
      )}
      {!!err && <Notice text={err} color={C.err} />}
      {!!msg && <Notice text={msg} color={C.err} />}
      {ok && !form && !!data?.servers.length && (
        <>
          <Section text="Servidores" />
          <ListPanel>
            <View style={st.search}>
              <TextInput value={q} onChangeText={setQ} placeholder="Buscar por nome, usuário ou host" placeholderTextColor={C.muted}
                autoCapitalize="none" autoCorrect={false} accessibilityLabel="Buscar servidor" style={st.searchInput} />
              <IconBtn glyph="↻" label="Atualizar Servidores" disabled={busy} onPress={reload} />
            </View>
            {shown.map(x => {
              const l = status(x.status, x.latencyMs);
              return (
                <Row key={x.serverId} icon="⇄" title={x.label || x.host} sub={`${x.user}@${x.host}${x.port !== 22 ? `:${x.port}` : ''}${x.key ? ' · chave' : ''}`}
                  status={{ color: l.color, text: l.text }} selected={x.serverId === sel?.serverId} onPress={() => setSelId(x.serverId)}
                  label={`${x.label || x.host}, ${x.user}@${x.host}:${x.port}, ${l.text}. Mostrar detalhes`} />
              );
            })}
          </ListPanel>
        </>
      )}
      {ok && data && !form && shown.length === 0 && <Notice text={data.servers.length ? 'Nenhum servidor encontrado para essa busca.' : 'Nenhum servidor salvo. Toque em “+ Adicionar servidor”.'} />}
      {ok && !form && sel && selSt && (
        <Panel title={sel.label || sel.host} style={{ flexGrow: 1 }}><View>
          <SettingRow title="Testar alcance" value={selSt.text} onPress={() => probe(sel)} disabled={!!acting} label={`Testar alcance de ${sel.label}`} />
          <SettingRow title="Porta" value={String(sel.port)} />
          <SettingRow title="Chave" value={sel.key ? baseName(sel.key) : 'senha no terminal'} />
          <SettingRow title="Excluir servidor" danger onPress={() => remove(sel)} disabled={!!acting} label={`Remover ${sel.label}`} last />
        </View>
        </Panel>
      )}
      {ok && !form && !!data?.sessions.length && (
        <>
          <Section text="Sessões SSH no PC" />
          <ListPanel>
            {data.sessions.map(x => (
              <Row key={x.sessionId} icon="›_" title={x.label || 'SSH'} sub={x.connected ? 'conectada' : 'desconectada'}
                status={x.connected ? { color: C.ok, text: 'ativa' } : { color: C.warn, text: acting === x.sessionId ? 'abrindo…' : 'reconectar' }}
                disabled={!!acting} onPress={() => x.connected ? act(x.sessionId, () => onOpen(x.sessionId)) : reconnect(x)}
                label={`${x.connected ? 'Abrir' : 'Reconectar'} ${x.label}`} />
            ))}
          </ListPanel>
        </>
      )}
    </ScrollView>
  );
}

const st = StyleSheet.create({
  search: { flexDirection: 'row', alignItems: 'center', paddingLeft: 12, borderRadius: 18, backgroundColor: 'rgba(255,255,255,0.05)' },
  searchInput: { flex: 1, minHeight: 44, color: C.text, fontFamily: F.regular, fontSize: 12, padding: 0 },
});

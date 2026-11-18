import { useCallback, useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import type { Conn, Pin } from '../conn.ts';
import { sessionList } from '../extras.ts';
import { KEY_SEQ, ctrl, type Key } from '../keys.ts';
import { useRemote } from './common.tsx';
import { Chip, Hero, HeroBtn, ListPanel, Panel, Row, Section, TopBar, screen } from './aurora.tsx';
import { C, F, TAB_SPACE, errText } from './theme.ts';

export interface SessionsProps {
  status: string; on: boolean; pin: Pin | null; out: string;
  link: string; setLink: (s: string) => void;
  onPair: () => void; onScan: () => void; onReconnect: () => void; onDisconnect: () => void;
  send: (s: string) => void;
  /** Conexão autenticada (lista "Abas" e troca de sessão); null enquanto desconectado. */
  conn?: Conn | null; onOpen?: (sessionId: string) => Promise<void>;
  /** Abre o Histórico (botão do título). */
  onHistory?: () => void;
  /** Tamanho da fonte do terminal (Ajustes). */
  termFont?: number;
}
type HostSession = { session_id: string; label: string; state: 'running' | 'exited'; last_activity_ms: number | string };

const KEYS: { k: Key; t: string; l: string }[] = [
  { k: 'esc', t: 'Esc', l: 'Enviar Esc' }, { k: 'tab', t: 'Tab', l: 'Enviar Tab' },
];

/** Terminal da sessão aberta no PC, abas abertas no host, mais pareamento quando desconectado. */
export function SessionsScreen(p: SessionsProps) {
  const [line, setLine] = useState('');
  const [ctrlOn, setCtrlOn] = useState(false);
  const [msg, setMsg] = useState('');
  const scroll = useRef<any>(null);
  const submit = () => { p.send(line + '\r'); setLine(''); };
  // Ctrl fica preso até a próxima letra digitada: ela vai como byte de controle (Ctrl+C = \x03), sem entrar no campo.
  const type = (v: string) => {
    if (ctrlOn && v.length === line.length + 1 && v.startsWith(line)) {
      const c = ctrl(v.slice(-1));
      setCtrlOn(false);
      if (c) { p.send(c); return; }
    }
    setLine(v);
  };
  const ok = p.status === 'conectado';
  const conn = ok ? p.conn ?? null : null;
  const load = useCallback(() => conn!.extra(sessionList()).then(r => r.sessions as HostSession[]), [conn]);
  const { data, reload } = useRemote(conn ? load : null);
  const current = conn?.sessionId;
  const open = async (id: string) => {
    setMsg('');
    try { await p.onOpen?.(id); reload(); } catch (e) { setMsg(errText(e)); }
  };
  const badge = ok ? { text: 'AO VIVO', color: C.ok } : p.on ? { text: 'CONECTANDO', color: C.warn } : { text: 'OFFLINE', color: C.err };
  const here = data?.find(s => s.session_id === current);
  return (
    <View style={[screen, { flex: 1, paddingBottom: TAB_SPACE }]}>
      <TopBar title={p.on ? 'Sessões' : 'Parear PC'} glyph="↺" label="Abrir Histórico" onPress={p.onHistory} />
      <Hero icon={p.on ? '›_' : '▣'} title={ok ? here?.label || 'PC conectado' : p.on ? 'Conectando ao PC…' : 'Conectar ao PC'}
        sub={p.status} subLive subLabel={`Estado: ${p.status}`} badge={badge}>
        {p.on && <HeroBtn title="Desconectar" primary onPress={p.onDisconnect} />}
        {!p.on && <HeroBtn title="Ler QR code" primary onPress={p.onScan} />}
        {!p.on && p.pin && <HeroBtn title="Conectar ao PC" onPress={p.onReconnect} label="Conectar ao PC pareado" />}
      </Hero>
      {!p.on && (
        <>
          <Section text="Link de pareamento" />
          <ListPanel>
            <View style={st.linkRow}>
              <TextInput style={st.link} value={p.link} onChangeText={p.setLink} placeholder="Cole o link remoteterm://pair?..." placeholderTextColor={C.muted}
                autoCapitalize="none" autoCorrect={false} accessibilityLabel="Link de pareamento" />
              <HeroBtn title="Parear" primary onPress={p.onPair} disabled={!p.link} label="Parear com o link colado" />
            </View>
          </ListPanel>
        </>
      )}
      {ok && !!data?.length && (
        <>
          <Section text="Abas" />
          <ListPanel style={{ maxHeight: 132 }}>
            <ScrollView nestedScrollEnabled contentContainerStyle={{ gap: 4 }}>
              {data.map(s => {
                const me = s.session_id === current;
                const dead = s.state === 'exited';
                return (
                  <Row key={s.session_id} icon={/claude|codex|agente/i.test(s.label) ? '✦' : '›_'} title={s.label || 'Terminal'}
                    sub={dead ? 'encerrada' : me ? 'exibida aqui' : 'aberta no PC'} selected={me}
                    status={me ? { color: C.ok, text: 'ativo' } : dead ? { color: C.err, text: 'fim' } : null}
                    onPress={me ? undefined : () => open(s.session_id)} label={me ? `${s.label}, exibida aqui` : `Mostrar ${s.label}`} />
                );
              })}
            </ScrollView>
          </ListPanel>
        </>
      )}
      {!!msg && <Text style={st.msg} accessibilityLiveRegion="polite">{msg}</Text>}
      <Panel style={{ flex: 1 }}>
        <ScrollView ref={scroll} style={{ flex: 1 }} onContentSizeChange={() => scroll.current?.scrollToEnd({ animated: false })}
          accessibilityLabel="Saída do terminal">
          <Text style={[st.mono, p.termFont ? { fontSize: p.termFont, lineHeight: Math.round(p.termFont * 1.65) } : null]} selectable>{p.out}</Text>
        </ScrollView>
        <View style={st.keys}>
          <TextInput style={st.cmd} value={line} onChangeText={type} autoCapitalize="none" autoCorrect={false}
            placeholder={ctrlOn ? 'Ctrl + letra' : 'comando'} placeholderTextColor={C.dim} onSubmitEditing={submit} blurOnSubmit={false} editable={p.on}
            accessibilityLabel="Comando para enviar ao terminal" />
          <Chip title="Enviar" onPress={submit} disabled={!p.on} label="Enviar comando" style={{ flex: 1 }} />
        </View>
        <View style={st.keys} accessibilityLabel="Teclas do terminal">
          {KEYS.map(x => <Chip key={x.k} title={x.t} onPress={() => p.send(KEY_SEQ[x.k])} disabled={!p.on} label={x.l} style={{ flex: 1 }} />)}
          <Chip title="Ctrl" on={ctrlOn} onPress={() => setCtrlOn(!ctrlOn)} disabled={!p.on} label={`Ctrl${ctrlOn ? ', ligado: digite a letra' : ''}`} style={{ flex: 1 }} />
          <Chip title="↑" onPress={() => p.send(KEY_SEQ.up)} disabled={!p.on} label="Seta para cima" style={{ flex: 1 }} />
          <Chip title="↓" onPress={() => p.send(KEY_SEQ.down)} disabled={!p.on} label="Seta para baixo" style={{ flex: 1 }} />
          <Chip title="^C" onPress={() => p.send('\x03')} disabled={!p.on} label="Enviar Ctrl+C" style={{ flex: 1 }} />
        </View>
      </Panel>
    </View>
  );
}

const st = StyleSheet.create({
  linkRow: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 4, paddingLeft: 12, paddingRight: 6, borderRadius: 18, backgroundColor: 'rgba(255,255,255,0.05)' },
  link: { flex: 1, minHeight: 44, color: C.text, fontFamily: F.mono, fontSize: 11, padding: 0 },
  mono: { fontFamily: F.mono, color: '#e6dcff', fontSize: 11, lineHeight: 18 },
  keys: { flexDirection: 'row', gap: 5, paddingTop: 6 },
  cmd: { flex: 3, minHeight: 44, borderRadius: 9, backgroundColor: 'rgba(255,255,255,0.12)', color: C.text, fontFamily: F.mono, fontSize: 11, paddingHorizontal: 10, paddingVertical: 0 },
  msg: { color: C.err, fontFamily: F.regular, fontSize: 12, textAlign: 'center' },
});

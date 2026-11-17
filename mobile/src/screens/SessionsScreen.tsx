import { useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import type { Pin } from '../conn.ts';
import { Chip, Hero, HeroBtn, ListPanel, Panel, Section, TopBar, screen } from './aurora.tsx';
import { C, F, TAB_SPACE } from './theme.ts';

export interface SessionsProps {
  status: string; on: boolean; pin: Pin | null; out: string;
  link: string; setLink: (s: string) => void;
  onPair: () => void; onScan: () => void; onReconnect: () => void; onDisconnect: () => void;
  send: (s: string) => void;
}

/** Terminal da sessão aberta no PC, mais pareamento quando desconectado. */
export function SessionsScreen(p: SessionsProps) {
  const [line, setLine] = useState('');
  const scroll = useRef<any>(null);
  const submit = () => { p.send(line + '\r'); setLine(''); };
  const ok = p.status === 'conectado';
  const badge = ok ? { text: 'AO VIVO', color: C.ok } : p.on ? { text: 'CONECTANDO', color: C.warn } : { text: 'OFFLINE', color: C.err };
  return (
    <View style={[screen, { flex: 1, paddingBottom: TAB_SPACE }]}>
      <TopBar title={p.on ? 'Sessões' : 'Parear PC'} />
      <Hero icon={p.on ? '›_' : '▣'} title={ok ? 'PC conectado' : p.on ? 'Conectando ao PC…' : 'Conectar ao PC'}
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
      <Panel style={{ flex: 1 }}>
        <ScrollView ref={scroll} style={{ flex: 1 }} onContentSizeChange={() => scroll.current?.scrollToEnd({ animated: false })}
          accessibilityLabel="Saída do terminal">
          <Text style={st.mono} selectable>{p.out}</Text>
        </ScrollView>
        <View style={st.keys}>
          <TextInput style={st.cmd} value={line} onChangeText={setLine} autoCapitalize="none" autoCorrect={false}
            placeholder="comando" placeholderTextColor={C.dim} onSubmitEditing={submit} blurOnSubmit={false} editable={p.on}
            accessibilityLabel="Comando para enviar ao terminal" />
          <Chip title="Enviar" onPress={submit} disabled={!p.on} label="Enviar comando" style={{ flex: 1 }} />
          <Chip title="Ctrl+C" onPress={() => p.send('\x03')} disabled={!p.on} label="Enviar Ctrl+C" style={{ flex: 1 }} />
        </View>
      </Panel>
    </View>
  );
}

const st = StyleSheet.create({
  linkRow: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 4, paddingLeft: 12, paddingRight: 6, borderRadius: 18, backgroundColor: 'rgba(255,255,255,0.05)' },
  link: { flex: 1, minHeight: 44, color: C.text, fontFamily: F.mono, fontSize: 11, padding: 0 },
  mono: { fontFamily: F.mono, color: '#e6dcff', fontSize: 11, lineHeight: 18 },
  keys: { flexDirection: 'row', gap: 5, paddingTop: 8 },
  cmd: { flex: 3, minHeight: 44, borderRadius: 9, backgroundColor: 'rgba(255,255,255,0.12)', color: C.text, fontFamily: F.mono, fontSize: 11, paddingHorizontal: 10, paddingVertical: 0 },
});

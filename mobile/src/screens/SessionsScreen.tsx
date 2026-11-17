import { useRef, useState } from 'react';
import { ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import type { Pin } from '../conn.ts';
import { Pill } from './common.tsx';
import { C, TAB_SPACE } from './theme.ts';

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
  return (
    <View style={[st.root, { paddingBottom: TAB_SPACE }]}>
      <View style={st.top}>
        <View style={[st.dot, { backgroundColor: ok ? C.ok : p.on ? C.warn : C.err }]} />
        <Text style={st.status} accessibilityLiveRegion="polite" accessibilityLabel={`Estado: ${p.status}`}>{p.status}</Text>
        {p.on && <Pill title="Desconectar" onPress={p.onDisconnect} />}
      </View>
      {!p.on && (
        <View style={{ gap: 6 }}>
          <TextInput style={st.input} value={p.link} onChangeText={p.setLink} placeholder="Cole o link remoteterm://pair?..." placeholderTextColor={C.muted}
            autoCapitalize="none" autoCorrect={false} accessibilityLabel="Link de pareamento" />
          <View style={st.row}>
            <Pill title="Parear" primary onPress={p.onPair} disabled={!p.link} label="Parear com o link colado" />
            <Pill title="Ler QR code" onPress={p.onScan} />
            {p.pin && <Pill title="Conectar ao PC" onPress={p.onReconnect} label="Conectar ao PC pareado" />}
          </View>
        </View>
      )}
      <ScrollView ref={scroll} style={st.term} onContentSizeChange={() => scroll.current?.scrollToEnd({ animated: false })}
        accessibilityLabel="Saída do terminal">
        <Text style={st.mono} selectable>{p.out}</Text>
      </ScrollView>
      <View style={[st.row, { flexWrap: 'nowrap' }]}>
        <TextInput style={[st.input, { flex: 1 }]} value={line} onChangeText={setLine} autoCapitalize="none" autoCorrect={false}
          placeholder="comando" placeholderTextColor={C.muted} onSubmitEditing={submit} blurOnSubmit={false} editable={p.on}
          accessibilityLabel="Comando para enviar ao terminal" />
        <Pill title="Enviar" primary onPress={submit} disabled={!p.on} label="Enviar comando" />
        <Pill title="Ctrl+C" onPress={() => p.send('\x03')} disabled={!p.on} label="Enviar Ctrl+C" />
      </View>
    </View>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, padding: 8, paddingTop: 32 },
  top: { flexDirection: 'row', alignItems: 'center', gap: 8, marginBottom: 6 },
  dot: { width: 10, height: 10, borderRadius: 5 },
  status: { color: C.muted, flex: 1 },
  input: { borderWidth: 1, borderColor: C.border, borderRadius: 14, backgroundColor: C.glass, color: C.text, padding: 6, marginVertical: 4, minHeight: 40 },
  term: { flex: 1, backgroundColor: C.term, borderRadius: 14, marginVertical: 6 },
  mono: { fontFamily: 'monospace', color: '#e6dcff', fontSize: 12 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 4, flexWrap: 'wrap' },
});

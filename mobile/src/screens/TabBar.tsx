import { Pressable, StyleSheet, Text, View } from 'react-native';
import { C, F } from './theme.ts';

export type Tab = 'sessoes' | 'agentes' | 'ssh' | 'historico';
export const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: 'sessoes', label: 'Sessões', icon: '▣' },
  { id: 'agentes', label: 'Agentes', icon: '✦' },
  { id: 'ssh', label: 'SSH', icon: '⇄' },
  { id: 'historico', label: 'Histórico', icon: '↺' },
];

/** Barra de abas inferior flutuante (vidro), item ativo em branco. */
export function TabBar({ tab, onChange }: { tab: Tab; onChange: (t: Tab) => void }) {
  return (
    <View style={st.bar} accessibilityRole="tablist">
      {TABS.map(t => {
        const on = t.id === tab;
        return (
          <Pressable key={t.id} onPress={() => onChange(t.id)} style={[st.item, on && st.active]}
            accessibilityRole="tab" accessibilityLabel={t.label} accessibilityState={{ selected: on }}>
            <Text style={[st.label, on && st.activeText]} numberOfLines={1}>{t.label}</Text>
          </Pressable>
        );
      })}
    </View>
  );
}

const st = StyleSheet.create({
  // Mockup V1: barra de vidro de 62 px (+6 de padding e 1 de borda), 20 px das laterais e 26 px do fundo; item ativo branco de 50 px.
  bar: {
    marginHorizontal: 20, marginBottom: 26, height: 76, flexDirection: 'row', alignItems: 'center', padding: 6, gap: 4, borderRadius: 24,
    backgroundColor: C.glass, borderColor: C.border, borderWidth: 1,
    boxShadow: '0 0 28px rgba(168,85,247,0.35), inset 0 1px 0 rgba(255,255,255,0.22)',
  },
  item: { flex: 1, height: 50, borderRadius: 18, alignItems: 'center', justifyContent: 'center' },
  active: { backgroundColor: C.activeBg },
  label: { color: C.muted, fontFamily: F.regular, fontSize: 12, includeFontPadding: false },
  activeText: { color: C.activeText, fontFamily: F.xbold },
});

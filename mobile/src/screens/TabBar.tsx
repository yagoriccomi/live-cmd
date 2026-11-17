import { Pressable, StyleSheet, Text, View } from 'react-native';
import { C } from './theme.ts';

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
            <Text style={[st.icon, on && st.activeText]} importantForAccessibility="no">{t.icon}</Text>
            <Text style={[st.label, on && st.activeText]} numberOfLines={1}>{t.label}</Text>
          </Pressable>
        );
      })}
    </View>
  );
}

const st = StyleSheet.create({
  bar: {
    position: 'absolute', left: 16, right: 16, bottom: 16, flexDirection: 'row', padding: 6, gap: 4, borderRadius: 30,
    backgroundColor: 'rgba(40,20,80,0.82)', borderColor: C.border, borderWidth: 1,
    shadowColor: C.accent, shadowOpacity: 0.45, shadowRadius: 18, shadowOffset: { width: 0, height: 6 }, elevation: 12,
  },
  item: { flex: 1, minHeight: 52, borderRadius: 24, alignItems: 'center', justifyContent: 'center' },
  active: { backgroundColor: C.activeBg },
  icon: { color: C.muted, fontSize: 18 },
  label: { color: C.muted, fontSize: 11, fontWeight: '600', marginTop: 2 },
  activeText: { color: C.activeText },
});

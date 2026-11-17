import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, View, type StyleProp, type ViewStyle } from 'react-native';
import { C, errText } from './theme.ts';

export function Glass({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[s.glass, style]}>{children}</View>;
}

export function Header({ title, subtitle, onReload, busy }: { title: string; subtitle?: string; onReload?: () => void; busy?: boolean }) {
  return (
    <View style={s.header}>
      <View style={{ flex: 1 }}>
        <Text style={s.title} accessibilityRole="header">{title}</Text>
        {subtitle && <Text style={s.subtitle}>{subtitle}</Text>}
      </View>
      {onReload && (
        <Pressable onPress={onReload} disabled={busy} style={s.reload} accessibilityRole="button" accessibilityLabel={`Atualizar ${title}`} accessibilityState={{ busy: !!busy }}>
          {busy ? <ActivityIndicator color={C.text} /> : <Text style={s.reloadText}>↻</Text>}
        </Pressable>
      )}
    </View>
  );
}

export function Notice({ text, color = C.muted }: { text: string; color?: string }) {
  return <Text style={[s.notice, { color }]} accessibilityLiveRegion="polite">{text}</Text>;
}

export function Dot({ color, label }: { color: string; label: string }) {
  return <View style={[s.dot, { backgroundColor: color }]} accessible accessibilityLabel={label} />;
}

/** Botão pílula do tema; `primary` usa o acento. */
export function Pill({ title, onPress, disabled, primary, label }: { title: string; onPress: () => void; disabled?: boolean; primary?: boolean; label?: string }) {
  return (
    <Pressable onPress={onPress} disabled={disabled} accessibilityRole="button" accessibilityLabel={label ?? title} accessibilityState={{ disabled: !!disabled }}
      style={({ pressed }) => [s.pill, primary && s.pillPrimary, (disabled || pressed) && { opacity: disabled ? 0.4 : 0.7 }]}>
      <Text style={[s.pillText, primary && { color: '#fff' }]}>{title}</Text>
    </Pressable>
  );
}

/** Lista vinda do host: carrega quando há conexão, recarrega sob pedido. */
export function useRemote<T>(load: (() => Promise<T>) | null) {
  const [data, setData] = useState<T | null>(null);
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);
  const reload = useCallback(async () => {
    if (!load) return;
    setBusy(true); setErr('');
    try { setData(await load()); } catch (e) { setErr(errText(e)); } finally { setBusy(false); }
  }, [load]);
  useEffect(() => { if (load) reload(); else { setData(null); setErr(''); } }, [load, reload]);
  return { data, err, busy, reload };
}

export const OFFLINE = 'Conecte ao PC na aba Sessões para ver esta lista.';

export const s = StyleSheet.create({
  glass: { backgroundColor: C.glass, borderColor: C.border, borderWidth: 1, borderRadius: 18, padding: 12, marginBottom: 10 },
  header: { flexDirection: 'row', alignItems: 'center', marginBottom: 12 },
  title: { color: C.text, fontSize: 22, fontWeight: '700' },
  subtitle: { color: C.muted, fontSize: 13, marginTop: 2 },
  reload: { width: 44, height: 44, borderRadius: 22, alignItems: 'center', justifyContent: 'center', backgroundColor: C.glass, borderColor: C.border, borderWidth: 1 },
  reloadText: { color: C.text, fontSize: 20 },
  notice: { textAlign: 'center', marginVertical: 24, fontSize: 14 },
  dot: { width: 10, height: 10, borderRadius: 5 },
  pill: { minHeight: 40, paddingHorizontal: 16, borderRadius: 20, justifyContent: 'center', alignItems: 'center', backgroundColor: C.glass, borderColor: C.border, borderWidth: 1 },
  pillPrimary: { backgroundColor: C.accent, borderColor: C.accent2 },
  pillText: { color: C.text, fontWeight: '600' },
  row: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  name: { color: C.text, fontSize: 16, fontWeight: '600' },
  meta: { color: C.muted, fontSize: 12 },
  mono: { fontFamily: 'monospace', color: '#e6dcff', fontSize: 13 },
  group: { color: C.accent2, fontSize: 13, fontWeight: '700', marginTop: 6, marginBottom: 6, letterSpacing: 0.5 },
});

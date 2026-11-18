// Peças visuais do mockup V1 Glass Aurora (docs/design/aurora-v1/VM-*.dc.html), em 390×844:
// fundo com as manchas desfocadas, título + botão em degradê, cartão de destaque, rótulo de seção,
// painel de vidro com linhas e painel escuro. Só aparência: nenhuma regra de tela mora aqui.
import type { ReactNode } from 'react';
import { ActivityIndicator, Image, Pressable, StyleSheet, Text, View, type StyleProp, type ViewStyle } from 'react-native';
import { C, F } from './theme.ts';

const GRAD = `linear-gradient(135deg, ${C.accent}, ${C.accent2})`;
/** Alvo de toque mínimo de 44 px sem mudar o desenho de botões de 34 px. */
const SLOP = { top: 5, bottom: 5, left: 0, right: 0 };

/** As três manchas (violeta, rosa, azul) com blur de 100–110 px, pré-renderizadas: um webp de 14 KB em vez de lib nativa. */
export function AuroraBg() {
  return <Image source={require('../../assets/aurora.webp')} style={StyleSheet.absoluteFill} resizeMode="stretch" accessible={false} />;
}

/** Fundo + coluna da tela + barra de abas no rodapé (fora da área rolável, como no mockup). */
export function Shell({ children, bar }: { children: ReactNode; bar: ReactNode }) {
  return (
    <View style={a.shell}>
      <AuroraBg />
      <View style={{ flex: 1 }}>{children}</View>
      {bar}
    </View>
  );
}

/** Coluna padrão das telas: 54 px no topo, 20 px nas laterais, 12 px entre blocos. */
export const screen: ViewStyle = { paddingTop: 54, paddingHorizontal: 20, gap: 12 };

export function TopBar({ title, glyph, label, onPress, busy, disabled }: {
  title: string; glyph?: string; label?: string; onPress?: () => void; busy?: boolean; disabled?: boolean;
}) {
  return (
    <View style={a.top}>
      <Text style={a.title} accessibilityRole="header" numberOfLines={1}>{title}</Text>
      {onPress && (
        <Pressable onPress={onPress} disabled={disabled || busy} hitSlop={2} accessibilityRole="button" accessibilityLabel={label}
          accessibilityState={{ disabled: !!disabled, busy: !!busy }} style={({ pressed }) => [a.plus, (pressed || disabled) && { opacity: disabled ? 0.45 : 0.75 }]}>
          {busy ? <ActivityIndicator color={C.text} /> : <Text style={a.plusText}>{glyph ?? '+'}</Text>}
        </Pressable>
      )}
    </View>
  );
}

export type Badge = { text: string; color: string };

/** Cartão em degradê do topo: ícone, título, linha mono, selo e botões. */
export function Hero({ icon, title, sub, badge, children, subLive, subLabel, top }: {
  icon: string; title?: string; sub?: string; badge?: Badge | null; children?: ReactNode; subLive?: boolean; subLabel?: string; top?: ReactNode;
}) {
  return (
    <View style={a.hero}>
      <View style={a.heroRow}>
        <View style={a.heroIcon} importantForAccessibility="no-hide-descendants"><Text style={a.heroGlyph}>{icon}</Text></View>
        <View style={{ flex: 1, minWidth: 0 }}>
          {top ?? <Text style={a.heroTitle} numberOfLines={1} accessibilityRole="header">{title}</Text>}
          {!!sub && <Text style={a.heroSub} numberOfLines={1} accessibilityLiveRegion={subLive ? 'polite' : undefined} accessibilityLabel={subLabel}>{sub}</Text>}
        </View>
        {badge && <Text style={[a.badge, { backgroundColor: badge.color }]}>{badge.text}</Text>}
      </View>
      {children && <View style={a.heroBtns}>{children}</View>}
    </View>
  );
}

/** Botão do cartão de destaque: branco (principal) ou vidro. */
export function HeroBtn({ title, onPress, primary, disabled, label }: { title: string; onPress: () => void; primary?: boolean; disabled?: boolean; label?: string }) {
  return (
    <Pressable onPress={onPress} disabled={disabled} hitSlop={SLOP} accessibilityRole="button" accessibilityLabel={label ?? title} accessibilityState={{ disabled: !!disabled }}
      style={({ pressed }) => [a.hbtn, primary && a.hbtnOn, (pressed || disabled) && { opacity: disabled ? 0.45 : 0.75 }]}>
      <Text style={[a.hbtnText, primary && a.hbtnTextOn]}>{title}</Text>
    </Pressable>
  );
}

export function Section({ text }: { text: string }) {
  return <Text style={a.section} accessibilityRole="header" numberOfLines={1}>{text.toUpperCase()}</Text>;
}

/** Painel de vidro com linhas (lista curta). */
export function ListPanel({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[a.list, style]}>{children}</View>;
}

/** Linha do painel de vidro: ícone, título, linha mono e estado colorido; tocar faz a ação principal. */
export function Row({ icon, title, sub, status, onPress, label, disabled, right, selected }: {
  icon: string; title: string; sub?: string; status?: Badge | null; onPress?: () => void; label?: string; disabled?: boolean; right?: ReactNode; selected?: boolean;
}) {
  const body = (
    <>
      <View style={a.rowIcon} importantForAccessibility="no-hide-descendants"><Text style={a.rowGlyph}>{icon}</Text></View>
      <View style={{ flex: 1, minWidth: 0 }}>
        <Text style={a.rowTitle} numberOfLines={1}>{title}</Text>
        {!!sub && <Text style={a.rowSub} numberOfLines={1}>{sub}</Text>}
      </View>
      {status && <Text style={[a.rowStatus, { color: status.color }]} numberOfLines={1}>● {status.text}</Text>}
    </>
  );
  return (
    <View style={[a.row, selected && a.rowSel]}>
      {onPress
        ? <Pressable onPress={onPress} disabled={disabled} accessibilityRole="button" accessibilityLabel={label ?? title} accessibilityState={{ disabled: !!disabled, selected: !!selected }}
            style={({ pressed }) => [a.rowMain, pressed && { opacity: 0.75 }]}>{body}</Pressable>
        : <View style={a.rowMain} accessible accessibilityLabel={label}>{body}</View>}
      {right}
    </View>
  );
}

/** Botão só de ícone dentro de uma linha (44×44). */
export function IconBtn({ glyph, label, onPress, disabled, color }: { glyph: string; label: string; onPress: () => void; disabled?: boolean; color?: string }) {
  return (
    <Pressable onPress={onPress} disabled={disabled} accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled: !!disabled }}
      style={({ pressed }) => [a.ibtn, (pressed || disabled) && { opacity: disabled ? 0.4 : 0.7 }]}>
      <Text style={[a.ibtnText, color ? { color } : null]}>{glyph}</Text>
    </Pressable>
  );
}

/** Painel escuro que ocupa o resto da tela (terminal, linha do tempo, detalhes). */
export function Panel({ title, children, style, right }: { title?: string; children: ReactNode; style?: StyleProp<ViewStyle>; right?: ReactNode }) {
  return (
    <View style={[a.panel, style]}>
      {!!title && (
        <View style={a.panelHead}>
          <Text style={a.panelTitle} accessibilityRole="header" numberOfLines={1}>{title}</Text>
          {right}
        </View>
      )}
      {children}
    </View>
  );
}

/** Rótulo de grupo dentro do painel escuro (HOJE, ONTEM, nome da pasta). */
export function GroupLabel({ text }: { text: string }) {
  return <Text style={[a.section, { marginTop: 6 }]} accessibilityRole="header" numberOfLines={1}>{text.toUpperCase()}</Text>;
}

/** Linha de ajuste do painel escuro: rótulo à esquerda, valor + › à direita. */
export function SettingRow({ title, value, onPress, label, danger, disabled, last }: {
  title: string; value?: string; onPress?: () => void; label?: string; danger?: boolean; disabled?: boolean; last?: boolean;
}) {
  const body = (
    <>
      <Text style={[a.setTitle, danger && { color: C.err }]} numberOfLines={1}>{title}</Text>
      <Text style={a.setValue} numberOfLines={1}>{value ?? ''}{onPress ? ' ›' : ''}</Text>
    </>
  );
  const style = [a.setRow, !last && a.setLine];
  return onPress
    ? <Pressable onPress={onPress} disabled={disabled} accessibilityRole="button" accessibilityLabel={label ?? title} accessibilityState={{ disabled: !!disabled }}
        style={({ pressed }) => [...style, (pressed || disabled) && { opacity: disabled ? 0.45 : 0.75 }]}>{body}</Pressable>
    : <View style={style} accessible accessibilityLabel={`${title}: ${value ?? ''}`}>{body}</View>;
}

/** Linha de ajuste com chave liga/desliga (44 px de altura, chave de 44×26 em degradê quando ligada). */
export function ToggleRow({ title, value, onChange, disabled, last }: { title: string; value: boolean; onChange: (v: boolean) => void; disabled?: boolean; last?: boolean }) {
  return (
    <Pressable onPress={() => onChange(!value)} disabled={disabled} accessibilityRole="switch" accessibilityLabel={title} accessibilityState={{ checked: value, disabled: !!disabled }}
      style={({ pressed }) => [a.setRow, !last && a.setLine, (pressed || disabled) && { opacity: disabled ? 0.45 : 0.75 }]}>
      <Text style={a.setTitle} numberOfLines={1}>{title}</Text>
      <View style={[a.sw, value && a.swOn]}><View style={[a.knob, value && { alignSelf: 'flex-end' }]} /></View>
    </Pressable>
  );
}

/** Tecla/filtro pequeno (Esc, Tab, Últimos 7 dias); `on` fica branco. */
export function Chip({ title, onPress, on, disabled, label, style }: { title: string; onPress: () => void; on?: boolean; disabled?: boolean; label?: string; style?: StyleProp<ViewStyle> }) {
  return (
    <Pressable onPress={onPress} disabled={disabled} accessibilityRole="button" accessibilityLabel={label ?? title} accessibilityState={{ disabled: !!disabled, selected: on }}
      style={({ pressed }) => [a.chip, on && a.chipOn, style, (pressed || disabled) && { opacity: disabled ? 0.4 : 0.7 }]}>
      <Text style={[a.chipText, on && a.chipTextOn]} numberOfLines={1}>{title}</Text>
    </Pressable>
  );
}

const glassShadow = '0 0 28px rgba(168,85,247,0.35), inset 0 1px 0 rgba(255,255,255,0.22)';
export const a = StyleSheet.create({
  shell: { flex: 1, backgroundColor: C.bg },
  top: { flexDirection: 'row', alignItems: 'center', minHeight: 42 },
  title: { flex: 1, color: C.text, fontFamily: F.xbold, fontSize: 30, letterSpacing: -1, includeFontPadding: false },
  plus: { width: 42, height: 42, borderRadius: 14, alignItems: 'center', justifyContent: 'center', backgroundImage: GRAD, boxShadow: '0 0 22px #ff4fd899' },
  plusText: { color: C.text, fontFamily: F.regular, fontSize: 20, includeFontPadding: false },
  hero: {
    borderRadius: 24, padding: 16, gap: 10, borderWidth: 1, borderColor: 'rgba(255,255,255,0.45)',
    backgroundImage: 'linear-gradient(135deg, rgba(139,92,246,0.87), rgba(255,79,216,0.667))', boxShadow: '0 0 40px #ff4fd866',
  },
  heroRow: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  heroIcon: { width: 40, height: 40, borderRadius: 12, backgroundColor: 'rgba(255,255,255,0.13)', alignItems: 'center', justifyContent: 'center' },
  heroGlyph: { color: C.text, fontFamily: F.xbold, fontSize: 16, includeFontPadding: false },
  heroTitle: { color: C.text, fontFamily: F.xbold, fontSize: 16, includeFontPadding: false },
  heroSub: { color: C.text, opacity: 0.92, fontFamily: F.mono, fontSize: 12, marginTop: 2, includeFontPadding: false },
  badge: {
    paddingVertical: 4, paddingHorizontal: 9, borderRadius: 9, overflow: 'hidden', color: C.ink, fontFamily: F.xbold, fontSize: 10, letterSpacing: 0.5, includeFontPadding: false,
  },
  heroBtns: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  hbtn: { paddingVertical: 9, paddingHorizontal: 14, borderRadius: 12, backgroundColor: 'rgba(255,255,255,0.12)' },
  hbtnOn: { backgroundColor: '#fff' },
  hbtnText: { color: C.text, fontFamily: F.regular, fontSize: 13, includeFontPadding: false },
  hbtnTextOn: { color: C.activeText, fontFamily: F.xbold },
  section: { color: C.muted, fontFamily: F.semi, fontSize: 11, letterSpacing: 2, includeFontPadding: false },
  list: { backgroundColor: C.glass, borderWidth: 1, borderColor: C.border, borderRadius: 24, padding: 6, gap: 4, boxShadow: glassShadow },
  row: { flexDirection: 'row', alignItems: 'center', borderRadius: 18, backgroundColor: 'rgba(255,255,255,0.05)' },
  rowSel: { backgroundColor: 'rgba(255,255,255,0.12)' },
  rowMain: { flex: 1, minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 10, paddingHorizontal: 12 },
  rowIcon: { width: 34, height: 34, borderRadius: 10, backgroundColor: 'rgba(255,255,255,0.13)', alignItems: 'center', justifyContent: 'center' },
  rowGlyph: { color: C.text, fontFamily: F.xbold, fontSize: 14, includeFontPadding: false },
  rowTitle: { color: C.text, fontFamily: F.bold, fontSize: 13, includeFontPadding: false },
  rowSub: { color: C.muted, fontFamily: F.mono, fontSize: 11, marginTop: 1, includeFontPadding: false },
  rowStatus: { fontFamily: F.bold, fontSize: 11, includeFontPadding: false },
  ibtn: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  ibtnText: { color: C.muted, fontFamily: F.regular, fontSize: 15, includeFontPadding: false },
  panel: { backgroundColor: C.term, borderWidth: 1, borderColor: C.border, borderRadius: 24, paddingVertical: 12, paddingHorizontal: 14, gap: 8, boxShadow: '0 0 28px rgba(168,85,247,0.35)', overflow: 'hidden' },
  panelHead: { flexDirection: 'row', alignItems: 'center' },
  panelTitle: { flex: 1, color: C.text, fontFamily: F.bold, fontSize: 14, includeFontPadding: false },
  setRow: { minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 8 },
  setLine: { borderBottomWidth: 1, borderBottomColor: 'rgba(255,255,255,0.08)' },
  setTitle: { flex: 1, color: C.text, fontFamily: F.regular, fontSize: 13, includeFontPadding: false },
  setValue: { color: C.muted, fontFamily: F.regular, fontSize: 12, includeFontPadding: false },
  chip: { minHeight: 44, paddingHorizontal: 12, borderRadius: 9, backgroundColor: 'rgba(255,255,255,0.12)', alignItems: 'center', justifyContent: 'center' },
  chipOn: { backgroundColor: '#fff' },
  sw: { width: 44, height: 26, borderRadius: 13, padding: 3, backgroundColor: 'rgba(255,255,255,0.18)', justifyContent: 'center' },
  swOn: { backgroundImage: GRAD },
  knob: { width: 20, height: 20, borderRadius: 10, backgroundColor: '#fff' },
  chipText: { color: C.text, fontFamily: F.regular, fontSize: 11, includeFontPadding: false },
  chipTextOn: { color: C.activeText, fontFamily: F.xbold },
});

/** Linhas da linha do tempo do painel escuro (Histórico e "Retomar" em Agentes): hora, ponto, texto mono, rótulo. */
export const tl = StyleSheet.create({
  line: { flexDirection: 'row', alignItems: 'center' },
  main: { flex: 1, minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 10, paddingHorizontal: 4 },
  time: { width: 36, color: C.muted, fontFamily: F.regular, fontSize: 11, includeFontPadding: false },
  cmd: { flex: 1, color: C.text, fontFamily: F.mono, fontSize: 11, includeFontPadding: false },
  kind: { color: C.muted, fontFamily: F.regular, fontSize: 10, maxWidth: 110, includeFontPadding: false },
});

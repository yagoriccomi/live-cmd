import { useEffect, useState } from 'react';
import { ScrollView, View } from 'react-native';
import type { Conn, Pin } from '../conn.ts';
import { TERM_FONTS, type Prefs } from '../prefs.ts';
import { FONTS, THEMES, agentDefaultGet, settingsGet, settingsSet, type AgentDefault, type Appearance } from '../extras.ts';
import { Notice, confirm } from './common.tsx';
import { Hero, HeroBtn, ListPanel, Panel, Row, Section, SettingRow, ToggleRow, TopBar, screen } from './aurora.tsx';
import { C, TAB_SPACE, errText } from './theme.ts';

/** Nomes dos temas do PC como a tela Configurações do desktop os mostra. */
const THEME_NAME: Record<string, string> = { dark: 'Aurora', ametista: 'Ametista', magenta: 'Magenta', cosmo: 'Cosmo' };
/** Próximo valor da lista (volta ao começo): cada toque avança um passo, como o tamanho da fonte do terminal. */
const next = <T,>(list: readonly T[], v: T) => list[(list.indexOf(v) + 1) % list.length];
/** Próximo degrau acima do valor atual (valor fora dos degraus, como 82%, sobe para o seguinte). */
const up = (list: number[], v: number) => list.find(x => x > v) ?? list[0];
const steps = (lo: number, hi: number, by: number) => Array.from({ length: Math.floor((hi - lo) / by) + 1 }, (_, i) => lo + i * by);
const SIZES = steps(10, 24, 1), OPACITY = steps(60, 100, 5), BLUR = steps(0, 60, 10);

/** Ajustes (mockups VM-Ajustes-1 / G4-MobAjustes): aparência e comportamento locais do celular, e a seção
 *  das configurações do PC, que só liga quando o host anunciar a capacidade `settings` (settings_get/settings_set). */
export function SettingsScreen({ conn, pin, status, prefs, setPrefs, onOpenTerminals, onUnpair }: {
  conn: Conn | null; pin: Pin | null; status: string; prefs: Prefs; setPrefs: (p: Prefs) => void;
  onOpenTerminals: () => void; onUnpair: () => void;
}) {
  const online = status === 'conectado';
  const pcSettings = !!conn && conn.caps.includes('settings');
  const nextFont = () => setPrefs({ ...prefs, termFont: TERM_FONTS[(TERM_FONTS.indexOf(prefs.termFont) + 1) % TERM_FONTS.length] });
  async function unpair() {
    if (!await confirm('Desparear este PC?', 'O celular esquece o PC e a conexão atual é encerrada. Para voltar, leia o QR code de novo.', 'Desparear')) return;
    onUnpair();
  }
  const pcOff = !conn ? 'conecte ao PC' : 'atualize o PC';
  // Aparência do PC (cap `settings`): a mesma de Configurações no desktop; cada mudança grava no PC e volta inteira.
  const [pc, setPc] = useState<Appearance | null>(null);
  const [def, setDef] = useState<AgentDefault | null>(null);
  const [pcMsg, setPcMsg] = useState('');
  const [saving, setSaving] = useState(false);
  const canDefault = !!conn && conn.caps.includes('agent_default');
  useEffect(() => {
    setPc(null); setDef(null); setPcMsg('');
    if (pcSettings) conn!.extra(settingsGet()).then(r => setPc(r.settings), e => setPcMsg(errText(e)));
    if (canDefault) conn!.extra(agentDefaultGet()).then(r => setDef(r.value), () => {});
  }, [conn, pcSettings, canDefault]);
  async function change(p: Partial<Appearance>) {
    if (!pc || saving) return;
    setSaving(true); setPcMsg('');
    setPc({ ...pc, ...p });
    try { setPc((await conn!.extra(settingsSet(p))).settings); } catch (e) { setPcMsg(errText(e)); setPc(pc); } finally { setSaving(false); }
  }
  const off = !pc || saving;
  const v = (x: string) => pc ? x : pcSettings ? 'carregando…' : pcOff;
  return (
    <ScrollView contentContainerStyle={[screen, { flexGrow: 1, paddingBottom: TAB_SPACE }]}>
      <TopBar title="Ajustes" />
      <Hero icon="›_" title={pin ? 'PC pareado' : 'Nenhum PC pareado'} sub={pin ? `${status} · ${pin.server}` : 'leia o QR code na aba Sessões'}
        badge={pin ? (online ? { text: 'ONLINE', color: C.ok } : { text: 'OFFLINE', color: 'rgba(255,255,255,0.7)' }) : null}>
        <HeroBtn title={pin ? 'Abrir terminais' : 'Parear PC'} primary onPress={onOpenTerminals} />
      </Hero>
      <Section text="Aparelho" />
      <ListPanel>
        <Row icon="▣" title={pin ? 'PC pareado' : 'Sem PC'} sub={pin ? (online ? 'conectado' : status) : 'não pareado'}
          status={pin ? (online ? { color: C.ok, text: 'online' } : { color: C.warn, text: 'offline' }) : null} label={`PC: ${pin ? status : 'não pareado'}`} />
        <Row icon="✦" title="Este celular" sub="live-cmd mobile" label="Este celular" />
      </ListPanel>
      <Panel title="Aparência" style={{ flexGrow: 1 }}>
        <View>
          <SettingRow title="Tema" value="Aurora" />
          <SettingRow title="Fonte do terminal" value={`Fira Code ${prefs.termFont}`} onPress={nextFont} label={`Fonte do terminal: Fira Code ${prefs.termFont}. Trocar tamanho`} />
          <ToggleRow title="Vibrar ao concluir" value={prefs.vibrate} onChange={vibrate => setPrefs({ ...prefs, vibrate })} />
          <SettingRow title="Desparear este PC" danger onPress={unpair} disabled={!pin} label="Desparear este PC" last />
        </View>
      </Panel>
      <Panel title="No PC">
        <View>
          <SettingRow title="Tema" value={v(THEME_NAME[pc?.theme ?? ''] ?? pc?.theme ?? '')} disabled={off} onPress={() => pc && change({ theme: next(THEMES, pc.theme as any) })} label="Tema do PC. Trocar" />
          <SettingRow title="Fonte" value={v(pc?.fontFamily ?? '')} disabled={off} onPress={() => pc && change({ fontFamily: next(FONTS, pc.fontFamily as any) })} label="Fonte do terminal no PC. Trocar" />
          <SettingRow title="Tamanho" value={v(`${pc?.fontSize} px`)} disabled={off} onPress={() => pc && change({ fontSize: up(SIZES, pc.fontSize) })} label="Tamanho da fonte no PC. Aumentar" />
          <SettingRow title="Opacidade" value={v(`${pc?.opacity}%`)} disabled={off} onPress={() => pc && change({ opacity: up(OPACITY, pc.opacity) })} label="Opacidade da janela no PC. Trocar" />
          <SettingRow title="Desfoque" value={v(`${pc?.blur} px`)} disabled={off} onPress={() => pc && change({ blur: up(BLUR, pc.blur) })} label="Desfoque do fundo no PC. Trocar" />
          <ToggleRow title="Brilho" value={!!pc?.glow} disabled={off} onChange={glow => change({ glow })} />
          <ToggleRow title="Animações" value={!!pc?.animations} disabled={off} onChange={animations => change({ animations })} last={!canDefault} />
          {canDefault && <SettingRow title="Agente padrão" value={def ? [def.agent, def.args].filter(Boolean).join(' ') : 'nenhum'} last />}
        </View>
        {!!pcMsg && <Notice text={pcMsg} color={C.err} />}
      </Panel>
    </ScrollView>
  );
}

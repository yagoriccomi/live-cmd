import { ScrollView, View } from 'react-native';
import type { Conn, Pin } from '../conn.ts';
import { TERM_FONTS, type Prefs } from '../prefs.ts';
import { confirm } from './common.tsx';
import { Hero, HeroBtn, ListPanel, Panel, Row, Section, SettingRow, ToggleRow, TopBar, screen } from './aurora.tsx';
import { C, TAB_SPACE } from './theme.ts';

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
          {/* Liga quando o host tiver settings_get/settings_set (branch do desktop); até lá fica só leitura. */}
          <SettingRow title="Agente padrão" value={pcSettings ? 'claude' : pcOff} disabled={!pcSettings} />
          <ToggleRow title="Retomar com --resume" value={false} onChange={() => {}} disabled={!pcSettings} last />
        </View>
      </Panel>
    </ScrollView>
  );
}

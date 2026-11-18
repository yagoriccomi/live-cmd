import { useEffect, useRef, useState } from 'react';
import { Linking, SafeAreaView, StyleSheet, Vibration, View } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import { getRandomBytes } from 'expo-crypto';
import { CameraView, useCameraPermissions } from 'expo-camera';
import { useFonts } from 'expo-font';
import { StatusBar } from 'expo-status-bar';
import { b64d, b64e, parseQr } from './src/auth.ts';
import { Conn, type Pin } from './src/conn.ts';
import { Pill } from './src/screens/common.tsx';
import { TabBar, type Tab } from './src/screens/TabBar.tsx';
import { SessionsScreen } from './src/screens/SessionsScreen.tsx';
import { AgentsScreen } from './src/screens/AgentsScreen.tsx';
import { SshScreen } from './src/screens/SshScreen.tsx';
import { HistoryScreen } from './src/screens/HistoryScreen.tsx';
import { SettingsScreen } from './src/screens/SettingsScreen.tsx';
import { DEFAULT_PREFS, loadPrefs, savePrefs, type Prefs } from './src/prefs.ts';
import { C, errText } from './src/screens/theme.ts';
import { Banner, Shell } from './src/screens/aurora.tsx';
import { agentList } from './src/extras.ts';
import { longRun } from './src/done.ts';
import { FONTS } from './src/screens/fonts.ts';

// Mesmo servidor padrão do desktop (desktop/app/src/settings.rs DEFAULT_SERVER).
const ALLOW = ['live-cmd-signaling.yago-riccomi-s.workers.dev'];
const MAX_OUT = 20000;
const dec = new TextDecoder();
const enc = new TextEncoder();
// ponytail: só remove CSI/OSC; emulador VT completo vem em M3.
const strip = (s: string) => s.replace(/\x1b\][^\x07\x1b]*(\x07|\x1b\\)/g, '').replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, '').replace(/\r/g, '');

async function seed(): Promise<Uint8Array> {
  const s = await SecureStore.getItemAsync('client-ed25519');
  if (s) return b64d(s, 32);
  const n = getRandomBytes(32);
  await SecureStore.setItemAsync('client-ed25519', b64e(n));
  return n;
}

export default function App() {
  const [link, setLink] = useState('');
  const [pin, setPin] = useState<Pin | null>(null);
  const [status, setStatus] = useState('desconectado');
  const [out, setOut] = useState('');
  const conn = useRef<Conn | null>(null);
  /** Conexão autenticada (já com sessão): as abas remotas só pedem dados com ela. */
  const [live, setLive] = useState<Conn | null>(null);
  const [tab, setTab] = useState<Tab>('sessoes');
  const [scan, setScan] = useState(false);
  const [, askCam] = useCameraPermissions();
  // Sora/Fira Code: enquanto carregam, o texto usa a fonte do sistema (não bloqueia a abertura).
  useFonts(FONTS);
  const [prefs, setPrefsState] = useState<Prefs>(DEFAULT_PREFS);
  const prefsRef = useRef(prefs);
  prefsRef.current = prefs;
  const setPrefs = (p: Prefs) => { setPrefsState(p); savePrefs(p); };
  useEffect(() => { loadPrefs().then(setPrefsState); }, []);
  /** Agente pedindo confirmação (evento `agent_waiting`): aviso no topo de qualquer aba. */
  const [waiting, setWaiting] = useState<{ sessionId: string; label: string } | null>(null);
  /** Muda a cada evento `agent_waiting`: a aba Agentes recarrega a lista. */
  const [agentsRev, setAgentsRev] = useState(0);

  // "Vibrar ao concluir": comando terminou (OSC 133 `command_completed`) ou agente pediu confirmação.
  useEffect(() => {
    if (!live) { setWaiting(null); return; }
    // Só comando que durou mais de 10 s (do último Enter ao OSC 133;D).
    live.onCommandDone = () => { if (prefsRef.current.vibrate && longRun(live.lastEnterMs, Date.now())) Vibration.vibrate(40); };
    live.onAgentWaiting = e => {
      setAgentsRev(n => n + 1);
      if (e.waiting) {
        setWaiting({ sessionId: e.sessionId, label: e.label });
        if (prefsRef.current.vibrate) Vibration.vibrate([0, 80, 80, 80]);
      } else setWaiting(w => w?.sessionId === e.sessionId ? null : w);
    };
    // A inscrição no evento vem com `agentList(true)`; só com o cap `agent_waiting`.
    if (live.caps.includes('agent_waiting')) live.extra(agentList(true)).catch(() => {});
    return () => { live.onCommandDone = undefined; live.onAgentWaiting = undefined; };
  }, [live]);

  useEffect(() => { SecureStore.getItemAsync('pin').then(p => p && setPin(JSON.parse(p))); }, []);
  // QR lido pela câmera do sistema abre o app via scheme remoteterm://
  useEffect(() => {
    const go = (u?: string | null) => { if (u?.startsWith('remoteterm://pair?')) { setLink(u); connect(true, u); } };
    Linking.getInitialURL().then(go);
    const s = Linking.addEventListener('url', e => go(e.url));
    return () => s.remove();
  }, []);

  async function connect(pair: boolean, text = link) {
    try {
      let p = pin, otk: Uint8Array | undefined;
      if (pair) {
        const t = text.trim();
        const q = parseQr(t.slice(Math.max(0, t.indexOf('remoteterm://'))), ALLOW);
        p = { hostPk: b64e(q.hostPk), room: q.session, server: q.server };
        otk = q.otk;
      }
      if (!p) return;
      const c = new Conn(p, await seed(), otk, {
        status: s => {
          setStatus(s);
          if (s === 'conectado') setLive(c);
          if (s === 'conectado' && pair) { SecureStore.setItemAsync('pin', JSON.stringify(p)); setPin(p); setLink(''); }
        },
        output: b => setOut(o => (o + strip(dec.decode(b, { stream: true }))).slice(-MAX_OUT)),
        reset: () => setOut(''),
        closed: code => { if (prefsRef.current.vibrate) Vibration.vibrate(60); setStatus(code ? `fechado: ${code}` : 'desconectado'); conn.current = null; setLive(null); },
      });
      conn.current = c;
      c.start();
    } catch (e: any) { setStatus(`erro: ${e?.code ?? e?.message}`); }
  }

  /** Agente retomado ou SSH aberto: mostra a sessão no terminal. */
  async function open(sessionId: string) {
    try { await live?.switchTo(sessionId); setTab('sessoes'); } catch (e) { setStatus(errText(e)); }
  }

  const send = (s: string) => conn.current?.input(enc.encode(s));

  if (scan) return (
    <SafeAreaView style={st.root}>
      <CameraView style={{ flex: 1 }} barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
        onBarcodeScanned={({ data }) => { if (!data.includes('remoteterm://pair?')) return; setScan(false); setLink(data); connect(true, data); }} />
      <View style={{ padding: 12 }}><Pill title="Cancelar" onPress={() => setScan(false)} label="Cancelar leitura do QR code" /></View>
    </SafeAreaView>
  );

  return (
    <Shell bar={<TabBar tab={tab === 'historico' ? 'sessoes' : tab} onChange={setTab} />}>
      <StatusBar style="light" />
      {waiting && (
        <Banner text={`${waiting.label || 'Agente'} está esperando sua confirmação`}
          onPress={() => { const id = waiting.sessionId; setWaiting(null); open(id); }} onClose={() => setWaiting(null)} />
      )}
      {tab === 'sessoes' && (
        <SessionsScreen status={status} on={!!conn.current} pin={pin} out={out} link={link} setLink={setLink}
          onPair={() => connect(true)} onScan={async () => { if ((await askCam()).granted) setScan(true); }}
          onReconnect={() => connect(false)} onDisconnect={() => conn.current?.close()} send={send}
          conn={live} onOpen={open} onHistory={() => setTab('historico')} termFont={prefs.termFont} />
      )}
      {tab === 'agentes' && <AgentsScreen conn={live} onOpen={open} rev={agentsRev} />}
      {tab === 'ssh' && <SshScreen conn={live} onOpen={open} />}
      {tab === 'historico' && <HistoryScreen conn={live} />}
      {tab === 'ajustes' && (
        <SettingsScreen conn={live} pin={pin} status={status} prefs={prefs} setPrefs={setPrefs} onOpenTerminals={() => setTab('sessoes')}
          onUnpair={() => { conn.current?.close(); SecureStore.deleteItemAsync('pin'); setPin(null); }} />
      )}
    </Shell>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, backgroundColor: C.bg },
});

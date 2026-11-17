import { useEffect, useRef, useState } from 'react';
import { Linking, SafeAreaView, StyleSheet, View } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import { getRandomBytes } from 'expo-crypto';
import { CameraView, useCameraPermissions } from 'expo-camera';
import { b64d, b64e, parseQr } from './src/auth.ts';
import { Conn, type Pin } from './src/conn.ts';
import { Pill } from './src/screens/common.tsx';
import { TabBar, type Tab } from './src/screens/TabBar.tsx';
import { SessionsScreen } from './src/screens/SessionsScreen.tsx';
import { AgentsScreen } from './src/screens/AgentsScreen.tsx';
import { SshScreen } from './src/screens/SshScreen.tsx';
import { HistoryScreen } from './src/screens/HistoryScreen.tsx';
import { C, errText } from './src/screens/theme.ts';

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
        closed: code => { setStatus(code ? `fechado: ${code}` : 'desconectado'); conn.current = null; setLive(null); },
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
    <SafeAreaView style={st.root}>
      {tab === 'sessoes' && (
        <SessionsScreen status={status} on={!!conn.current} pin={pin} out={out} link={link} setLink={setLink}
          onPair={() => connect(true)} onScan={async () => { if ((await askCam()).granted) setScan(true); }}
          onReconnect={() => connect(false)} onDisconnect={() => conn.current?.close()} send={send} />
      )}
      {tab === 'agentes' && <AgentsScreen conn={live} onOpen={open} />}
      {tab === 'ssh' && <SshScreen conn={live} onOpen={open} />}
      {tab === 'historico' && <HistoryScreen conn={live} onRan={() => setTab('sessoes')} />}
      <TabBar tab={tab} onChange={setTab} />
    </SafeAreaView>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, backgroundColor: C.bg },
});

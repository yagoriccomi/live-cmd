import { useEffect, useRef, useState } from 'react';
import { Button, Linking, SafeAreaView, ScrollView, StyleSheet, Text, TextInput, View } from 'react-native';
import * as SecureStore from 'expo-secure-store';
import { getRandomBytes } from 'expo-crypto';
import { CameraView, useCameraPermissions } from 'expo-camera';
import { b64d, b64e, parseQr } from './src/auth.ts';
import { Conn, type Pin } from './src/conn.ts';

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
  const [line, setLine] = useState('');
  const conn = useRef<Conn | null>(null);
  const scroll = useRef<any>(null);
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
          if (s === 'conectado' && pair) { SecureStore.setItemAsync('pin', JSON.stringify(p)); setPin(p); setLink(''); }
        },
        output: b => setOut(o => (o + strip(dec.decode(b, { stream: true }))).slice(-MAX_OUT)),
        closed: code => { setStatus(code ? `fechado: ${code}` : 'desconectado'); conn.current = null; },
      });
      conn.current = c;
      c.start();
    } catch (e: any) { setStatus(`erro: ${e?.code ?? e?.message}`); }
  }

  const send = (s: string) => conn.current?.input(enc.encode(s));
  const on = !!conn.current;

  if (scan) return (
    <SafeAreaView style={st.root}>
      <CameraView style={{ flex: 1 }} barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
        onBarcodeScanned={({ data }) => { if (!data.includes('remoteterm://pair?')) return; setScan(false); setLink(data); connect(true, data); }} />
      <Button title="Cancelar" onPress={() => setScan(false)} />
    </SafeAreaView>
  );

  return (
    <SafeAreaView style={st.root}>
      <Text style={st.status}>{status}</Text>
      {!on && (
        <View>
          <TextInput style={st.input} value={link} onChangeText={setLink} placeholder="Cole o link remoteterm://pair?..." autoCapitalize="none" autoCorrect={false} />
          <Button title="Parear" onPress={() => connect(true)} disabled={!link} />
          <Button title="Ler QR code" onPress={async () => { if ((await askCam()).granted) setScan(true); }} />
          {pin && <Button title="Conectar ao PC pareado" onPress={() => connect(false)} />}
        </View>
      )}
      {on && <Button title="Desconectar" onPress={() => conn.current?.close()} />}
      <ScrollView ref={scroll} style={st.term} onContentSizeChange={() => scroll.current?.scrollToEnd({ animated: false })}>
        <Text style={st.mono} selectable>{out}</Text>
      </ScrollView>
      <View style={st.row}>
        <TextInput style={[st.input, { flex: 1 }]} value={line} onChangeText={setLine} autoCapitalize="none" autoCorrect={false}
          placeholder="comando" onSubmitEditing={() => { send(line + '\r'); setLine(''); }} blurOnSubmit={false} editable={on} />
        <Button title="Enviar" onPress={() => { send(line + '\r'); setLine(''); }} disabled={!on} />
        <Button title="Ctrl+C" onPress={() => send('\x03')} disabled={!on} />
      </View>
    </SafeAreaView>
  );
}

const st = StyleSheet.create({
  root: { flex: 1, padding: 8, paddingTop: 32, backgroundColor: '#140b2e' },
  status: { color: '#c8b8ff', marginBottom: 6 },
  input: { borderWidth: 1, borderColor: 'rgba(206,170,255,0.5)', borderRadius: 14, backgroundColor: 'rgba(255,255,255,0.08)', color: '#f4efff', padding: 6, marginVertical: 4 },
  term: { flex: 1, backgroundColor: 'rgba(10,4,30,0.85)', borderRadius: 14, marginVertical: 6 },
  mono: { fontFamily: 'monospace', color: '#e6dcff', fontSize: 12 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 4 },
});

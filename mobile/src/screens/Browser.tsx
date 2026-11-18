import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, Text, View } from 'react-native';
import type { Conn } from '../conn.ts';
import { folderList, joinPath, parentPath, sshKeyList } from '../extras.ts';
import { s } from './common.tsx';
import { C, errText } from './theme.ts';

type Listing = { path: string; up: string | null; dirs: string[]; keys: string[] };

/** Navegador de arquivos do PC, sem digitar caminho.
 *  `folders`: subpastas a partir de `start` (raiz = unidades), como o diálogo de agente do desktop; tocar numa pasta a escolhe e entra nela.
 *  `keys`: só dentro de %USERPROFILE%\.ssh do PC (`ssh_key_list`, o host recusa sair dela); tocar numa chave a escolhe. */
export function Browser({ conn, mode, start, onPick }: { conn: Conn; mode: 'folders' | 'keys'; start?: string; onPick: (path: string) => void }) {
  const [at, setAt] = useState(start ?? '');
  const [list, setList] = useState<Listing | null>(null);
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    setBusy(true); setErr('');
    const req = mode === 'folders'
      ? conn.extra(folderList(at)).then((r): Listing => ({ path: r.path, up: r.path ? parentPath(r.path) : null, dirs: r.dirs, keys: [] }))
      : conn.extra(sshKeyList(at || undefined)).then((r): Listing => ({ path: r.path, up: r.path.toLowerCase() === r.root.toLowerCase() ? null : parentPath(r.path), dirs: r.dirs, keys: r.keys }));
    req.then(l => { if (live) setList(l); })
      .catch(e => { if (live) setErr(errText(e) === 'Não encontrado no PC.' ? (mode === 'keys' ? 'Pasta .ssh não encontrada no PC.' : 'Pasta não encontrada no PC.') : errText(e)); })
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [conn, mode, at]);

  const go = (path: string) => { setAt(path); if (mode === 'folders') onPick(path); };
  const Item = ({ text, label, onPress, strong }: { text: string; label: string; onPress: () => void; strong?: boolean }) => (
    <Pressable onPress={onPress} accessibilityRole="button" accessibilityLabel={label}
      style={({ pressed }) => [{ minHeight: 44, justifyContent: 'center', paddingHorizontal: 10, borderRadius: 10 }, pressed && { backgroundColor: C.glass }]}>
      <Text style={[s.mono, strong && { color: C.text, fontWeight: '600' }]} numberOfLines={1}>{text}</Text>
    </Pressable>
  );

  return (
    <View style={[s.glass, { padding: 6, marginTop: 4 }]} accessibilityLabel={mode === 'keys' ? 'Chaves no PC' : 'Pastas no PC'}>
      <View style={[s.row, { paddingHorizontal: 6, minHeight: 32 }]}>
        <Text style={[s.meta, { flex: 1 }]} numberOfLines={1}>{list?.path || (mode === 'folders' ? 'Unidades do PC' : '.ssh')}</Text>
        {busy && <ActivityIndicator color={C.text} />}
      </View>
      {!!err && <Text style={[s.meta, { color: C.err, padding: 6 }]} accessibilityLiveRegion="polite">{err}</Text>}
      {list && list.up !== null && <Item text=".." label="Pasta de cima" onPress={() => go(list.up!)} />}
      {list?.dirs.map(d => <Item key={'d' + d} text={list.path ? `${d}\\` : d} label={`Pasta ${d}`} onPress={() => go(joinPath(list.path, d))} />)}
      {list?.keys.map(k => <Item key={'k' + k} text={k} label={`Usar a chave ${k}`} strong onPress={() => onPick(joinPath(list.path, k))} />)}
      {list && !busy && !err && list.dirs.length === 0 && list.keys.length === 0 && (
        <Text style={[s.meta, { padding: 6 }]}>{mode === 'keys' ? 'Nenhuma chave nesta pasta.' : 'Nenhuma subpasta.'}</Text>
      )}
    </View>
  );
}

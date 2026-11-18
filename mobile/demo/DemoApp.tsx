// Modo de demonstração SÓ PARA CAPTURA de tela (comparação com os mockups V1 em docs/design/aurora-v1/).
// Fora do caminho de produção: App.tsx não importa nada daqui; só demo/index.ts (entrada alternativa) e o
// harness web da comparação usam. Dados iguais aos dos mockups; a "conexão" responde com fixtures, sem rede.
import { useEffect, useState } from 'react';
import { Linking } from 'react-native';
import { useFonts } from 'expo-font';
import { StatusBar } from 'expo-status-bar';
import { FONTS } from '../src/screens/fonts.ts';
import type { Conn } from '../src/conn.ts';
import type { Agent, HistoryEntry, SshServer, SshSession } from '../src/extras.ts';
import { Shell } from '../src/screens/aurora.tsx';
import { TabBar, type Tab } from '../src/screens/TabBar.tsx';
import { SessionsScreen } from '../src/screens/SessionsScreen.tsx';
import { AgentsScreen } from '../src/screens/AgentsScreen.tsx';
import { SshScreen } from '../src/screens/SshScreen.tsx';
import { HistoryScreen } from '../src/screens/HistoryScreen.tsx';
import { SettingsScreen } from '../src/screens/SettingsScreen.tsx';
import { DEFAULT_PREFS } from '../src/prefs.ts';

export type DemoName = 'terminal' | 'pareamento' | 'erro' | 'agentes' | 'ssh' | 'historico' | 'ajustes';
export const DEMOS: DemoName[] = ['terminal', 'pareamento', 'erro', 'agentes', 'ssh', 'historico', 'ajustes'];

const NOW = new Date(2026, 10, 17, 23, 50).getTime();
const at = (daysAgo: number, h: number, m: number) => {
  const d = new Date(NOW); d.setDate(d.getDate() - daysAgo); d.setHours(h, m, 0, 0);
  return BigInt(d.getTime());
};

const AGENTS_DATA: Agent[] = [
  { sessionId: 'a1', agent: 'claude', args: '', label: 'claude · live-cmd', cwd: 'S:\\Projetos\\live-cmd', state: 'running', lastActivityMs: at(0, 23, 41) },
  { sessionId: 'a2', agent: 'codex', args: '', label: 'codex · refactor', cwd: 'S:\\Projetos\\api', state: 'running', lastActivityMs: at(0, 23, 30) },
  { sessionId: 'a3', agent: 'claude', args: '', label: 'Fix setas ConPTY', cwd: 'S:\\Projetos\\live-cmd', state: 'saved', lastActivityMs: at(0, 22, 10) },
  { sessionId: 'a4', agent: 'claude', args: '', label: 'Menu da bandeja', cwd: 'S:\\Projetos\\live-cmd', state: 'saved', lastActivityMs: at(1, 19, 2) },
  { sessionId: 'a5', agent: 'codex', args: '', label: 'Migração do banco', cwd: 'S:\\Projetos\\api', state: 'exited', lastActivityMs: at(5, 16, 40) },
  { sessionId: 'a6', agent: 'claude', args: '', label: 'Release 0.1.7', cwd: 'S:\\Projetos\\api', state: 'saved', lastActivityMs: at(8, 10, 15) },
];
const SERVERS: SshServer[] = [
  { serverId: 's1', label: 'prod-01', host: 'prod-01.exemplo.com', user: 'deploy', port: 22, key: 'C:\\Users\\yago\\.ssh\\id_ed25519', latencyMs: 42, status: 'ok' },
  { serverId: 's2', label: 'staging', host: '10.0.0.12', user: 'deploy', port: 22, key: null, latencyMs: 18, status: 'ok' },
];
const SSH_SESSIONS: SshSession[] = [];
const HOST_SESSIONS = [
  { session_id: 'demo', label: 'claude · live-cmd', state: 'running', last_activity_ms: 0 },
  { session_id: 'pw', label: 'pwsh', state: 'running', last_activity_ms: 0 },
];
const HISTORY: HistoryEntry[] = [
  { entryId: 'h1', command: 'git push origin claude/mockups', cwd: 'S:\\Projetos\\live-cmd', atMs: at(0, 23, 41), exitCode: 0, kind: 'pwsh', label: 'pwsh', pc: 'DESKTOP-YAGO' },
  { entryId: 'h2', command: 'git push --tags', cwd: 'S:\\Projetos\\live-cmd', atMs: at(0, 22, 8), exitCode: 128, kind: 'pwsh', label: 'pwsh', pc: 'DESKTOP-YAGO' },
  { entryId: 'h3', command: 'git push deploy main', cwd: '/srv/app', atMs: at(1, 18, 30), exitCode: 0, kind: 'ssh', label: 'prod-01', pc: 'DESKTOP-YAGO' },
  { entryId: 'h4', command: 'git push -u origin fix/conpty', cwd: 'S:\\Projetos\\live-cmd', atMs: at(1, 11, 2), exitCode: 0, kind: 'pwsh', label: 'pwsh', pc: 'DESKTOP-YAGO' },
  { entryId: 'h5', command: 'git push', cwd: '/srv/app', atMs: at(2, 9, 47), exitCode: 0, kind: 'ssh', label: 'staging', pc: 'DESKTOP-YAGO' },
];
const OUT = [
  '❯ claude --resume fix-conpty',
  '✦ Retomando sessão de ontem 22:10…',
  'Lendo src/pty/keys.ts (212 linhas)',
  '✓ setas ↑ ↓ agora enviam ESC [A / ESC [B',
  'Editando src/pty/win32_input.rs',
  '? Rodar cargo test --package desktop',
  'test result: ok. 121 passed; 0 failed',
  'contexto 41% · 3 arquivos alterados',
  '❯ ',
].join('\n');

/** Conexão falsa: só responde às listas das abas, com os dados acima. */
const fake = {
  caps: ['agents', 'ssh', 'history'],
  sessionId: 'demo',
  extra: async (body: Record<string, unknown>) => {
    switch (body.type) {
      case 'agent_list': return { agents: AGENTS_DATA };
      case 'session_list': return { sessions: HOST_SESSIONS, profiles: [] };
      case 'ssh_list': return { servers: SERVERS, sessions: SSH_SESSIONS };
      case 'history_query': {
        const q = String(body.query ?? '').toLowerCase();
        return { entries: HISTORY.filter(e => e.command.toLowerCase().includes(q)) };
      }
      default: return {};
    }
  },
} as unknown as Conn;

const TAB_OF: Record<DemoName, Tab> = { terminal: 'sessoes', pareamento: 'sessoes', erro: 'sessoes', agentes: 'agentes', ssh: 'ssh', historico: 'sessoes', ajustes: 'ajustes' };
const PIN = { hostPk: '', room: '', server: 'live-cmd-signaling' };
const noop = () => {};

export function DemoScreen({ name }: { name: DemoName }) {
  const tab = TAB_OF[name];
  const sessions = (status: string, on: boolean, out: string) => (
    <SessionsScreen status={status} on={on} pin={on ? null : { hostPk: '', room: '', server: '' }} out={out} link="" setLink={noop}
      onPair={noop} onScan={noop} onReconnect={noop} onDisconnect={noop} send={noop}
      conn={on ? fake : null} onOpen={async () => {}} onHistory={noop} />
  );
  return (
    <Shell bar={<TabBar tab={tab} onChange={noop} />}>
      {name === 'terminal' && sessions('conectado', true, OUT)}
      {name === 'pareamento' && sessions('desconectado', false, '')}
      {name === 'erro' && sessions('fechado: Permission denied (publickey)', false, '')}
      {name === 'agentes' && <AgentsScreen conn={fake} onOpen={async () => {}} />}
      {name === 'ssh' && <SshScreen conn={fake} onOpen={async () => {}} />}
      {name === 'historico' && <HistoryScreen conn={fake} onRan={noop} />}
      {name === 'ajustes' && <SettingsScreen conn={fake} pin={PIN} status="conectado" prefs={{ ...DEFAULT_PREFS, termFont: 13 }} setPrefs={noop} onOpenTerminals={noop} onUnpair={noop} />}
    </Shell>
  );
}

/** Raiz da entrada de demonstração no aparelho: troca de tela por deep link remoteterm://demo?s=<nome>. */
export default function DemoApp() {
  const [name, setName] = useState<DemoName>('terminal');
  useFonts(FONTS);
  useEffect(() => {
    const go = (u?: string | null) => { const m = u?.match(/[?&]s=(\w+)/); if (m && (DEMOS as string[]).includes(m[1])) setName(m[1] as DemoName); };
    Linking.getInitialURL().then(go);
    const s = Linking.addEventListener('url', e => go(e.url));
    return () => s.remove();
  }, []);
  return <><StatusBar style="light" /><DemoScreen key={name} name={name} /></>;
}

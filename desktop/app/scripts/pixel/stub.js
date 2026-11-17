// Stub de window.__TAURI__ SÓ para captura de tela (scripts/pixel). Não entra no app.
// Devolve os mesmos dados dos mockups V1 Aurora. A tela vem do hash: #terminal, #agentes, #ssh,
// #historico, #estados; na janela principal, #pair e #settings (as abas do app.js).
(() => {
  const [screen = "terminal", theme] = location.hash.slice(1).split(":");
  try { localStorage.theme = theme || "dark"; } catch {} // #terminal:light, #terminal:contrast
  const ls = []; // ouvintes de evento
  const sid = (n) => "00000000-0000-0000-0000-00000000000" + n;
  const now = Date.now(), H = 3600e3, D = 24 * H;
  const today = (h, m) => { const d = new Date(); d.setHours(h, m, 0, 0); return +d; };
  const daysAgo = (n, h, m) => today(h, m) - n * D;
  const sessions = screen === "estados" ? [] : [
    { session_id: sid(1), label: "claude · live-cmd", state: "running" },
    { session_id: sid(2), label: "pwsh", state: "running" },
    { session_id: sid(3), label: "prod-01", state: "running" },
    { session_id: sid(4), label: "codex · refactor", state: "running" },
    { session_id: sid(5), label: "cmd", state: "exited" },
    { session_id: sid(6), label: "staging", state: "running" },
  ];
  const cwds = { 1: "S:\\Projetos\\live-cmd", 2: "S:\\Projetos\\live-cmd", 3: "deploy@prod-01 · htop", 4: "aguarda permissão", 5: "C:\\Users\\yago", 6: "deploy@10.0.0.12" };
  const DATA = {
    term_list: { sessions, profiles: [{ profile_id: "pwsh", label: "PowerShell" }] },
    term_connections: { remotes: [] },
    term_attach: { state: "running" },
    check_update: null,
    agent_list: { online: true, groups: [
      { name: "live-cmd", path: "S:\\Projetos\\live-cmd", sessions: [
        { session_id: sid(1), agent: "claude", label: "claude · live-cmd", status: "rodando", used_ms: now - 5 * 60e3 },
        { session_id: sid(7), agent: "claude", label: "Fix setas ConPTY", status: "encerrada", used_ms: now - D },
        { session_id: sid(8), agent: "claude", label: "Menu da bandeja", status: "encerrada", used_ms: now - 2 * D },
      ] },
      { name: "api", path: "S:\\Projetos\\api", sessions: [
        { session_id: sid(4), agent: "codex", label: "codex · refactor", status: "saiu", used_ms: now - 30 * 60e3 },
        { session_id: sid(9), agent: "codex", label: "Migração do banco", status: "encerrada", used_ms: now - 5 * D },
      ] },
    ] },
    ssh_list: [
      { id: "a", name: "prod-01", user: "deploy", host: "prod-01.exemplo.com", port: 22, key: "" },
      { id: "b", name: "staging", user: "deploy", host: "10.0.0.12", port: 22, key: "" },
      { id: "c", name: "homelab", user: "yago", host: "192.168.1.40", port: 22, key: "" },
      { id: "d", name: "backup-nas", user: "admin", host: "nas.local", port: 22, key: "" },
      { id: "e", name: "vps-lab", user: "root", host: "lab.exemplo.dev", port: 22, key: "" },
      { id: "f", name: "build-runner", user: "ci", host: "10.0.0.30", port: 22, key: "" },
    ],
    ssh_sessions: [{ session_id: sid(3), label: "prod-01", state: "connected" }],
    hist_list: [
      { ts: today(23, 41), cmd: "git push origin claude/mockups", pc: "Este PC", label: "pwsh · live-cmd", kind: "pwsh", code: 0 },
      { ts: today(22, 8), cmd: "git push --tags", pc: "Este PC", label: "pwsh · live-cmd", kind: "pwsh", code: 128 },
      { ts: daysAgo(1, 18, 30), cmd: "git push deploy main", pc: "Este PC", label: "prod-01", kind: "ssh", code: 0 },
      { ts: daysAgo(1, 11, 2), cmd: "git push -u origin fix/conpty", pc: "Este PC", label: "pwsh · live-cmd", kind: "pwsh", code: 0 },
      { ts: daysAgo(2, 9, 47), cmd: "git push", pc: "Este PC", label: "staging", kind: "ssh", code: 0 },
    ],
    // Janela principal (index.html): QR desenhado como o do mockup Pareamento.
    qr: { qr: "data:image/svg+xml," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 21 21" shape-rendering="crispEdges"><rect width="21" height="21" fill="#fff"/><g fill="#2a1460"><path d="M1 1h7v7H1zM13 1h7v7h-7zM1 13h7v7H1z"/><path fill="#8b5cf6" d="M3 3h3v3H3zM15 3h3v3h-3zM3 15h3v3H3z"/><path d="M9 2h1v2H9zM10 5h2v1h-2zM9 9h3v3H9zM14 9h2v1h-2zM2 10h2v1H2zM5 9h1v2H5zM13 13h3v3h-3zM9 14h1v2H9zM10 17h2v1h-2zM17 13h2v1h-2zM18 17h1v2h-1zM14 18h2v1h-2z"/></g></svg>'),
      now_ms: now, expires_ms: now + 292e3 },
    status: { state: "ready", pairing_expires_ms: now + 292e3 },
    devices: [{ id: "p8", label: "Pixel 8", connected: false, paired_at_ms: daysAgo(15, 10, 0) }],
    get_settings: { autostart: true, server: "wss://sinal.livecmd.dev", scrollback: 10000 },
    get_profiles: { profiles: null, shells: [{ label: "PowerShell", command: "pwsh.exe" }, { label: "WSL", command: "wsl.exe" }] },
  };
  const PROBE = { "prod-01.exemplo.com": { ok: true, ms: 42 }, "10.0.0.12": { ok: true, ms: 18 }, "192.168.1.40": { ok: false, code: "OFFLINE" },
    "nas.local": { ok: false, code: "ERROR" }, "lab.exemplo.dev": { ok: true, ms: 95 }, "10.0.0.30": { ok: false, code: "OFFLINE" } };
  async function invoke(cmd, args) {
    if (cmd === "ssh_probe") return PROBE[args.host];
    if (cmd === "term_list" && args.conn !== "local") return { sessions: [], profiles: [] };
    if (cmd in DATA) return structuredClone(DATA[cmd]);
    return null;
  }
  const emit = (name, payload) => { for (const [n, cb] of ls) if (n === name) cb({ payload }); };
  window.__TAURI__ = {
    core: { invoke },
    event: { listen: async (name, cb) => { ls.push([name, cb]); return () => {}; } },
    dialog: { open: async () => null, save: async () => null },
    opener: { openUrl: async () => {} },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) },
  };
  // Linhas do terminal do mockup: [texto, cor, fundo, flags].
  const P = "#ff4fd8", M = "#a99bd6", G = "#5ef2b0", Y = "#ffd35c";
  const rows = [
    [["❯", P], [" claude --resume fix-conpty"]], [["✦ Retomando sessão de ontem 22:10…", M]], [["Lendo src/pty/keys.ts (212 linhas)"]],
    [["✓ setas ↑ ↓ agora enviam ESC [A / ESC [B", G]], [["Editando src/pty/win32_input.rs"]], [["? Rodar cargo test --package desktop", Y]],
    [["test result: ok. 121 passed; 0 failed", G]], [["contexto 41% · 3 arquivos alterados", M]], [["❯", P], [" "], [" ", "#fff", "#fff", 16]],
  ].map((r) => r.map(([t, f = null, b = null, fl = 0]) => [t, f, b, fl]));
  const ready = (fn) => setTimeout(fn, 400);
  window.addEventListener("DOMContentLoaded", () => ready(async () => {
    for (const [i, c] of Object.entries(cwds)) emit("term-cwd", { key: "local/" + sid(i), cwd: c });
    const click = (id) => document.getElementById(id)?.click();
    if (screen === "terminal" || screen === "estados") {
      document.querySelector("#local-ul li button")?.click();
      await new Promise((r) => setTimeout(r, 150));
      emit("term-screen", { key: "local/" + sid(1), rows, app_cursor: false });
    }
    if (screen === "agentes") click("nav-agents");
    if (screen === "ssh") click("ssh-open");
    if (screen === "historico") { document.getElementById("hist-q").value = "git push"; click("hist-open"); }
    setTimeout(() => document.activeElement?.blur(), 300);
  }));
})();

// Robô E2E do app real (Tauri + WebView2) via tauri-driver/WebDriver, sem dependências npm.
// Uso: `npm test` nesta pasta, depois de `cargo build` em desktop/app e `cargo build --bin live-cmd-termd` em desktop.
// Precisa: `cargo install tauri-driver --locked` e msedgedriver da mesma versão do WebView2
// (caminho em MSEDGEDRIVER, padrão S:\tools\msedgedriver\msedgedriver.exe). APP=<exe> troca o binário.
// Isolado do app do usuário: LIVE_CMD_INSTANCE=e2e (pipe/token próprios, sem instância única) e pasta de dados temporária.
// Cenários: um arquivo por ticket em specs/NNN-*.mjs, exportando `default async (s, h)`; rodam em ordem de nome.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, existsSync, openSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const app = process.env.APP || join(here, "..", "target", "debug", "live-cmd-app.exe");
const termd = join(dirname(app), "live-cmd-termd.exe");
const edge = process.env.MSEDGEDRIVER || "S:\\tools\\msedgedriver\\msedgedriver.exe";
const data = mkdtempSync(join(tmpdir(), "live-cmd-e2e-"));
// WebView2 próprio: com a pasta padrão (dev.livecmd.app\EBWebView) o E2E dividia o processo de navegador com o app
// aberto do usuário e a sessão caía no meio ("browser has closed the connection").
const env = { ...process.env, LIVE_CMD_INSTANCE: "e2e", LIVE_CMD_DATA_DIR: data, WEBVIEW2_USER_DATA_FOLDER: join(data, "webview") };
for (const f of [app, termd, edge]) if (!existsSync(f)) { console.error("falta: " + f); process.exit(2); }

const PORT = 4444, base = `http://127.0.0.1:${PORT}`;
const driver = spawn("tauri-driver", ["--port", String(PORT), "--native-driver", edge], { env, stdio: ["ignore", openSync(join(env.LIVE_CMD_DATA_DIR, "driver.log"), "a"), openSync(join(env.LIVE_CMD_DATA_DIR, "driver.log"), "a")] });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function wd(method, path, body) {
  const r = await fetch(base + path, { method, headers: { "content-type": "application/json" }, body: body && JSON.stringify(body) });
  const j = await r.json();
  if (!r.ok) throw new Error(`${method} ${path}: ${j.value?.message || r.status}`);
  return j.value;
}
async function open() {
  for (let i = 0; ; i++) {
    try { return (await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } })).sessionId; }
    catch (e) { if (i > 40) throw e; await sleep(250); }
  }
}

// Ajudantes para os cenários.
function helpers() {
  const S = () => "/session/" + id;
  const h = {
    js: (src, ...args) => wd("POST", S() + "/execute/sync", { script: src, args }),
    async until(src, ms = 10000, ...args) {
      const end = Date.now() + ms;
      for (;;) { const v = await h.js(src, ...args); if (v) return v; if (Date.now() > end) throw new Error("tempo esgotado: " + src); await sleep(150); }
    },
    el: (css) => h.until(`return document.querySelector(arguments[0])`, 10000, css), // referência W3C já pronta para `origin`
    // Arrasto por ponteiro real (WebDriver Actions). HTML5 drag de DOM não é sintetizado pelo WebDriver.
    // Centro do elemento + deslocamento, em coordenadas da janela (a lista se redesenha: referência de elemento fica velha).
    at: (css, dx = 0, dy = 0) => h.until(`const r = document.querySelector(arguments[0])?.getBoundingClientRect();
      return r && [Math.round(r.x + r.width / 2 + arguments[1]), Math.round(r.y + r.height / 2 + arguments[2])]`, 10000, css, dx, dy),
    // Arrasto por ponteiro real (WebDriver Actions).
    async drag(fromCss, toCss, dx = 0, dy = 0) {
      const [x0, y0] = await h.at(fromCss), [x1, y1] = await h.at(toCss, dx, dy), mv = (x, y, duration = 100) => ({ type: "pointerMove", origin: "viewport", x, y, duration });
      await wd("POST", S() + "/actions", { actions: [{ type: "pointer", id: "m", parameters: { pointerType: "mouse" }, actions: [
        mv(x0, y0, 0), { type: "pointerDown", button: 0 }, { type: "pause", duration: 150 }, mv(x0 + 20, y0 + 5),
        mv((x0 + x1) >> 1, (y0 + y1) >> 1, 200), mv(x1, y1, 200), { type: "pause", duration: 200 }, { type: "pointerUp", button: 0 }] }] });
      await wd("DELETE", S() + "/actions");
    },
    // Move o ponteiro real pelos pontos [x, y, pausa ms] (hover).
    async hover(pts) {
      await wd("POST", S() + "/actions", { actions: [{ type: "pointer", id: "m", parameters: { pointerType: "mouse" }, actions:
        pts.flatMap(([x, y, ms = 0]) => [{ type: "pointerMove", origin: "viewport", x, y, duration: 60 }, { type: "pause", duration: ms }]) }] });
    },
    async click(css) { const [x, y] = await h.at(css); await wd("POST", S() + "/actions", { actions: [{ type: "pointer", id: "m", parameters: { pointerType: "mouse" }, actions: [
      { type: "pointerMove", origin: "viewport", x, y, duration: 0 }, { type: "pointerDown", button: 0 }, { type: "pointerUp", button: 0 }] }] }); },
    // Tecla real (WebDriver Actions), no elemento focado. F2 = "\uE032".
    async key(k) { await wd("POST", S() + "/actions", { actions: [{ type: "key", id: "k", actions: [{ type: "keyDown", value: k }, { type: "keyUp", value: k }] }] }); },
    // Fecha e reabre o app (mesma pasta de dados e daemon): testa o que persiste.
    async reopen() { await wd("DELETE", S()); id = await open(); await h.ready(); },
    ready: () => h.until(`return document.readyState === "complete" && !!document.getElementById("tabs")`, 20000),
    eq(a, b, what) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${what}: esperado ${JSON.stringify(b)}, veio ${JSON.stringify(a)}`); },
  };
  return h;
}

let id, fails = 0;
try {
  id = await open();
  const h = helpers();
  await h.ready();
  for (const f of readdirSync(join(here, "specs")).filter((f) => f.endsWith(".mjs") && f.includes(process.env.ONLY || "")).sort()) {
    const t0 = Date.now();
    const spec = (await import(pathToFileURL(join(here, "specs", f)))).default;
    // msedgedriver às vezes perde o renderer no meio de `/actions` ("browser has closed the connection"):
    // sessão nova e uma segunda tentativa; a falha não derruba os cenários seguintes.
    for (let n = 1; ; n++) {
      try { await spec(h); console.log(`ok   ${f} (${Date.now() - t0} ms)${n > 1 ? " [2a tentativa]" : ""}`); break; }
      catch (e) {
        const lost = /invalid session id/.test(e.message);
        if (lost) { await fetch(`${base}/session/${id}`, { method: "DELETE" }).catch(() => {}); id = await open(); await h.ready(); }
        if (lost && n < 2) continue;
        fails++; console.log(`FALHA ${f}: ${e.message}`); break;
      }
    }
  }
} catch (e) { fails++; console.log("FALHA ao abrir o app: " + e.message); }
finally {
  await fetch(`${base}/session/${id}`, { method: "DELETE" }).catch(() => {});
  spawnSync(termd, ["--shutdown"], { env }); // daemon do E2E (pipe próprio), nunca o do usuário
  if (driver.pid) spawnSync("taskkill", ["/PID", String(driver.pid), "/T", "/F"]);
}
console.log(fails ? `${fails} falha(s)` : "tudo ok");
process.exit(fails ? 1 : 0);

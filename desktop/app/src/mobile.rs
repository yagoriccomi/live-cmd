//! Dados das telas do celular (Agentes, SSH, Histórico): o host (`live_cmd_echo::extras`) pede ao
//! provedor registrado aqui, que usa a mesma lógica e os mesmos arquivos das telas desktop
//! (`agents.rs`, `settings.rs`, `ssh.rs`, `hist.rs`) na pasta de dados do app.
//! Roda com o lock do hub tomado: só disco; o teste de alcance SSH vai para outra thread.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use live_cmd_echo::extras::{Agent, AgentDefault, AgentState, Appearance, HistoryEntry, HistoryFilter, Input, Provider, SshServer};

use crate::{agents, hist, settings, ssh, term};

/// Latência guardada vale por este tempo; depois, um novo teste em segundo plano.
const FRESH: Duration = Duration::from_secs(30);
/// Teto do teste de alcance (só em segundo plano; a lista nunca espera por ele).
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Teste de alcance: latência em ms ou o código da falha (`DNS`, `TIMEOUT`, `OFFLINE`, …), como `ssh::probe`.
pub type Probe = Arc<dyn Fn(&str, u16) -> Result<u32, String> + Send + Sync>;
/// Aviso para as janelas desktop: (evento, dados). Ex.: `appearance`, `agent-waiting`.
pub type Notify = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

#[derive(Default)]
struct Lat { r: Option<Result<u32, String>>, at: Option<Instant>, busy: bool }

pub struct AppProvider {
    dir: PathBuf,
    probe: Probe,
    /// Por `(host, porta)`.
    lat: Arc<Mutex<HashMap<(String, u16), Lat>>>,
    /// O mesmo store da tela Histórico (ver `hist::Shared`).
    hist: hist::Shared,
    /// Digitação desde o último Enter e último comando à espera do código, por sessão deste PC (`Provider::session_input`).
    typed: Mutex<HashMap<[u8; 16], hist::Typed>>,
    pending: Mutex<HashMap<[u8; 16], u64>>,
    notify: Option<Notify>,
}

impl AppProvider {
    pub fn new(dir: PathBuf) -> AppProvider {
        let probe: Probe = Arc::new(|h, p| {
            let v = ssh::probe(h, p, PROBE_TIMEOUT);
            v["ms"].as_u64().map(|m| m.min(u32::MAX as u64) as u32).ok_or_else(|| v["code"].as_str().unwrap_or("OFFLINE").to_owned())
        });
        AppProvider::with_probe(dir, probe)
    }

    pub fn with_probe(dir: PathBuf, probe: Probe) -> AppProvider {
        AppProvider { dir, probe, lat: Default::default(), hist: Default::default(), typed: Default::default(), pending: Default::default(), notify: None }
    }

    /// Avisa as janelas desktop quando o celular muda a aparência ou um agente passa a esperar confirmação.
    pub fn with_notify(mut self, n: Notify) -> AppProvider { self.notify = Some(n); self }

    /// Usa o store da tela Histórico em vez de um próprio.
    pub fn with_history(mut self, h: hist::Shared) -> AppProvider { self.hist = h; self }

    /// Último resultado conhecido; se velho (ou nunca medido) ou `force`, dispara um teste em outra thread.
    fn latency(&self, host: &str, port: u16, force: bool) -> (Option<u32>, &'static str) {
        let key = (host.to_owned(), port);
        let mut m = self.lat.lock().unwrap_or_else(|e| e.into_inner());
        let l = m.entry(key.clone()).or_default();
        if !l.busy && (force || l.at.is_none_or(|t| t.elapsed() >= FRESH)) {
            l.busy = true;
            let (lat, probe) = (self.lat.clone(), self.probe.clone());
            std::thread::spawn(move || {
                let r = probe(&key.0, key.1);
                let mut m = lat.lock().unwrap_or_else(|e| e.into_inner());
                m.insert(key, Lat { r: Some(r), at: Some(Instant::now()), busy: false });
            });
        }
        // Mesmos estados da tela desktop (ssh.js `statusText`).
        let status = match (&l.r, l.busy) {
            (_, true) => "testing",
            (None, false) => "unknown",
            (Some(Ok(_)), _) => "ok",
            (Some(Err(c)), _) => match c.as_str() { "DNS" => "dns", "TIMEOUT" => "timeout", _ => "offline" },
        };
        (l.r.as_ref().and_then(|r| r.as_ref().ok()).copied(), status)
    }

    fn with_history_store<R>(&self, f: impl FnOnce(&mut hist::Store) -> R) -> R { hist::with_shared(&self.hist, &self.dir, f) }
}

/// `xxxxxxxx-xxxx-…` em 16 bytes.
fn uuid_bytes(s: &str) -> Option<[u8; 16]> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if s.len() != 36 || hex.len() != 32 { return None; }
    let v: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

fn uuid_str(b: &[u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

impl Provider for AppProvider {
    /// Registro da tela Agentes (`agents.json`), só das sessões deste PC (conexão local).
    fn saved_agents(&self) -> Vec<Agent> {
        agents::read(&self.dir).into_iter().filter(|a| a.conn == term::LOCAL).filter_map(|a| Some(Agent {
            session_id: uuid_bytes(&a.session_id)?, agent: a.agent, args: a.args, label: a.label, cwd: a.cwd,
            state: AgentState::Saved, last_activity_ms: a.used_ms, profile_id: Some(a.profile_id), waiting: false,
        })).collect()
    }

    /// Toda sessão de agente deste PC (criada pelo desktop ou pelo celular) entra no registro; `record` marca `used_ms`.
    fn agent_started(&self, a: &Agent) {
        let _ = agents::record(&self.dir, agents::Agent {
            conn: term::LOCAL.into(), session_id: uuid_str(&a.session_id), profile_id: a.profile_id.clone().unwrap_or_default(),
            agent: a.agent.clone(), args: a.args.clone(), cwd: a.cwd.clone(), label: a.label.clone(), used_ms: 0,
        });
    }

    fn agent_removed(&self, session_id: &[u8; 16]) { let _ = agents::remove_entry(&self.dir, term::LOCAL, &uuid_str(session_id)); }

    fn ssh_servers(&self) -> Vec<SshServer> {
        settings::ssh_servers(&self.dir).unwrap_or_default().into_iter().map(|s| {
            let (latency_ms, status) = self.latency(&s.host, s.port, false);
            SshServer { latency_ms, status, server_id: s.id, label: s.name, host: s.host, user: s.user, port: s.port, key: s.key, keepalive_s: s.keepalive_s, auto_reconnect: s.auto_reconnect }
        }).collect()
    }

    fn ssh_target(&self, server_id: &str) -> Option<live_cmd_echo::ssh::Ssh> {
        settings::ssh_servers(&self.dir).ok()?.into_iter().find(|s| s.id == server_id).map(|s| s.target()).filter(|t| t.check().is_ok())
    }

    /// Mesma regra da tela desktop (`settings::save_ssh_server`): nome, destino validado, limite de 200.
    fn ssh_save(&self, server_id: Option<&str>, label: &str, t: &live_cmd_echo::ssh::Ssh) -> Result<String, String> {
        let s = settings::SshServer { id: server_id.unwrap_or_default().into(), name: label.into(), user: t.user.clone(), host: t.host.clone(), port: t.port, key: t.key.clone(),
            keepalive_s: t.keepalive_s, auto_reconnect: t.auto_reconnect };
        settings::save_ssh_server(&self.dir, s).map(|s| s.id)
    }

    /// Mesma aparência da tela Configurações do desktop (`appearance.json`).
    fn settings_get(&self) -> Option<Appearance> { Some(settings::appearance(&self.dir)) }

    fn settings_set(&self, a: &Appearance) -> Result<(), String> {
        settings::save_appearance(&self.dir, a)?;
        if let Some(n) = &self.notify { n("appearance", a.json()); }
        Ok(())
    }

    fn agent_waiting(&self, session_id: &[u8; 16], label: &str, waiting: bool) {
        if let Some(n) = &self.notify { n("agent-waiting", serde_json::json!({ "session_id": uuid_str(session_id), "label": label, "waiting": waiting })); }
    }

    fn ssh_remove(&self, server_id: &str) -> Result<(), String> {
        if !settings::ssh_servers(&self.dir)?.iter().any(|s| s.id == server_id) { return Err("NOT_FOUND".into()); }
        settings::remove_ssh_server(&self.dir, server_id)
    }

    fn ssh_probe(&self, server_id: &str) -> Result<(), String> {
        let s = settings::ssh_servers(&self.dir)?.into_iter().find(|s| s.id == server_id).ok_or("NOT_FOUND")?;
        self.latency(&s.host, s.port, true);
        Ok(())
    }

    /// Mesmos filtros da tela desktop (`hist::Query`).
    /// Mesmo resumo da tela Histórico do desktop (`hist::sessions`), sobre o histórico inteiro do filtro.
    fn history_sessions(&self, f: &HistoryFilter, _offset_min: i32) -> Vec<live_cmd_echo::extras::HistorySession> {
        let q = hist::Query { q: f.query.clone(), days: f.days, kind: f.kind.clone(), errors: f.errors, limit: Some(usize::MAX) };
        self.with_history_store(|s| hist::sessions(s, &q, crate::host::now_ms()))
    }

    fn history(&self, f: &HistoryFilter) -> Vec<HistoryEntry> {
        let q = hist::Query { q: f.query.clone(), days: f.days, kind: f.kind.clone(), errors: f.errors, limit: Some(f.limit) };
        self.with_history_store(|s| s.search(&q, crate::host::now_ms())).into_iter().map(|e| HistoryEntry {
            entry_id: e.id.to_string(), command: e.cmd, cwd: e.cwd, at_ms: e.ts, exit_code: e.code.and_then(|c| i32::try_from(c).ok()),
            kind: e.kind, label: e.label, pc: e.pc,
        }).collect()
    }

    fn history_command(&self, entry_id: &str) -> Option<String> {
        let id = entry_id.parse().ok()?;
        self.with_history_store(|s| s.get(id).map(|e| e.cmd.clone()))
    }

    fn history_clear(&self) -> Result<(), String> { self.with_history_store(|s| s.clear()).map_err(|_| "IO_ERROR".into()) }

    /// Ponto comum do histórico das sessões deste PC: terminal desktop (cliente local), outro PC e celular.
    /// Mesmas regras de `hist::on_input`: senha, sem eco e tela alternativa não gravam; o host já tirou as de agente.
    fn session_input(&self, i: &Input) {
        let cmds = hist::on_input(self.typed.lock().unwrap_or_else(|e| e.into_inner()).entry(i.session_id).or_default(), i.data, i.line.as_deref());
        if cmds.is_empty() { return; }
        let kind = if i.ssh { "ssh" } else { hist::kind_of(&i.label) };
        if kind == "ia" { return; }
        let label = if i.label.is_empty() { "Terminal" } else { i.label.as_str() };
        for c in cmds {
            if let Ok(id) = self.with_history_store(|s| s.add_in(crate::host::now_ms(), &c, kind, label, "Este PC", i.cwd.as_deref())) {
                self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(i.session_id, id);
            }
        }
    }

    fn command_done(&self, session_id: &[u8; 16], code: Option<i32>) {
        let Some(id) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(session_id) else { return };
        if let Some(c) = code { let _ = self.with_history_store(|s| s.set_code(id, c.into())); }
    }

    fn session_gone(&self, session_id: &[u8; 16]) {
        self.typed.lock().unwrap_or_else(|e| e.into_inner()).remove(session_id);
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(session_id);
    }

    /// "Salvar como padrão": o mesmo `agent-default.json` do diálogo desktop.
    fn agent_default(&self) -> Option<AgentDefault> {
        agents::read_default(&self.dir).map(|d| AgentDefault { agent: d.agent, cwd: d.cwd, args: d.args })
    }

    fn agent_default_save(&self, d: &AgentDefault) -> Result<(), String> {
        agents::save_default(&self.dir, agents::AiDefault { agent: d.agent.clone(), cwd: d.cwd.clone(), args: d.args.clone() }).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("live-cmd-mobile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn uuid_em_bytes() {
        assert_eq!(uuid_bytes("0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").map(|b| b[0]), Some(0x0f));
        assert_eq!(uuid_bytes("0f1e2d3c4b5a49688778695a4b3c2d1e"), None);
        assert_eq!(uuid_bytes("zz1e2d3c-4b5a-4968-8778-695a4b3c2d1e"), None);
        assert_eq!(uuid_str(&uuid_bytes("0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").unwrap()), "0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e");
    }

    #[test]
    fn agentes_so_deste_pc() {
        let d = tmp("agentes");
        let a = |conn: &str, sid: &str| agents::Agent { conn: conn.into(), session_id: sid.into(), profile_id: "pwsh".into(), agent: "claude".into(), args: "--model opus".into(), cwd: Some(r"C:\p".into()), label: "p · claude".into(), used_ms: 0 };
        agents::record(&d, a("local", "0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e")).unwrap();
        agents::record(&d, a("pc2", "1f1e2d3c-4b5a-4968-8778-695a4b3c2d1e")).unwrap();
        agents::record(&d, a("local", "id-ruim")).unwrap();
        let l = AppProvider::new(d.clone()).saved_agents();
        assert_eq!(l.len(), 1, "outro PC e id ilegível ficam fora");
        assert_eq!((l[0].session_id[0], l[0].state, l[0].profile_id.as_deref(), l[0].args.as_str()), (0x0f, AgentState::Saved, Some("pwsh"), "--model opus"));
        assert!(l[0].last_activity_ms > 0);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn agente_do_celular_entra_no_registro_e_atualiza_uso() {
        let d = tmp("registro");
        let p = AppProvider::new(d.clone());
        let sid = uuid_bytes("2f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").unwrap();
        let a = Agent { session_id: sid, agent: "codex".into(), args: String::new(), label: "api · codex".into(), cwd: Some(r"C:\api".into()), state: AgentState::Running, last_activity_ms: 0, profile_id: Some("pwsh".into()), waiting: false };
        p.agent_started(&a);
        let r = agents::read(&d);
        assert_eq!((r.len(), r[0].conn.as_str(), r[0].label.as_str(), r[0].profile_id.as_str()), (1, "local", "api · codex", "pwsh"));
        let first = r[0].used_ms;
        std::thread::sleep(Duration::from_millis(5));
        p.agent_started(&a);
        let r = agents::read(&d);
        assert_eq!(r.len(), 1, "retomar não duplica");
        assert!(r[0].used_ms > first, "retomar marca o uso");
        p.agent_removed(&sid);
        assert!(agents::read(&d).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ssh_lista_sem_esperar_o_teste_e_destino_validado() {
        let d = tmp("ssh");
        let srv = |name: &str, user: &str| settings::SshServer { id: String::new(), name: name.into(), user: user.into(), host: "10.0.0.5".into(), port: 2222, key: None, ..Default::default() };
        let s = settings::save_ssh_server(&d, srv("prod", "deploy")).unwrap();
        let calls = Arc::new(Mutex::new(0));
        let c = calls.clone();
        let p = AppProvider::with_probe(d.clone(), Arc::new(move |h, port| { assert_eq!((h, port), ("10.0.0.5", 2222)); *c.lock().unwrap() += 1; Ok(7) }));
        let l = p.ssh_servers();
        assert_eq!((l[0].server_id.as_str(), l[0].label.as_str(), l[0].user.as_str(), l[0].port), (s.id.as_str(), "prod", "deploy", 2222));
        let t = Instant::now();
        while p.ssh_servers()[0].latency_ms.is_none() { assert!(t.elapsed() < Duration::from_secs(2), "teste em segundo plano não voltou"); std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!((p.ssh_servers()[0].latency_ms, p.ssh_servers()[0].status), (Some(7), "ok"));
        assert_eq!(*calls.lock().unwrap(), 1, "latência recente fica em cache");
        // "Testar" do celular força um teste novo.
        p.ssh_probe(&s.id).unwrap();
        let t = Instant::now();
        while *calls.lock().unwrap() < 2 { assert!(t.elapsed() < Duration::from_secs(2)); std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!(p.ssh_probe("nao-existe"), Err("NOT_FOUND".into()));
        let t = p.ssh_target(&s.id).unwrap();
        assert_eq!((t.user.as_str(), t.host.as_str(), t.port), ("deploy", "10.0.0.5", 2222));
        assert_eq!(p.ssh_target("nao-existe"), None);
        // Arquivo editado à mão com destino inválido: não conecta.
        std::fs::write(d.join("ssh.json"), r#"[{"id":"x","name":"x","user":"-oProxyCommand=calc","host":"h","port":22}]"#).unwrap();
        assert_eq!(p.ssh_target("x"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ssh_falha_vira_status() {
        let d = tmp("ssh-falha");
        settings::save_ssh_server(&d, settings::SshServer { id: String::new(), name: "x".into(), user: "u".into(), host: "nada.invalid".into(), port: 22, key: None, ..Default::default() }).unwrap();
        let p = AppProvider::with_probe(d.clone(), Arc::new(|_, _| Err("DNS".into())));
        assert_eq!(p.ssh_servers()[0].status, "testing");
        let t = Instant::now();
        while p.ssh_servers()[0].status == "testing" { assert!(t.elapsed() < Duration::from_secs(2)); std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!((p.ssh_servers()[0].status, p.ssh_servers()[0].latency_ms), ("dns", None));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ssh_cadastro_pelo_celular_usa_a_regra_do_desktop() {
        let d = tmp("ssh-save");
        let p = AppProvider::new(d.clone());
        let t = |user: &str| live_cmd_echo::ssh::Ssh { user: user.into(), host: "h".into(), port: 22, key: Some(r"C:\k\id".into()), ..Default::default() };
        let id = p.ssh_save(None, "srv", &t("u")).unwrap();
        assert_eq!(p.ssh_save(Some(&id), "srv2", &t("u2")).unwrap(), id);
        let l = settings::ssh_servers(&d).unwrap();
        assert_eq!((l.len(), l[0].name.as_str(), l[0].user.as_str(), l[0].key.as_deref()), (1, "srv2", "u2", Some(r"C:\k\id")));
        assert_eq!(p.ssh_save(None, "", &t("u")), Err("INVALID_SSH_NAME".into()));
        assert_eq!(p.ssh_save(Some("nao-existe"), "x", &t("u")), Err("NOT_FOUND".into()));
        p.ssh_remove(&id).unwrap();
        assert!(settings::ssh_servers(&d).unwrap().is_empty());
        assert_eq!(p.ssh_remove(&id), Err("NOT_FOUND".into()));
        let _ = std::fs::remove_dir_all(d);
    }

    /// Ponto comum do host (`session_input`): comando digitado pelo celular ou pelo terminal desktop entra uma vez,
    /// com as mesmas regras da tela desktop; senha, sem eco e tela alternativa não gravam; código chega depois.
    #[test]
    fn historico_do_host_com_as_regras_do_desktop() {
        let d = tmp("hist-host");
        let shared = hist::Shared::default();
        let p = AppProvider::new(d.clone()).with_history(shared.clone());
        let sid = [7u8; 16];
        let inp = |data: &str, line: Option<&str>, label: &str, ssh: bool| p.session_input(&Input { session_id: sid, data, line: line.map(str::to_owned), label: label.into(), cwd: Some(r"C:\proj".into()), ssh });
        // Celular manda tecla a tecla; o Enter vem com a linha da tela do host.
        for c in ["g", "i", "t", " ", "s"] { inp(c, None, "", false); }
        inp("\r", Some(r"PS C:\proj> git s"), "PowerShell", false);
        p.command_done(&sid, Some(2));
        inp("segredo\r", Some("Password:"), "PowerShell", false);
        inp("abc", None, "", false);
        inp("\r", Some("Enter passphrase"), "PowerShell", false);
        inp("oculto", None, "", false);
        inp("\r", Some(r"PS C:\proj>"), "PowerShell", false);
        inp(":q\r", None, "PowerShell", false);
        inp("ls\r", Some("$"), "srv", true);
        inp("oi\r", Some("> oi"), "api · claude", false);
        let all = hist::with_shared(&shared, &d, |s| s.entries().to_vec());
        assert_eq!(all.iter().map(|e| (e.cmd.as_str(), e.kind.as_str(), e.pc.as_str(), e.code)).collect::<Vec<_>>(),
            vec![("git s", "pwsh", "Este PC", Some(2)), ("ls", "ssh", "Este PC", None)], "{all:?}");
        assert_eq!(all[0].cwd.as_deref(), Some(r"C:\proj"));
        p.session_gone(&sid);
        p.command_done(&sid, Some(9));
        assert_eq!(hist::with_shared(&shared, &d, |s| s.entries()[1].code), None, "sessão encerrada não recebe código atrasado");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn padrao_do_agente_e_o_mesmo_arquivo_do_desktop() {
        let d = tmp("padrao");
        let p = AppProvider::new(d.clone());
        assert_eq!(p.agent_default(), None);
        p.agent_default_save(&AgentDefault { agent: "codex".into(), cwd: Some(r" C:\api ".into()), args: " --full-auto ".into() }).unwrap();
        assert_eq!(agents::read_default(&d), Some(agents::AiDefault { agent: "codex".into(), cwd: Some(r"C:\api".into()), args: "--full-auto".into() }), "a tela desktop lê o mesmo");
        assert_eq!(p.agent_default().map(|x| (x.agent, x.cwd, x.args)), Some(("codex".into(), Some(r"C:\api".into()), "--full-auto".into())));
        assert_eq!(p.agent_default_save(&AgentDefault { agent: "bash".into(), ..Default::default() }), Err("INVALID_MESSAGE".into()));
        assert_eq!(p.agent_default_save(&AgentDefault { agent: "claude".into(), args: "& calc".into(), cwd: None }), Err("INVALID_MESSAGE".into()));
        std::fs::write(d.join("agent-default.json"), r#"{"agent":"claude","args":"; calc"}"#).unwrap();
        assert_eq!(p.agent_default(), None, "arquivo editado à mão com args perigosos é ignorado");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn historico_busca_filtros_e_limpar() {
        let d = tmp("hist");
        let shared = hist::Shared::default();
        let now = crate::host::now_ms();
        let (a, b) = hist::with_shared(&shared, &d, |s| {
            let a = s.add_in(now - 10 * 86_400_000, "git status", "pwsh", "PowerShell", "Este PC", Some(r"C:\proj")).unwrap();
            let b = s.add(now - 1000, "cargo test", "ssh", "srv", "Este PC").unwrap();
            s.set_code(b, 101).unwrap();
            (a, b)
        });
        let p = AppProvider::new(d.clone()).with_history(shared.clone());
        let f = |query: &str, limit: usize| HistoryFilter { query: query.into(), limit, ..Default::default() };
        let all = p.history(&f("", 10));
        assert_eq!(all.iter().map(|e| (e.command.as_str(), e.exit_code)).collect::<Vec<_>>(), vec![("cargo test", Some(101)), ("git status", None)], "mais recente primeiro");
        assert_eq!((all[0].entry_id.clone(), all[0].kind.as_str(), all[0].label.as_str()), (b.to_string(), "ssh", "srv"));
        assert_eq!(all[1].cwd.as_deref(), Some(r"C:\proj"));
        assert_eq!(p.history(&f("GIT", 10)).len(), 1);
        assert_eq!(p.history(&f("", 1)).len(), 1);
        assert_eq!(p.history(&HistoryFilter { days: Some(7), ..f("", 10) }).len(), 1, "só os últimos 7 dias");
        assert_eq!(p.history(&HistoryFilter { kind: Some("pwsh".into()), ..f("", 10) })[0].command, "git status");
        assert_eq!(p.history(&HistoryFilter { errors: true, ..f("", 10) })[0].command, "cargo test");
        assert_eq!(p.history_command(&a.to_string()).as_deref(), Some("git status"));
        assert_eq!(p.history_command("123"), None);
        assert_eq!(p.history_command("abc"), None);
        // Resumo "Sessões": o mesmo da tela desktop (`hist::sessions`), uma linha por sessão e dia.
        let g = p.history_sessions(&f("", 1), 0);
        assert_eq!(g.iter().map(|g| (g.label.as_str(), g.count, g.errors)).collect::<Vec<_>>(), vec![("srv", 1, 1), ("PowerShell", 1, 0)], "ignora o limite da lista");
        p.history_clear().unwrap();
        assert!(hist::with_shared(&shared, &d, |s| s.search(&hist::Query::default(), now)).is_empty(), "a tela desktop vê o histórico limpo");
        let _ = std::fs::remove_dir_all(d);
    }

    /// Celular grava a aparência no mesmo arquivo da tela desktop e as janelas recebem o evento.
    #[test]
    fn settings_do_celular() {
        let d = tmp("aparencia");
        let seen: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Default::default();
        let s2 = seen.clone();
        let p = AppProvider::new(d.clone()).with_notify(Arc::new(move |e: &str, v| s2.lock().unwrap().push((e.into(), v))));
        assert_eq!(p.settings_get(), Some(Appearance::default()), "sem arquivo: padrão");
        let a = Appearance { theme: "cosmo".into(), blur: 12, ..Appearance::default() };
        p.settings_set(&a).unwrap();
        assert_eq!(settings::appearance(&d), a, "a tela desktop lê o mesmo valor");
        assert_eq!(seen.lock().unwrap()[0], ("appearance".to_string(), a.json()));
        p.agent_waiting(&[0x6b; 16], "api", true);
        assert_eq!(seen.lock().unwrap()[1].1["waiting"], true);
        assert!(p.settings_set(&Appearance { opacity: 10, ..a }).is_err(), "fora da faixa não grava");
        let _ = std::fs::remove_dir_all(d);
    }
}

//! Dados das telas do celular (Agentes, SSH, Histórico): o host (`live_cmd_echo::extras`) pede ao
//! provedor registrado aqui, que usa a mesma lógica e os mesmos arquivos das telas desktop
//! (`agents.rs`, `settings.rs`, `ssh.rs`, `hist.rs`) na pasta de dados do app.
//! Roda com o lock do hub tomado: só disco; o teste de alcance SSH vai para outra thread.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use live_cmd_echo::extras::{Agent, AgentState, HistoryEntry, HistoryFilter, Provider, SshServer};

use crate::{agents, hist, settings, ssh, term};

/// Latência guardada vale por este tempo; depois, um novo teste em segundo plano.
const FRESH: Duration = Duration::from_secs(30);
/// Teto do teste de alcance (só em segundo plano; a lista nunca espera por ele).
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Teste de alcance: latência em ms ou o código da falha (`DNS`, `TIMEOUT`, `OFFLINE`, …), como `ssh::probe`.
pub type Probe = Arc<dyn Fn(&str, u16) -> Result<u32, String> + Send + Sync>;

#[derive(Default)]
struct Lat { r: Option<Result<u32, String>>, at: Option<Instant>, busy: bool }

pub struct AppProvider {
    dir: PathBuf,
    probe: Probe,
    /// Por `(host, porta)`.
    lat: Arc<Mutex<HashMap<(String, u16), Lat>>>,
    /// O mesmo store da tela Histórico (ver `hist::Shared`).
    hist: hist::Shared,
}

impl AppProvider {
    pub fn new(dir: PathBuf) -> AppProvider {
        let probe: Probe = Arc::new(|h, p| {
            let v = ssh::probe(h, p, PROBE_TIMEOUT);
            v["ms"].as_u64().map(|m| m.min(u32::MAX as u64) as u32).ok_or_else(|| v["code"].as_str().unwrap_or("OFFLINE").to_owned())
        });
        AppProvider::with_probe(dir, probe)
    }

    pub fn with_probe(dir: PathBuf, probe: Probe) -> AppProvider { AppProvider { dir, probe, lat: Default::default(), hist: Default::default() } }

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
            state: AgentState::Saved, last_activity_ms: a.used_ms, profile_id: Some(a.profile_id),
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
            SshServer { latency_ms, status, server_id: s.id, label: s.name, host: s.host, user: s.user, port: s.port, key: s.key }
        }).collect()
    }

    fn ssh_target(&self, server_id: &str) -> Option<live_cmd_echo::ssh::Ssh> {
        settings::ssh_servers(&self.dir).ok()?.into_iter().find(|s| s.id == server_id).map(|s| s.target()).filter(|t| t.check().is_ok())
    }

    /// Mesma regra da tela desktop (`settings::save_ssh_server`): nome, destino validado, limite de 200.
    fn ssh_save(&self, server_id: Option<&str>, label: &str, t: &live_cmd_echo::ssh::Ssh) -> Result<String, String> {
        let s = settings::SshServer { id: server_id.unwrap_or_default().into(), name: label.into(), user: t.user.clone(), host: t.host.clone(), port: t.port, key: t.key.clone() };
        settings::save_ssh_server(&self.dir, s).map(|s| s.id)
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
        let a = Agent { session_id: sid, agent: "codex".into(), args: String::new(), label: "api · codex".into(), cwd: Some(r"C:\api".into()), state: AgentState::Running, last_activity_ms: 0, profile_id: Some("pwsh".into()) };
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
        let srv = |name: &str, user: &str| settings::SshServer { id: String::new(), name: name.into(), user: user.into(), host: "10.0.0.5".into(), port: 2222, key: None };
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
        settings::save_ssh_server(&d, settings::SshServer { id: String::new(), name: "x".into(), user: "u".into(), host: "nada.invalid".into(), port: 22, key: None }).unwrap();
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
        let t = |user: &str| live_cmd_echo::ssh::Ssh { user: user.into(), host: "h".into(), port: 22, key: Some(r"C:\k\id".into()) };
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
        p.history_clear().unwrap();
        assert!(hist::with_shared(&shared, &d, |s| s.search(&hist::Query::default(), now)).is_empty(), "a tela desktop vê o histórico limpo");
        let _ = std::fs::remove_dir_all(d);
    }
}

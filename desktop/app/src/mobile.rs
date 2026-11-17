//! Dados das telas do celular (Agentes, SSH, Histórico): o host (`live_cmd_echo::extras`) pede ao
//! provedor registrado aqui, que lê os mesmos arquivos das telas desktop na pasta de dados do app.
//! Roda com o lock do hub tomado: só leitura de disco; o teste de alcance SSH vai para outra thread.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use live_cmd_echo::extras::{Agent, AgentState, HistoryEntry, Provider, SshServer};

use crate::{agents, hist, settings, ssh, term};

/// Latência guardada vale por este tempo; depois, um novo teste em segundo plano.
const FRESH: Duration = Duration::from_secs(30);
/// Teto do teste de alcance (só em segundo plano; a lista nunca espera por ele).
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Teste de alcance: latência em ms ou `None` (fora do ar, DNS, tempo esgotado).
pub type Probe = Arc<dyn Fn(&str, u16) -> Option<u32> + Send + Sync>;

#[derive(Default)]
struct Lat { ms: Option<u32>, at: Option<Instant>, busy: bool }

pub struct AppProvider {
    dir: PathBuf,
    probe: Probe,
    /// Por `(host, porta)`.
    lat: Arc<Mutex<HashMap<(String, u16), Lat>>>,
}

impl AppProvider {
    pub fn new(dir: PathBuf) -> AppProvider {
        let probe: Probe = Arc::new(|h, p| ssh::probe(h, p, PROBE_TIMEOUT)["ms"].as_u64().map(|m| m.min(u32::MAX as u64) as u32));
        AppProvider::with_probe(dir, probe)
    }

    pub fn with_probe(dir: PathBuf, probe: Probe) -> AppProvider { AppProvider { dir, probe, lat: Default::default() } }

    /// Última latência conhecida; se velha (ou nunca medida), dispara um teste em outra thread.
    fn latency(&self, host: &str, port: u16) -> Option<u32> {
        let key = (host.to_owned(), port);
        let mut m = self.lat.lock().unwrap_or_else(|e| e.into_inner());
        let l = m.entry(key.clone()).or_default();
        if !l.busy && l.at.is_none_or(|t| t.elapsed() >= FRESH) {
            l.busy = true;
            let (lat, probe) = (self.lat.clone(), self.probe.clone());
            std::thread::spawn(move || {
                let ms = probe(&key.0, key.1);
                let mut m = lat.lock().unwrap_or_else(|e| e.into_inner());
                m.insert(key, Lat { ms, at: Some(Instant::now()), busy: false });
            });
        }
        l.ms
    }

    fn history_store(&self) -> hist::Store { hist::Store::open(self.dir.join(hist::FILE), hist::MAX) }
}

/// `xxxxxxxx-xxxx-…` em 16 bytes.
fn uuid_bytes(s: &str) -> Option<[u8; 16]> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if s.len() != 36 || hex.len() != 32 { return None; }
    let v: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

impl Provider for AppProvider {
    /// Registro da tela Agentes (`agents.json`), só das sessões deste PC (conexão local).
    fn saved_agents(&self) -> Vec<Agent> {
        agents::read(&self.dir).into_iter().filter(|a| a.conn == term::LOCAL).filter_map(|a| Some(Agent {
            session_id: uuid_bytes(&a.session_id)?, agent: a.agent, args: a.args, label: a.label, cwd: a.cwd,
            state: AgentState::Saved, last_activity_ms: a.used_ms, profile_id: Some(a.profile_id),
        })).collect()
    }

    fn ssh_servers(&self) -> Vec<SshServer> {
        settings::ssh_servers(&self.dir).unwrap_or_default().into_iter().map(|s| SshServer {
            latency_ms: self.latency(&s.host, s.port), server_id: s.id, label: s.name, host: s.host, user: s.user, port: s.port,
        }).collect()
    }

    fn ssh_target(&self, server_id: &str) -> Option<live_cmd_echo::ssh::Ssh> {
        settings::ssh_servers(&self.dir).ok()?.into_iter().find(|s| s.id == server_id).map(|s| s.target()).filter(|t| t.check().is_ok())
    }

    fn history(&self, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let q = hist::Query { q: query.to_owned(), limit: Some(limit), ..Default::default() };
        self.history_store().search(&q, crate::host::now_ms()).into_iter().map(|e| HistoryEntry {
            entry_id: e.id.to_string(), command: e.cmd, cwd: None, at_ms: e.ts, exit_code: e.code.and_then(|c| i32::try_from(c).ok()),
        }).collect()
    }

    fn history_command(&self, entry_id: &str) -> Option<String> {
        self.history_store().get(entry_id.parse().ok()?).map(|e| e.cmd.clone())
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
    fn ssh_lista_sem_esperar_o_teste_e_destino_validado() {
        let d = tmp("ssh");
        let srv = |name: &str, user: &str| settings::SshServer { id: String::new(), name: name.into(), user: user.into(), host: "10.0.0.5".into(), port: 2222, key: None };
        let s = settings::save_ssh_server(&d, srv("prod", "deploy")).unwrap();
        let calls = Arc::new(Mutex::new(0));
        let c = calls.clone();
        let p = AppProvider::with_probe(d.clone(), Arc::new(move |h, port| { assert_eq!((h, port), ("10.0.0.5", 2222)); *c.lock().unwrap() += 1; Some(7) }));
        let l = p.ssh_servers();
        assert_eq!((l[0].server_id.as_str(), l[0].label.as_str(), l[0].user.as_str(), l[0].port), (s.id.as_str(), "prod", "deploy", 2222));
        let t = Instant::now();
        while p.ssh_servers()[0].latency_ms.is_none() { assert!(t.elapsed() < Duration::from_secs(2), "teste em segundo plano não voltou"); std::thread::sleep(Duration::from_millis(5)); }
        assert_eq!(p.ssh_servers()[0].latency_ms, Some(7));
        assert_eq!(*calls.lock().unwrap(), 1, "latência recente fica em cache");
        let t = p.ssh_target(&s.id).unwrap();
        assert_eq!((t.user.as_str(), t.host.as_str(), t.port), ("deploy", "10.0.0.5", 2222));
        assert_eq!(p.ssh_target("nao-existe"), None);
        // Arquivo editado à mão com destino inválido: não conecta.
        std::fs::write(d.join("ssh.json"), r#"[{"id":"x","name":"x","user":"-oProxyCommand=calc","host":"h","port":22}]"#).unwrap();
        assert_eq!(p.ssh_target("x"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn historico_busca_e_comando() {
        let d = tmp("hist");
        let mut s = hist::Store::open(d.join(hist::FILE), hist::MAX);
        let a = s.add(1000, "git status", "pwsh", "PowerShell", "Este PC").unwrap();
        let b = s.add(2000, "cargo test", "pwsh", "PowerShell", "Este PC").unwrap();
        s.set_code(b, 101).unwrap();
        let p = AppProvider::new(d.clone());
        let all = p.history("", 10);
        assert_eq!(all.iter().map(|e| (e.command.as_str(), e.exit_code)).collect::<Vec<_>>(), vec![("cargo test", Some(101)), ("git status", None)], "mais recente primeiro");
        assert_eq!((all[0].entry_id.clone(), all[0].at_ms), (b.to_string(), 2000));
        assert_eq!(p.history("GIT", 10).len(), 1);
        assert_eq!(p.history("", 1).len(), 1);
        assert_eq!(p.history_command(&a.to_string()).as_deref(), Some("git status"));
        assert_eq!(p.history_command("123"), None);
        assert_eq!(p.history_command("abc"), None);
        let _ = std::fs::remove_dir_all(d);
    }
}

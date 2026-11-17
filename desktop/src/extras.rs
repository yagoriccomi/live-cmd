//! Controles extras do `pty-channel` para as telas do celular: Agentes, SSH e Histórico.
//! Cada pedido e resposta leva `api` (hoje 1). `api` numérico diferente vira `VERSION_UNSUPPORTED`
//! com `request_id` (não fatal), para o app poder evoluir sem derrubar a conexão.
//!
//! Os dados vêm de um [`Provider`]. Sem provedor registrado ([`set_provider`]), SSH e Histórico
//! respondem listas vazias e Agentes mostra só as sessões vivas com agente (lidas do hub em `lib.rs`).
#![allow(dead_code)]
use crate::auth::{uuid_v4_lower, Error::{self, *}};
use serde_json::{json, Map, Value};
use std::sync::{Arc, LazyLock, RwLock};

type R<T> = Result<T, Error>;
pub const API: u64 = 1;
/// Teto de itens por resposta e de bytes do corpo (a mensagem inteira não passa de 65536).
pub const MAX_ITEMS: usize = 200;
const MAX_BODY: usize = 60_000;
const MAX_TEXT: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    AgentList,
    AgentResume { session_id: [u8; 16], cols: u16, rows: u16 },
    SshList,
    SshConnect { server_id: String, cols: u16, rows: u16 },
    HistoryQuery { query: String, limit: usize },
    HistoryRun { entry_id: String, session_id: [u8; 16] },
    /// `api` numérico diferente de 1: o host responde `VERSION_UNSUPPORTED` com `request_id`.
    Unsupported,
}

/// `None`: o tipo não é extra. `Some(Err)`: é extra, mas inválido.
pub fn parse(ty: &str, o: &Map<String, Value>) -> Option<R<Req>> {
    let known = ["agent_list", "agent_resume", "ssh_list", "ssh_connect", "history_query", "history_run"];
    if !known.contains(&ty) { return None; }
    Some(parse_known(ty, o))
}

fn parse_known(ty: &str, o: &Map<String, Value>) -> R<Req> {
    let fields = |req: &[&str], opt: &[&str]| -> R<()> {
        let ok = req.iter().all(|k| o.contains_key(*k))
            && o.keys().all(|k| ["v", "type", "connection_id", "request_id", "api"].contains(&k.as_str()) || req.contains(&k.as_str()) || opt.contains(&k.as_str()));
        if ok { Ok(()) } else { Err(InvalidMessage) }
    };
    match o.get("api") { Some(v) if v.as_u64() == Some(API) => {} Some(Value::Number(_)) => return Ok(Req::Unsupported), _ => return Err(InvalidMessage) }
    let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or(InvalidMessage);
    let id = |k: &str| s(k).and_then(|x| if (1..=64).contains(&x.len()) && x.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)) { Ok(x.to_owned()) } else { Err(InvalidMessage) });
    let sid = |k: &str| uuid_v4_lower(s(k)?).ok_or(InvalidMessage);
    let dim = |k: &str| o.get(k).and_then(Value::as_u64).filter(|n| (1..=1000).contains(n)).map(|n| n as u16).ok_or(InvalidMessage);
    Ok(match ty {
        "agent_list" => { fields(&[], &[])?; Req::AgentList }
        "agent_resume" => { fields(&["session_id", "cols", "rows"], &[])?; Req::AgentResume { session_id: sid("session_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "ssh_list" => { fields(&[], &[])?; Req::SshList }
        "ssh_connect" => { fields(&["server_id", "cols", "rows"], &[])?; Req::SshConnect { server_id: id("server_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "history_query" => {
            fields(&[], &["query", "limit"])?;
            let query = o.get("query").map(|_| s("query")).transpose()?.unwrap_or("").to_owned();
            if query.chars().count() > 256 { return Err(InvalidMessage); }
            let limit = match o.get("limit") { None => 50, Some(v) => v.as_u64().filter(|n| (1..=MAX_ITEMS as u64).contains(n)).ok_or(InvalidMessage)? as usize };
            Req::HistoryQuery { query, limit }
        }
        "history_run" => { fields(&["entry_id", "session_id"], &[])?; Req::HistoryRun { entry_id: id("entry_id")?, session_id: sid("session_id")? } }
        _ => return Err(InvalidMessage),
    })
}

// ---- dados ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentState { Running, Exited, Saved }
/// Sessão de IA. `session_id` é também o id da conversa (`--resume <id>`).
/// `profile_id`: perfil em que a sessão foi criada (retomar usa o mesmo; ausente ou apagado = o primeiro).
#[derive(Debug, Clone, PartialEq)]
pub struct Agent { pub session_id: [u8; 16], pub agent: String, pub args: String, pub label: String, pub cwd: Option<String>, pub state: AgentState, pub last_activity_ms: u64, pub profile_id: Option<String> }
#[derive(Debug, Clone, PartialEq)]
pub struct SshServer { pub server_id: String, pub label: String, pub host: String, pub user: String, pub port: u16, pub latency_ms: Option<u32> }
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry { pub entry_id: String, pub command: String, pub cwd: Option<String>, pub at_ms: u64, pub exit_code: Option<i32> }

/// Ponto de extensão: as fatias desktop (Agentes, SSH, Histórico) implementam e registram com [`set_provider`].
/// Todo método tem padrão vazio; o host já soma as sessões vivas com agente por conta própria.
pub trait Provider: Send + Sync {
    /// Agentes gravados que não estão vivos agora (ex.: fechados, de outra execução). Retomar abre sessão com o mesmo id.
    fn saved_agents(&self) -> Vec<Agent> { vec![] }
    fn ssh_servers(&self) -> Vec<SshServer> { vec![] }
    /// Destino do servidor para abrir a sessão com o `ssh.exe` direto (sem shell). O hub confere `check()` de novo.
    fn ssh_target(&self, _server_id: &str) -> Option<crate::ssh::Ssh> { None }
    fn history(&self, _query: &str, _limit: usize) -> Vec<HistoryEntry> { vec![] }
    fn history_command(&self, _entry_id: &str) -> Option<String> { None }
}
struct Empty;
impl Provider for Empty {}
static PROVIDER: LazyLock<RwLock<Arc<dyn Provider>>> = LazyLock::new(|| RwLock::new(Arc::new(Empty)));
pub fn set_provider(p: Arc<dyn Provider>) { *PROVIDER.write().unwrap_or_else(|e| e.into_inner()) = p; }
pub fn provider() -> Arc<dyn Provider> { PROVIDER.read().unwrap_or_else(|e| e.into_inner()).clone() }

// ---- corpos de resposta ----

fn clip(s: &str) -> String { if s.chars().count() <= MAX_TEXT { s.into() } else { s.chars().take(MAX_TEXT - 1).chain(['…']).collect() } }
/// Corta a lista até o corpo caber numa mensagem.
fn fit(ty: &str, key: &str, mut items: Vec<Value>) -> Value {
    items.truncate(MAX_ITEMS);
    loop {
        let body = json!({"type": ty, "api": API, key: items});
        if body.to_string().len() <= MAX_BODY || items.is_empty() { return body; }
        items.pop();
    }
}
pub fn agent_list_ok(agents: &[Agent]) -> Value {
    fit("agent_list_ok", "agents", agents.iter().map(|a| json!({
        "session_id": crate::proto::uuid_str(&a.session_id), "agent": a.agent, "label": clip(&a.label), "cwd": a.cwd.as_deref().map(clip),
        "state": match a.state { AgentState::Running => "running", AgentState::Exited => "exited", AgentState::Saved => "saved" },
        "last_activity_ms": crate::proto::u64s(a.last_activity_ms),
    })).collect())
}
pub fn ssh_list_ok(servers: &[SshServer]) -> Value {
    fit("ssh_list_ok", "servers", servers.iter().map(|s| json!({
        "server_id": s.server_id, "label": clip(&s.label), "host": clip(&s.host), "user": clip(&s.user), "port": s.port, "latency_ms": s.latency_ms,
    })).collect())
}
pub fn history_query_ok(entries: &[HistoryEntry]) -> Value {
    fit("history_query_ok", "entries", entries.iter().map(|e| json!({
        "entry_id": e.entry_id, "command": clip(&e.command), "cwd": e.cwd.as_deref().map(clip), "at_ms": crate::proto::u64s(e.at_ms), "exit_code": e.exit_code,
    })).collect())
}
pub fn session_ok(ty: &str, sid: &[u8; 16]) -> Value { json!({"type": ty, "api": API, "session_id": crate::proto::uuid_str(sid)}) }
pub fn history_run_ok() -> Value { json!({"type": "history_run_ok", "api": API}) }

/// Une as sessões vivas com agente e as gravadas do provedor (viva vence), mais recente primeiro.
pub fn merge_agents(live: Vec<Agent>, saved: Vec<Agent>) -> Vec<Agent> {
    let mut all = live;
    for s in saved { if !all.iter().any(|a| a.session_id == s.session_id) { all.push(s); } }
    all.sort_by(|a, b| b.last_activity_ms.cmp(&a.last_activity_ms));
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{encode_control, parse_control, Control, Req as P};

    const CID: &str = "9b2f6c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d";
    const RID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
    const SID: &str = "0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e";
    fn res<T>(r: R<T>) -> Value { match r { Ok(_) => json!("ok"), Err(e) => json!(e.code()) } }
    fn sid() -> [u8; 16] { uuid_v4_lower(SID).unwrap() }

    /// Pedidos que o app manda. Os `ok` são exatamente o que `mobile/src/extras.ts` monta (conferido no teste TS).
    fn requests() -> Vec<(&'static str, Value)> {
        vec![
            ("agent_list", json!({"type":"agent_list","api":1})),
            ("agent_resume", json!({"type":"agent_resume","api":1,"session_id":SID,"cols":80,"rows":24})),
            ("ssh_list", json!({"type":"ssh_list","api":1})),
            ("ssh_connect", json!({"type":"ssh_connect","api":1,"server_id":"srv-1","cols":80,"rows":24})),
            ("history_query", json!({"type":"history_query","api":1,"query":"git","limit":50})),
            ("history_query_vazio", json!({"type":"history_query","api":1})),
            ("history_run", json!({"type":"history_run","api":1,"entry_id":"h_42","session_id":SID})),
            ("api_2", json!({"type":"agent_list","api":2})),
            ("sem_api", json!({"type":"agent_list"})),
            ("api_string", json!({"type":"agent_list","api":"1"})),
            ("campo_desconhecido", json!({"type":"ssh_list","api":1,"x":1})),
            ("resume_sid_maiusculo", json!({"type":"agent_resume","api":1,"session_id":SID.to_uppercase(),"cols":80,"rows":24})),
            ("resume_cols_0", json!({"type":"agent_resume","api":1,"session_id":SID,"cols":0,"rows":24})),
            ("connect_id_vazio", json!({"type":"ssh_connect","api":1,"server_id":"","cols":80,"rows":24})),
            ("connect_id_espaco", json!({"type":"ssh_connect","api":1,"server_id":"a b","cols":80,"rows":24})),
            ("query_limit_0", json!({"type":"history_query","api":1,"limit":0})),
            ("query_limit_201", json!({"type":"history_query","api":1,"limit":201})),
            ("query_257", json!({"type":"history_query","api":1,"query":"x".repeat(257)})),
            ("run_sem_sessao", json!({"type":"history_run","api":1,"entry_id":"h_42"})),
        ]
    }

    fn agents() -> Vec<Agent> {
        vec![
            Agent { session_id: sid(), agent: "claude".into(), args: String::new(), label: "api".into(), cwd: Some("C:/proj/api".into()), state: AgentState::Running, last_activity_ms: 1_700_000_000_000, profile_id: None },
            Agent { session_id: uuid_v4_lower("2f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").unwrap(), agent: "codex".into(), args: String::new(), label: String::new(), cwd: None, state: AgentState::Saved, last_activity_ms: 5, profile_id: None },
        ]
    }

    #[test]
    fn pedidos() {
        let wrap = |mut v: Value| { v["v"] = 1.into(); v["connection_id"] = CID.into(); v["request_id"] = RID.into(); v.to_string() };
        let ok = ["agent_list", "agent_resume", "ssh_list", "ssh_connect", "history_query", "history_query_vazio", "history_run", "api_2"];
        let mut cases = vec![];
        for (name, body) in requests() {
            let r = parse_control(&wrap(body.clone()), CID);
            assert_eq!(res(r.clone()), if ok.contains(&name) { "ok" } else { "INVALID_MESSAGE" }, "{name}");
            cases.push(json!({"name": name, "body": body, "result": res(r)}));
        }
        let get = |n: &str| { let b = requests().into_iter().find(|c| c.0 == n).unwrap().1; let Ok(Control::Request { req: P::Extra(r), .. }) = parse_control(&wrap(b), CID) else { panic!("{n}") }; r };
        assert_eq!(get("api_2"), Req::Unsupported);
        assert_eq!(get("history_query_vazio"), Req::HistoryQuery { query: String::new(), limit: 50 });
        assert_eq!(get("ssh_connect"), Req::SshConnect { server_id: "srv-1".into(), cols: 80, rows: 24 });
        assert_eq!(get("history_run"), Req::HistoryRun { entry_id: "h_42".into(), session_id: sid() });
        fixture(json!({"connection_id": CID, "requests": cases, "replies": replies()}));
    }

    /// Respostas do host (construtores reais) e mutações; resultado esperado conferido no app.
    fn replies() -> Vec<Value> {
        let wrap = |b: Value| encode_control(CID, Some(RID), b);
        let with = |mut v: Value, path: &[&str], x: Value| { { let mut p = &mut v; for k in &path[..path.len() - 1] { p = if let Ok(i) = k.parse::<usize>() { &mut p[i] } else { &mut p[*k] }; } p[path[path.len() - 1]] = x; } v };
        let al = agent_list_ok(&agents());
        let sl = ssh_list_ok(&[SshServer { server_id: "srv-1".into(), label: "Produção".into(), host: "10.0.0.5".into(), user: "deploy".into(), port: 22, latency_ms: Some(38) },
            SshServer { server_id: "srv-2".into(), label: "NAS".into(), host: "nas.local".into(), user: "admin".into(), port: 2222, latency_ms: None }]);
        let hq = history_query_ok(&[HistoryEntry { entry_id: "h_42".into(), command: "git status".into(), cwd: Some("C:/proj".into()), at_ms: 1_700_000_000_000, exit_code: Some(0) },
            HistoryEntry { entry_id: "h_43".into(), command: "npm test".into(), cwd: None, at_ms: 7, exit_code: None }]);
        let cases: Vec<(&str, String, &str)> = vec![
            ("agent_list_ok", wrap(al.clone()), "ok"),
            ("agent_list_vazia", wrap(agent_list_ok(&[])), "ok"),
            ("agent_resume_ok", wrap(session_ok("agent_resume_ok", &sid())), "ok"),
            ("ssh_list_ok", wrap(sl.clone()), "ok"),
            ("ssh_list_vazia", wrap(ssh_list_ok(&[])), "ok"),
            ("ssh_connect_ok", wrap(session_ok("ssh_connect_ok", &sid())), "ok"),
            ("history_query_ok", wrap(hq.clone()), "ok"),
            ("history_query_vazia", wrap(history_query_ok(&[])), "ok"),
            ("history_run_ok", wrap(history_run_ok()), "ok"),
            ("api_2", wrap(with(history_run_ok(), &["api"], 2.into())), "VERSION_UNSUPPORTED"),
            ("sem_api", wrap(json!({"type":"history_run_ok"})), "INVALID_MESSAGE"),
            ("sem_request_id", encode_control(CID, None, history_run_ok()), "INVALID_MESSAGE"),
            ("agente_state_invalido", wrap(with(al.clone(), &["agents", "0", "state"], "zombie".into())), "INVALID_MESSAGE"),
            ("agente_atividade_numero", wrap(with(al.clone(), &["agents", "0", "last_activity_ms"], 1.into())), "INVALID_MESSAGE"),
            ("agente_campo_extra", wrap(with(al.clone(), &["agents", "0", "x"], 1.into())), "INVALID_MESSAGE"),
            ("ssh_porta_0", wrap(with(sl.clone(), &["servers", "0", "port"], 0.into())), "INVALID_MESSAGE"),
            ("ssh_latencia_texto", wrap(with(sl.clone(), &["servers", "0", "latency_ms"], "38".into())), "INVALID_MESSAGE"),
            ("historico_sem_command", wrap({ let mut v = hq.clone(); v["entries"][0].as_object_mut().unwrap().remove("command"); v }), "INVALID_MESSAGE"),
            ("resume_sid_maiusculo", wrap(json!({"type":"agent_resume_ok","api":1,"session_id":SID.to_uppercase()})), "INVALID_MESSAGE"),
        ];
        cases.into_iter().map(|(n, t, r)| { assert!(t.len() <= 65536); json!({"name": n, "text": t, "result": r}) }).collect()
    }

    #[test]
    fn corpo_cabe_numa_mensagem() {
        let big: Vec<HistoryEntry> = (0..200).map(|i| HistoryEntry { entry_id: format!("h{i}"), command: "x".repeat(2000), cwd: Some("y".repeat(2000)), at_ms: i, exit_code: None }).collect();
        let v = history_query_ok(&big);
        assert!(v.to_string().len() <= MAX_BODY);
        let n = v["entries"].as_array().unwrap().len();
        assert!(n > 0 && n < 200);
        assert_eq!(v["entries"][0]["command"].as_str().unwrap().chars().count(), MAX_TEXT);
    }

    #[test]
    fn merge_viva_vence_e_ordena() {
        let mut saved = agents();
        saved[0].state = AgentState::Saved;
        let live = vec![agents()[0].clone()];
        let m = merge_agents(live, saved);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].state, AgentState::Running);
        assert_eq!(m[1].agent, "codex");
    }

    #[test]
    fn provedor_padrao_vazio() {
        let p = Empty;
        assert!(p.ssh_servers().is_empty() && p.history("", 10).is_empty() && p.saved_agents().is_empty());
        assert_eq!(p.ssh_target("srv-1"), None);
    }

    fn fixture(v: Value) {
        let path = format!("{}/tests/fixtures/c3-extras.json", env!("CARGO_MANIFEST_DIR"));
        if std::env::var_os("LIVE_CMD_WRITE_FIXTURE").is_some() { std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap(); }
        let f: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f, v, "c3-extras.json desatualizada: rode com LIVE_CMD_WRITE_FIXTURE=1");
    }
}

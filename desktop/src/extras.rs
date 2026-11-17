//! Controles extras do `pty-channel` para as telas do celular: Agentes, SSH e Histórico.
//! Cada pedido e resposta leva `api` (hoje 1). `api` numérico diferente vira `VERSION_UNSUPPORTED`
//! com `request_id` (não fatal), para o app poder evoluir sem derrubar a conexão.
//!
//! O host anuncia as telas que entende em `auth_ok.caps` ([`CAPS`]); host antigo não manda `caps`
//! e o celular mostra "atualize o live-cmd no PC" em vez de mandar pedidos que ele não conhece.
//!
//! Os dados vêm de um [`Provider`]. Sem provedor registrado ([`set_provider`]), SSH e Histórico
//! respondem listas vazias e Agentes mostra só as sessões vivas com agente (lidas do hub em `lib.rs`).
#![allow(dead_code)]
use crate::auth::{uuid_v4_lower, Error::{self, *}};
use serde_json::{json, Map, Value};
use std::sync::{Arc, LazyLock, RwLock};

type R<T> = Result<T, Error>;
pub const API: u64 = 1;
/// Telas anunciadas em `auth_ok.caps`: cada uma cobre todos os pedidos `api: 1` dela.
pub const CAPS: [&str; 3] = ["agents", "ssh", "history"];
/// Teto de itens por resposta e de bytes do corpo (a mensagem inteira não passa de 65536).
pub const MAX_ITEMS: usize = 200;
const MAX_BODY: usize = 60_000;
const MAX_TEXT: usize = 512;
/// Tipos de sessão do histórico (`hist.rs` no app).
pub const HISTORY_KINDS: [&str; 5] = ["pwsh", "cmd", "wsl", "ssh", "outro"];

/// Filtros do histórico, os mesmos da tela desktop.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryFilter { pub query: String, pub limit: usize, pub days: Option<u32>, pub kind: Option<String>, pub errors: bool }

#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    AgentList,
    AgentResume { session_id: [u8; 16], cols: u16, rows: u16 },
    /// Nova sessão de agente: o host escolhe o primeiro perfil, como o desktop.
    AgentCreate { agent: String, args: String, cwd: Option<String>, label: Option<String>, cols: u16, rows: u16 },
    /// Encerra o terminal (se aberto) e tira do registro.
    AgentRemove { session_id: [u8; 16] },
    SshList,
    SshConnect { server_id: String, cols: u16, rows: u16 },
    /// Cadastro sem senha: só destino (`Ssh::check` já passou) e caminho da chave.
    SshSave { server_id: Option<String>, label: String, target: crate::ssh::Ssh },
    SshRemove { server_id: String },
    /// Dispara o teste de alcance em segundo plano; o resultado vem no próximo `ssh_list`.
    SshProbe { server_id: String },
    /// Sessão SSH desconectada (restaurada ou encerrada): abre o ssh.exe de novo com o mesmo id.
    SshReconnect { session_id: [u8; 16], cols: u16, rows: u16 },
    HistoryQuery(HistoryFilter),
    HistoryRun { entry_id: String, session_id: [u8; 16] },
    HistoryClear,
    /// `api` numérico diferente de 1: o host responde `VERSION_UNSUPPORTED` com `request_id`.
    Unsupported,
}

const KNOWN: [&str; 13] = ["agent_list", "agent_resume", "agent_create", "agent_remove", "ssh_list", "ssh_connect", "ssh_save", "ssh_remove",
    "ssh_probe", "ssh_reconnect", "history_query", "history_run", "history_clear"];

/// `None`: o tipo não é extra. `Some(Err)`: é extra, mas inválido.
pub fn parse(ty: &str, o: &Map<String, Value>) -> Option<R<Req>> {
    if !KNOWN.contains(&ty) { return None; }
    Some(parse_known(ty, o))
}

/// Texto curto de UI: até `max` caracteres, sem caracteres de controle.
fn text(x: &str, min: usize, max: usize) -> R<String> {
    let n = x.chars().count();
    if n < min || n > max || x.chars().any(char::is_control) { return Err(InvalidMessage); }
    Ok(x.to_owned())
}

fn parse_known(ty: &str, o: &Map<String, Value>) -> R<Req> {
    let fields = |req: &[&str], opt: &[&str]| -> R<()> {
        let ok = req.iter().all(|k| o.contains_key(*k))
            && o.keys().all(|k| ["v", "type", "connection_id", "request_id", "api"].contains(&k.as_str()) || req.contains(&k.as_str()) || opt.contains(&k.as_str()));
        if ok { Ok(()) } else { Err(InvalidMessage) }
    };
    match o.get("api") { Some(v) if v.as_u64() == Some(API) => {} Some(Value::Number(_)) => return Ok(Req::Unsupported), _ => return Err(InvalidMessage) }
    let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or(InvalidMessage);
    let opt_s = |k: &str| o.get(k).map(|_| s(k)).transpose();
    let id = |k: &str| s(k).and_then(|x| if (1..=64).contains(&x.len()) && x.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)) { Ok(x.to_owned()) } else { Err(InvalidMessage) });
    let sid = |k: &str| uuid_v4_lower(s(k)?).ok_or(InvalidMessage);
    let dim = |k: &str| o.get(k).and_then(Value::as_u64).filter(|n| (1..=1000).contains(n)).map(|n| n as u16).ok_or(InvalidMessage);
    Ok(match ty {
        "agent_list" => { fields(&[], &[])?; Req::AgentList }
        "agent_resume" => { fields(&["session_id", "cols", "rows"], &[])?; Req::AgentResume { session_id: sid("session_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "agent_create" => {
            fields(&["agent", "cols", "rows"], &["args", "cwd", "label"])?;
            let agent = s("agent")?;
            if !crate::proto::AGENTS.contains(&agent) { return Err(InvalidMessage); }
            let args = opt_s("args")?.unwrap_or("").trim().to_owned();
            if !crate::proto::valid_args(&args) { return Err(InvalidMessage); }
            // Pasta: o hub ainda exige caminho absoluto que exista (NOT_FOUND se não).
            let cwd = opt_s("cwd")?.map(|c| text(c.trim(), 1, 1024)).transpose()?;
            let label = opt_s("label")?.map(|l| text(l, 0, 64)).transpose()?.filter(|l| !l.is_empty());
            Req::AgentCreate { agent: agent.to_owned(), args, cwd, label, cols: dim("cols")?, rows: dim("rows")? }
        }
        "agent_remove" => { fields(&["session_id"], &[])?; Req::AgentRemove { session_id: sid("session_id")? } }
        "ssh_list" => { fields(&[], &[])?; Req::SshList }
        "ssh_connect" => { fields(&["server_id", "cols", "rows"], &[])?; Req::SshConnect { server_id: id("server_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "ssh_save" => {
            // Sem campo de senha: `password` (ou qualquer outro) é campo desconhecido.
            fields(&["label", "user", "host", "port"], &["server_id", "key"])?;
            let server_id = o.get("server_id").map(|_| id("server_id")).transpose()?;
            let label = text(s("label")?.trim(), 1, 40)?;
            let port = o.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p)).ok_or(InvalidMessage)? as u16;
            let key = match o.get("key") { None | Some(Value::Null) => None, Some(_) => Some(s("key")?.trim().to_owned()).filter(|k| !k.is_empty()) };
            let target = crate::ssh::Ssh { user: s("user")?.trim().to_owned(), host: s("host")?.trim().to_owned(), port, key };
            target.check().map_err(|_| InvalidMessage)?;
            Req::SshSave { server_id, label, target }
        }
        "ssh_remove" => { fields(&["server_id"], &[])?; Req::SshRemove { server_id: id("server_id")? } }
        "ssh_probe" => { fields(&["server_id"], &[])?; Req::SshProbe { server_id: id("server_id")? } }
        "ssh_reconnect" => { fields(&["session_id", "cols", "rows"], &[])?; Req::SshReconnect { session_id: sid("session_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "history_query" => {
            fields(&[], &["query", "limit", "days", "kind", "errors"])?;
            let query = opt_s("query")?.unwrap_or("").to_owned();
            if query.chars().count() > 256 { return Err(InvalidMessage); }
            let limit = match o.get("limit") { None => 50, Some(v) => v.as_u64().filter(|n| (1..=MAX_ITEMS as u64).contains(n)).ok_or(InvalidMessage)? as usize };
            let days = match o.get("days") { None | Some(Value::Null) => None, Some(v) => Some(v.as_u64().filter(|n| (1..=3650).contains(n)).ok_or(InvalidMessage)? as u32) };
            let kind = match o.get("kind") { None | Some(Value::Null) => None, Some(_) => Some(s("kind")?).filter(|k| HISTORY_KINDS.contains(k)).map(str::to_owned).ok_or(InvalidMessage).map(Some)? };
            let errors = match o.get("errors") { None => false, Some(v) => v.as_bool().ok_or(InvalidMessage)? };
            Req::HistoryQuery(HistoryFilter { query, limit, days, kind, errors })
        }
        "history_run" => { fields(&["entry_id", "session_id"], &[])?; Req::HistoryRun { entry_id: id("entry_id")?, session_id: sid("session_id")? } }
        "history_clear" => { fields(&[], &[])?; Req::HistoryClear }
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
/// `status`: alcance TCP como na tela desktop: `unknown`, `testing`, `ok`, `dns`, `timeout` ou `offline`.
#[derive(Debug, Clone, PartialEq)]
pub struct SshServer { pub server_id: String, pub label: String, pub host: String, pub user: String, pub port: u16, pub key: Option<String>, pub latency_ms: Option<u32>, pub status: &'static str }
/// Sessão SSH do hub: `connected = false` quando o ssh.exe saiu ou a sessão foi restaurada do disco.
#[derive(Debug, Clone, PartialEq)]
pub struct SshSession { pub session_id: [u8; 16], pub label: String, pub connected: bool }
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry { pub entry_id: String, pub command: String, pub cwd: Option<String>, pub at_ms: u64, pub exit_code: Option<i32>, pub kind: String, pub label: String, pub pc: String }

pub const SSH_STATUS: [&str; 6] = ["unknown", "testing", "ok", "dns", "timeout", "offline"];

/// Ponto de extensão: as fatias desktop (Agentes, SSH, Histórico) implementam e registram com [`set_provider`].
/// Todo método tem padrão vazio; o host já soma as sessões vivas com agente por conta própria.
/// Roda com o lock do hub tomado: nada de rede nem espera longa aqui.
/// Erros são códigos do protocolo (`NOT_FOUND`, `LIMIT_EXCEEDED`, `IO_ERROR`…); outro texto vira `INVALID_MESSAGE`.
pub trait Provider: Send + Sync {
    /// Agentes gravados que não estão vivos agora (ex.: fechados, de outra execução). Retomar abre sessão com o mesmo id.
    fn saved_agents(&self) -> Vec<Agent> { vec![] }
    /// Sessão de agente criada ou retomada neste host (por qualquer cliente): grava no registro e marca o uso.
    fn agent_started(&self, _a: &Agent) {}
    /// Agente removido pelo cliente: sai do registro.
    fn agent_removed(&self, _session_id: &[u8; 16]) {}
    fn ssh_servers(&self) -> Vec<SshServer> { vec![] }
    /// Destino do servidor para abrir a sessão com o `ssh.exe` direto (sem shell). O hub confere `check()` de novo.
    fn ssh_target(&self, _server_id: &str) -> Option<crate::ssh::Ssh> { None }
    /// Grava (id `None` = novo) e devolve o id. O destino já passou por `Ssh::check`.
    fn ssh_save(&self, _server_id: Option<&str>, _label: &str, _target: &crate::ssh::Ssh) -> Result<String, String> { Err("NOT_FOUND".into()) }
    fn ssh_remove(&self, _server_id: &str) -> Result<(), String> { Err("NOT_FOUND".into()) }
    /// Novo teste de alcance em segundo plano (não espera).
    fn ssh_probe(&self, _server_id: &str) -> Result<(), String> { Err("NOT_FOUND".into()) }
    fn history(&self, _f: &HistoryFilter) -> Vec<HistoryEntry> { vec![] }
    fn history_command(&self, _entry_id: &str) -> Option<String> { None }
    fn history_clear(&self) -> Result<(), String> { Ok(()) }
}
struct Empty;
impl Provider for Empty {}
static PROVIDER: LazyLock<RwLock<Arc<dyn Provider>>> = LazyLock::new(|| RwLock::new(Arc::new(Empty)));
pub fn set_provider(p: Arc<dyn Provider>) { *PROVIDER.write().unwrap_or_else(|e| e.into_inner()) = p; }
pub fn provider() -> Arc<dyn Provider> { PROVIDER.read().unwrap_or_else(|e| e.into_inner()).clone() }
/// Código do provedor para o erro do protocolo.
pub fn code(c: &str) -> Error { Error::from_code(c).unwrap_or(InvalidMessage) }

// ---- corpos de resposta ----

fn clip(s: &str) -> String { if s.chars().count() <= MAX_TEXT { s.into() } else { s.chars().take(MAX_TEXT - 1).chain(['…']).collect() } }
/// Corta a lista até o corpo caber numa mensagem.
fn fit(ty: &str, key: &str, mut items: Vec<Value>, extra: Map<String, Value>) -> Value {
    items.truncate(MAX_ITEMS);
    loop {
        let mut body = json!({"type": ty, "api": API, key: items});
        for (k, v) in &extra { body[k] = v.clone(); }
        if body.to_string().len() <= MAX_BODY || items.is_empty() { return body; }
        items.pop();
    }
}
pub fn agent_list_ok(agents: &[Agent]) -> Value {
    fit("agent_list_ok", "agents", agents.iter().map(|a| json!({
        "session_id": crate::proto::uuid_str(&a.session_id), "agent": a.agent, "args": clip(&a.args), "label": clip(&a.label), "cwd": a.cwd.as_deref().map(clip),
        "state": match a.state { AgentState::Running => "running", AgentState::Exited => "exited", AgentState::Saved => "saved" },
        "last_activity_ms": crate::proto::u64s(a.last_activity_ms),
    })).collect(), Map::new())
}
/// Servidores e as sessões SSH do hub (até [`MAX_ITEMS`] cada; sessões são no máximo 16).
pub fn ssh_list_ok(servers: &[SshServer], sessions: &[SshSession]) -> Value {
    let sess: Vec<Value> = sessions.iter().take(MAX_ITEMS).map(|s| json!({
        "session_id": crate::proto::uuid_str(&s.session_id), "label": clip(&s.label), "state": if s.connected { "connected" } else { "disconnected" },
    })).collect();
    let mut extra = Map::new();
    extra.insert("sessions".into(), sess.into());
    fit("ssh_list_ok", "servers", servers.iter().map(|s| json!({
        "server_id": s.server_id, "label": clip(&s.label), "host": clip(&s.host), "user": clip(&s.user), "port": s.port,
        "key": s.key.as_deref().map(clip), "latency_ms": s.latency_ms, "status": s.status,
    })).collect(), extra)
}
pub fn history_query_ok(entries: &[HistoryEntry]) -> Value {
    fit("history_query_ok", "entries", entries.iter().map(|e| json!({
        "entry_id": e.entry_id, "command": clip(&e.command), "cwd": e.cwd.as_deref().map(clip), "at_ms": crate::proto::u64s(e.at_ms), "exit_code": e.exit_code,
        "kind": clip(&e.kind), "label": clip(&e.label), "pc": clip(&e.pc),
    })).collect(), Map::new())
}
pub fn session_ok(ty: &str, sid: &[u8; 16]) -> Value { json!({"type": ty, "api": API, "session_id": crate::proto::uuid_str(sid)}) }
pub fn ssh_save_ok(server_id: &str) -> Value { json!({"type": "ssh_save_ok", "api": API, "server_id": server_id}) }
/// Resposta sem dados (`history_run_ok`, `history_clear_ok`, `agent_remove_ok`, `ssh_remove_ok`, `ssh_probe_ok`).
pub fn empty_ok(ty: &str) -> Value { json!({"type": ty, "api": API}) }
pub fn history_run_ok() -> Value { empty_ok("history_run_ok") }

/// Rótulo padrão de nova sessão de agente: `<pasta> · <agente>` (mesma regra do diálogo desktop).
pub fn agent_label(cwd: Option<&str>, agent: &str) -> String {
    let folder = cwd.unwrap_or("").rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("~");
    format!("{folder} · {agent}").chars().take(64).collect()
}

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
            ("agent_create", json!({"type":"agent_create","api":1,"agent":"claude","cwd":"C:\\proj\\api","args":"--model opus","cols":80,"rows":24})),
            ("agent_create_minimo", json!({"type":"agent_create","api":1,"agent":"codex","cols":80,"rows":24})),
            ("agent_remove", json!({"type":"agent_remove","api":1,"session_id":SID})),
            ("ssh_list", json!({"type":"ssh_list","api":1})),
            ("ssh_connect", json!({"type":"ssh_connect","api":1,"server_id":"srv-1","cols":80,"rows":24})),
            ("ssh_save", json!({"type":"ssh_save","api":1,"label":"Produção","user":"deploy","host":"10.0.0.5","port":22,"key":null})),
            ("ssh_save_editar", json!({"type":"ssh_save","api":1,"server_id":"srv-1","label":"NAS","user":"admin","host":"nas.local","port":2222,"key":"C:\\Users\\eu\\.ssh\\id_ed25519"})),
            ("ssh_remove", json!({"type":"ssh_remove","api":1,"server_id":"srv-1"})),
            ("ssh_probe", json!({"type":"ssh_probe","api":1,"server_id":"srv-1"})),
            ("ssh_reconnect", json!({"type":"ssh_reconnect","api":1,"session_id":SID,"cols":80,"rows":24})),
            ("history_query", json!({"type":"history_query","api":1,"query":"git","limit":50})),
            ("history_query_vazio", json!({"type":"history_query","api":1})),
            ("history_query_filtros", json!({"type":"history_query","api":1,"limit":200,"days":7,"kind":"pwsh","errors":true})),
            ("history_run", json!({"type":"history_run","api":1,"entry_id":"h_42","session_id":SID})),
            ("history_clear", json!({"type":"history_clear","api":1})),
            ("api_2", json!({"type":"agent_list","api":2})),
            ("sem_api", json!({"type":"agent_list"})),
            ("api_string", json!({"type":"agent_list","api":"1"})),
            ("campo_desconhecido", json!({"type":"ssh_list","api":1,"x":1})),
            ("resume_sid_maiusculo", json!({"type":"agent_resume","api":1,"session_id":SID.to_uppercase(),"cols":80,"rows":24})),
            ("resume_cols_0", json!({"type":"agent_resume","api":1,"session_id":SID,"cols":0,"rows":24})),
            ("create_agente_desconhecido", json!({"type":"agent_create","api":1,"agent":"bash","cols":80,"rows":24})),
            ("create_args_shell", json!({"type":"agent_create","api":1,"agent":"claude","args":"& calc","cols":80,"rows":24})),
            ("create_label_65", json!({"type":"agent_create","api":1,"agent":"claude","label":"x".repeat(65),"cols":80,"rows":24})),
            ("connect_id_vazio", json!({"type":"ssh_connect","api":1,"server_id":"","cols":80,"rows":24})),
            ("connect_id_espaco", json!({"type":"ssh_connect","api":1,"server_id":"a b","cols":80,"rows":24})),
            ("save_com_senha", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":22,"password":"segredo"})),
            ("save_user_opcao", json!({"type":"ssh_save","api":1,"label":"x","user":"-oProxyCommand=calc","host":"h","port":22})),
            ("save_host_opcao", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"-oProxyCommand=calc","port":22})),
            ("save_porta_0", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":0})),
            ("save_chave_relativa", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":22,"key":"id_rsa"})),
            ("save_nome_41", json!({"type":"ssh_save","api":1,"label":"x".repeat(41),"user":"u","host":"h","port":22})),
            ("query_limit_0", json!({"type":"history_query","api":1,"limit":0})),
            ("query_limit_201", json!({"type":"history_query","api":1,"limit":201})),
            ("query_257", json!({"type":"history_query","api":1,"query":"x".repeat(257)})),
            ("query_tipo_desconhecido", json!({"type":"history_query","api":1,"kind":"zsh"})),
            ("query_dias_0", json!({"type":"history_query","api":1,"days":0})),
            ("query_erros_texto", json!({"type":"history_query","api":1,"errors":"true"})),
            ("run_sem_sessao", json!({"type":"history_run","api":1,"entry_id":"h_42"})),
            ("clear_com_campo", json!({"type":"history_clear","api":1,"all":true})),
        ]
    }

    fn agents() -> Vec<Agent> {
        vec![
            Agent { session_id: sid(), agent: "claude".into(), args: "--model opus".into(), label: "api".into(), cwd: Some("C:/proj/api".into()), state: AgentState::Running, last_activity_ms: 1_700_000_000_000, profile_id: None },
            Agent { session_id: uuid_v4_lower("2f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").unwrap(), agent: "codex".into(), args: String::new(), label: String::new(), cwd: None, state: AgentState::Saved, last_activity_ms: 5, profile_id: None },
        ]
    }

    #[test]
    fn pedidos() {
        let wrap = |mut v: Value| { v["v"] = 1.into(); v["connection_id"] = CID.into(); v["request_id"] = RID.into(); v.to_string() };
        let ok = ["agent_list", "agent_resume", "agent_create", "agent_create_minimo", "agent_remove", "ssh_list", "ssh_connect", "ssh_save", "ssh_save_editar",
            "ssh_remove", "ssh_probe", "ssh_reconnect", "history_query", "history_query_vazio", "history_query_filtros", "history_run", "history_clear", "api_2"];
        let mut cases = vec![];
        for (name, body) in requests() {
            let r = parse_control(&wrap(body.clone()), CID);
            assert_eq!(res(r.clone()), if ok.contains(&name) { "ok" } else { "INVALID_MESSAGE" }, "{name}");
            cases.push(json!({"name": name, "body": body, "result": res(r)}));
        }
        let get = |n: &str| { let b = requests().into_iter().find(|c| c.0 == n).unwrap().1; let Ok(Control::Request { req: P::Extra(r), .. }) = parse_control(&wrap(b), CID) else { panic!("{n}") }; r };
        assert_eq!(get("api_2"), Req::Unsupported);
        assert_eq!(get("history_query_vazio"), Req::HistoryQuery(HistoryFilter { query: String::new(), limit: 50, ..Default::default() }));
        assert_eq!(get("history_query_filtros"), Req::HistoryQuery(HistoryFilter { query: String::new(), limit: 200, days: Some(7), kind: Some("pwsh".into()), errors: true }));
        assert_eq!(get("ssh_connect"), Req::SshConnect { server_id: "srv-1".into(), cols: 80, rows: 24 });
        assert_eq!(get("history_run"), Req::HistoryRun { entry_id: "h_42".into(), session_id: sid() });
        assert_eq!(get("agent_create_minimo"), Req::AgentCreate { agent: "codex".into(), args: String::new(), cwd: None, label: None, cols: 80, rows: 24 });
        assert_eq!(get("ssh_save"), Req::SshSave { server_id: None, label: "Produção".into(), target: crate::ssh::Ssh { user: "deploy".into(), host: "10.0.0.5".into(), port: 22, key: None } });
        fixture(json!({"connection_id": CID, "caps": CAPS, "requests": cases, "replies": replies()}));
    }

    /// Respostas do host (construtores reais) e mutações; resultado esperado conferido no app.
    fn replies() -> Vec<Value> {
        let wrap = |b: Value| encode_control(CID, Some(RID), b);
        let with = |mut v: Value, path: &[&str], x: Value| { { let mut p = &mut v; for k in &path[..path.len() - 1] { p = if let Ok(i) = k.parse::<usize>() { &mut p[i] } else { &mut p[*k] }; } p[path[path.len() - 1]] = x; } v };
        let al = agent_list_ok(&agents());
        let sl = ssh_list_ok(&[SshServer { server_id: "srv-1".into(), label: "Produção".into(), host: "10.0.0.5".into(), user: "deploy".into(), port: 22, key: None, latency_ms: Some(38), status: "ok" },
            SshServer { server_id: "srv-2".into(), label: "NAS".into(), host: "nas.local".into(), user: "admin".into(), port: 2222, key: Some("C:\\k".into()), latency_ms: None, status: "timeout" }],
            &[SshSession { session_id: sid(), label: "Produção".into(), connected: false }]);
        let hq = history_query_ok(&[HistoryEntry { entry_id: "h_42".into(), command: "git status".into(), cwd: Some("C:/proj".into()), at_ms: 1_700_000_000_000, exit_code: Some(0), kind: "pwsh".into(), label: "PowerShell".into(), pc: "Este PC".into() },
            HistoryEntry { entry_id: "h_43".into(), command: "npm test".into(), cwd: None, at_ms: 7, exit_code: None, kind: "ssh".into(), label: "srv".into(), pc: "Este PC".into() }]);
        let cases: Vec<(&str, String, &str)> = vec![
            ("agent_list_ok", wrap(al.clone()), "ok"),
            ("agent_list_vazia", wrap(agent_list_ok(&[])), "ok"),
            ("agent_resume_ok", wrap(session_ok("agent_resume_ok", &sid())), "ok"),
            ("agent_create_ok", wrap(session_ok("agent_create_ok", &sid())), "ok"),
            ("agent_remove_ok", wrap(empty_ok("agent_remove_ok")), "ok"),
            ("ssh_list_ok", wrap(sl.clone()), "ok"),
            ("ssh_list_vazia", wrap(ssh_list_ok(&[], &[])), "ok"),
            ("ssh_connect_ok", wrap(session_ok("ssh_connect_ok", &sid())), "ok"),
            ("ssh_reconnect_ok", wrap(session_ok("ssh_reconnect_ok", &sid())), "ok"),
            ("ssh_save_ok", wrap(ssh_save_ok("srv-1")), "ok"),
            ("ssh_remove_ok", wrap(empty_ok("ssh_remove_ok")), "ok"),
            ("ssh_probe_ok", wrap(empty_ok("ssh_probe_ok")), "ok"),
            ("history_query_ok", wrap(hq.clone()), "ok"),
            ("history_query_vazia", wrap(history_query_ok(&[])), "ok"),
            ("history_run_ok", wrap(history_run_ok()), "ok"),
            ("history_clear_ok", wrap(empty_ok("history_clear_ok")), "ok"),
            ("api_2", wrap(with(history_run_ok(), &["api"], 2.into())), "VERSION_UNSUPPORTED"),
            ("sem_api", wrap(json!({"type":"history_run_ok"})), "INVALID_MESSAGE"),
            ("sem_request_id", encode_control(CID, None, history_run_ok()), "INVALID_MESSAGE"),
            ("agente_state_invalido", wrap(with(al.clone(), &["agents", "0", "state"], "zombie".into())), "INVALID_MESSAGE"),
            ("agente_atividade_numero", wrap(with(al.clone(), &["agents", "0", "last_activity_ms"], 1.into())), "INVALID_MESSAGE"),
            ("agente_campo_extra", wrap(with(al.clone(), &["agents", "0", "x"], 1.into())), "INVALID_MESSAGE"),
            ("agente_sem_args", wrap({ let mut v = al.clone(); v["agents"][0].as_object_mut().unwrap().remove("args"); v }), "INVALID_MESSAGE"),
            ("ssh_porta_0", wrap(with(sl.clone(), &["servers", "0", "port"], 0.into())), "INVALID_MESSAGE"),
            ("ssh_latencia_texto", wrap(with(sl.clone(), &["servers", "0", "latency_ms"], "38".into())), "INVALID_MESSAGE"),
            ("ssh_status_invalido", wrap(with(sl.clone(), &["servers", "0", "status"], "talvez".into())), "INVALID_MESSAGE"),
            ("ssh_sessao_estado_invalido", wrap(with(sl.clone(), &["sessions", "0", "state"], "running".into())), "INVALID_MESSAGE"),
            ("ssh_sem_sessions", wrap({ let mut v = sl.clone(); v.as_object_mut().unwrap().remove("sessions"); v }), "INVALID_MESSAGE"),
            ("ssh_save_sem_id", wrap(json!({"type":"ssh_save_ok","api":1})), "INVALID_MESSAGE"),
            ("historico_sem_command", wrap({ let mut v = hq.clone(); v["entries"][0].as_object_mut().unwrap().remove("command"); v }), "INVALID_MESSAGE"),
            ("historico_kind_numero", wrap(with(hq.clone(), &["entries", "0", "kind"], 1.into())), "INVALID_MESSAGE"),
            ("resume_sid_maiusculo", wrap(json!({"type":"agent_resume_ok","api":1,"session_id":SID.to_uppercase()})), "INVALID_MESSAGE"),
            ("clear_com_campo", wrap(json!({"type":"history_clear_ok","api":1,"n":3})), "INVALID_MESSAGE"),
        ];
        cases.into_iter().map(|(n, t, r)| { assert!(t.len() <= 65536); json!({"name": n, "text": t, "result": r}) }).collect()
    }

    #[test]
    fn corpo_cabe_numa_mensagem() {
        let big: Vec<HistoryEntry> = (0..200).map(|i| HistoryEntry { entry_id: format!("h{i}"), command: "x".repeat(2000), cwd: Some("y".repeat(2000)), at_ms: i, exit_code: None, kind: "pwsh".into(), label: "z".repeat(2000), pc: "p".into() }).collect();
        let v = history_query_ok(&big);
        assert!(v.to_string().len() <= MAX_BODY);
        let n = v["entries"].as_array().unwrap().len();
        assert!(n > 0 && n < 200);
        assert_eq!(v["entries"][0]["command"].as_str().unwrap().chars().count(), MAX_TEXT);
        let servers: Vec<SshServer> = (0..200).map(|i| SshServer { server_id: format!("s{i}"), label: "l".repeat(600), host: "h".repeat(253), user: "u".into(), port: 22, key: Some("k".repeat(600)), latency_ms: None, status: "unknown" }).collect();
        let v = ssh_list_ok(&servers, &[SshSession { session_id: sid(), label: "x".into(), connected: true }]);
        assert!(v.to_string().len() <= MAX_BODY);
        assert_eq!(v["sessions"].as_array().unwrap().len(), 1, "sessões ficam mesmo cortando servidores");
    }

    #[test]
    fn rotulo_padrao_do_agente() {
        assert_eq!(agent_label(Some(r"C:\proj\api\"), "claude"), "api · claude");
        assert_eq!(agent_label(None, "codex"), "~ · codex");
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
        assert!(p.ssh_servers().is_empty() && p.history(&HistoryFilter::default()).is_empty() && p.saved_agents().is_empty());
        assert_eq!(p.ssh_target("srv-1"), None);
        assert_eq!(code("NOT_FOUND"), NotFound);
        assert_eq!(code("INVALID_SSH_USER"), InvalidMessage);
    }

    fn fixture(v: Value) {
        let path = format!("{}/tests/fixtures/c3-extras.json", env!("CARGO_MANIFEST_DIR"));
        if std::env::var_os("LIVE_CMD_WRITE_FIXTURE").is_some() { std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap(); }
        let f: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f, v, "c3-extras.json desatualizada: rode com LIVE_CMD_WRITE_FIXTURE=1");
    }
}

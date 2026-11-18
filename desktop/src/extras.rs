//! Controles extras do `pty-channel` para as telas do celular: Agentes, SSH e Histórico.
//! Cada pedido e resposta leva `api` (hoje 1). `api` numérico diferente vira `VERSION_UNSUPPORTED`
//! com `request_id` (não fatal), para o app poder evoluir sem derrubar a conexão.
//!
//! Capacidades: o cliente pergunta com `session_attach` para a sessão reservada [`CAPS_PROBE`]
//! (`from_offset: null`). Este host responde `caps_ok` com [`CAPS`]; host antigo responde
//! `error NOT_FOUND` com `request_id` (não fatal) e o celular mostra "atualize o live-cmd no PC".
//! Assim nenhum dos dois lados vê campo ou tipo novo sem ter pedido: o `auth_ok` continua igual,
//! porque os validadores antigos (host e clientes) recusam campo desconhecido e derrubam a conexão.
//!
//! Os dados vêm de um [`Provider`]. Sem provedor registrado ([`set_provider`]), SSH e Histórico
//! respondem listas vazias e Agentes mostra só as sessões vivas com agente (lidas do hub em `lib.rs`).
#![allow(dead_code)]
use crate::auth::{uuid_v4_lower, Error::{self, *}};
use serde_json::{json, Map, Value};
use std::sync::{Arc, LazyLock, RwLock};

type R<T> = Result<T, Error>;
pub const API: u64 = 1;
/// Telas que este host entende, respondidas em `caps_ok`: cada uma cobre todos os pedidos `api: 1` dela.
///
/// Capacidades novas são opt-in: o cliente só manda `ext`, `settings_*` ou os campos novos do `ssh_save` se a
/// capacidade estiver em `caps_ok`. Sem `ext`, as respostas são byte a byte as de antes (cliente antigo segue igual).
/// - `settings`: `settings_get` e `settings_set` (aparência, lista branca em [`Appearance`]).
/// - `ssh_opts`: `ssh_save` aceita `keepalive_s` e `auto_reconnect`; `ssh_list` com `ext: true` devolve os dois.
/// - `agent_waiting`: `agent_list` com `ext: true` pode trazer `state: "waiting"` e inscreve a conexão no evento
///   `agent_waiting` (sem `request_id`), mandado quando o CLI de IA passa a esperar (ou deixa de esperar) confirmação.
/// - `history_sessions`: `history_query` com `ext: true` traz também `sessions` (contagem por sessão e dia).
/// - `folders`: navegador de pastas do PC (`folder_list`); `ssh_keys`: seletor da chave em `%USERPROFILE%\.ssh`
///   (`ssh_key_list`); `agent_default`: "Salvar como padrão" do agente (`agent_default_get`/`agent_default_set`).
pub const CAPS: [&str; 10] = ["agents", "ssh", "history", "folders", "ssh_keys", "agent_default", "settings", "ssh_opts", "agent_waiting", "history_sessions"];
/// Sessão reservada da pergunta de capacidades (UUID v4 válido que o host nunca gera).
pub const CAPS_PROBE: &str = "00000000-0000-4000-8000-000000000001";
/// `session_attach` para [`CAPS_PROBE`]? (Antes de ir ao hub: nunca anexa nada.)
pub fn is_caps_probe(sid: &[u8; 16]) -> bool { crate::auth::uuid_bytes(CAPS_PROBE).as_ref() == Some(sid) }
pub fn caps_ok() -> Value { json!({"type": "caps_ok", "api": API, "caps": CAPS}) }
/// Teto de itens por resposta e de bytes do corpo (a mensagem inteira não passa de 65536).
pub const MAX_ITEMS: usize = 200;
const MAX_BODY: usize = 60_000;
const MAX_TEXT: usize = 512;
/// Tipos de sessão do histórico (`hist.rs` no app).
pub const HISTORY_KINDS: [&str; 5] = ["pwsh", "cmd", "wsl", "ssh", "outro"];
/// Itens por listagem de pastas ou chaves (o mesmo teto do `dir_list` do desktop).
pub const MAX_ENTRIES: usize = 500;
/// Arquivo maior que isso não é chave privada.
const MAX_KEY_FILE: u64 = 64 << 10;
/// Arquivos do `.ssh` que não são chave privada (comparados em minúsculas).
const NOT_KEYS: [&str; 6] = ["config", "known_hosts", "known_hosts.old", "authorized_keys", "authorized_keys2", "environment"];

/// "Salvar como padrão" da nova sessão de agente: o mesmo registro nas duas telas (`agent-default.json` no app).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AgentDefault { pub agent: String, pub cwd: Option<String>, pub args: String }

/// Entrada nova numa sessão deste host, venha de qual cliente vier (terminal desktop, outro PC, celular).
/// `line`: linha do cursor na tela do host quando `data` tem Enter e a tela é a normal; `None` na tela alternativa.
/// Sessões de agente de IA não chegam aqui (lá se digita prompt, não comando).
#[derive(Debug, Clone, PartialEq)]
pub struct Input<'a> { pub session_id: [u8; 16], pub data: &'a str, pub line: Option<String>, pub label: String, pub cwd: Option<String>, pub ssh: bool }

/// Filtros do histórico, os mesmos da tela desktop.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryFilter { pub query: String, pub limit: usize, pub days: Option<u32>, pub kind: Option<String>, pub errors: bool,
    /// `ext: true` (capacidade `history_sessions`): a resposta traz também o resumo por sessão e dia.
    pub ext: bool }

#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    /// `ext`: capacidade `agent_waiting` (estado `waiting` e inscrição no evento).
    AgentList { ext: bool },
    AgentResume { session_id: [u8; 16], cols: u16, rows: u16 },
    /// Nova sessão de agente: o host escolhe o primeiro perfil, como o desktop.
    AgentCreate { agent: String, args: String, cwd: Option<String>, label: Option<String>, cols: u16, rows: u16 },
    /// Encerra o terminal (se aberto) e tira do registro.
    AgentRemove { session_id: [u8; 16] },
    /// `ext`: capacidade `ssh_opts` (servidores com `keepalive_s` e `auto_reconnect`).
    SshList { ext: bool },
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
    /// Subpastas de `path` (vazio: unidades), como o `dir_list` do terminal desktop.
    FolderList(String),
    /// Chaves e subpastas dentro de `%USERPROFILE%\.ssh` (vazio: a própria `.ssh`).
    SshKeyList(String),
    AgentDefaultGet,
    AgentDefaultSet(AgentDefault),
    SettingsGet,
    /// Só os campos mandados mudam; o resto fica. Já validado contra a lista branca ([`Appearance::patch`]).
    SettingsSet(Map<String, Value>),
    /// `api` numérico diferente de 1: o host responde `VERSION_UNSUPPORTED` com `request_id`.
    Unsupported,
}

const KNOWN: [&str; 19] = ["agent_list", "agent_resume", "agent_create", "agent_remove", "ssh_list", "ssh_connect", "ssh_save", "ssh_remove",
    "ssh_probe", "ssh_reconnect", "history_query", "history_run", "history_clear", "folder_list", "ssh_key_list", "agent_default_get", "agent_default_set",
    "settings_get", "settings_set"];

/// Temas, fontes e faixas da aparência (telas Configurações do desktop e do celular).
/// "dark" é o Aurora (padrão). Os antigos "light" e "contrast" foram aposentados: `legacy_theme` os leva ao Aurora.
pub const THEMES: [&str; 4] = ["dark", "ametista", "magenta", "cosmo"];

/// Tema gravado por versões antigas (claro, alto contraste) vira Aurora, sem perder as outras opções.
pub fn legacy_theme(t: &str) -> &str { if matches!(t, "light" | "contrast") { "dark" } else { t } }
pub const FONTS: [&str; 4] = ["Fira Code", "Cascadia Mono", "Consolas", "Courier New"];
const FONT_SIZE: std::ops::RangeInclusive<u64> = 10..=24;
const OPACITY: std::ops::RangeInclusive<u64> = 60..=100;
const BLUR: std::ops::RangeInclusive<u64> = 0..=60;

/// Aparência do app, gravada em `settings.json` do PC. `opacity` em %, `blur` em px.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Appearance { pub theme: String, pub font_family: String, pub font_size: u8, pub opacity: u8, pub blur: u8, pub glow: bool, pub animations: bool }
impl Default for Appearance {
    fn default() -> Self { Appearance { theme: "dark".into(), font_family: "Fira Code".into(), font_size: 13, opacity: 100, blur: 30, glow: true, animations: true } }
}
impl Appearance {
    pub fn valid(&self) -> bool {
        THEMES.contains(&self.theme.as_str()) && FONTS.contains(&self.font_family.as_str()) && FONT_SIZE.contains(&(self.font_size as u64))
            && OPACITY.contains(&(self.opacity as u64)) && BLUR.contains(&(self.blur as u64))
    }
    /// Aplica os campos de `o` (lista branca, tipo e faixa conferidos); campo desconhecido é `INVALID_MESSAGE`.
    pub fn patch(&self, o: &Map<String, Value>) -> R<Appearance> {
        let mut a = self.clone();
        let num = |v: &Value, r: &std::ops::RangeInclusive<u64>| v.as_u64().filter(|n| r.contains(n)).map(|n| n as u8).ok_or(InvalidMessage);
        let pick = |v: &Value, list: &[&str]| v.as_str().filter(|x| list.contains(x)).map(str::to_owned).ok_or(InvalidMessage);
        for (k, v) in o {
            match k.as_str() {
                "theme" => a.theme = pick(v, &THEMES)?,
                "font_family" => a.font_family = pick(v, &FONTS)?,
                "font_size" => a.font_size = num(v, &FONT_SIZE)?,
                "opacity" => a.opacity = num(v, &OPACITY)?,
                "blur" => a.blur = num(v, &BLUR)?,
                "glow" => a.glow = v.as_bool().ok_or(InvalidMessage)?,
                "animations" => a.animations = v.as_bool().ok_or(InvalidMessage)?,
                _ => return Err(InvalidMessage),
            }
        }
        if a.valid() { Ok(a) } else { Err(InvalidMessage) }
    }
    pub fn json(&self) -> Value { serde_json::to_value(self).unwrap_or_default() }
}

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
    let ext = || match o.get("ext") { None => Ok(false), Some(v) => v.as_bool().ok_or(InvalidMessage) };
    let dim = |k: &str| o.get(k).and_then(Value::as_u64).filter(|n| (1..=1000).contains(n)).map(|n| n as u16).ok_or(InvalidMessage);
    Ok(match ty {
        "agent_list" => { fields(&[], &["ext"])?; Req::AgentList { ext: ext()? } }
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
        "ssh_list" => { fields(&[], &["ext"])?; Req::SshList { ext: ext()? } }
        "ssh_connect" => { fields(&["server_id", "cols", "rows"], &[])?; Req::SshConnect { server_id: id("server_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "ssh_save" => {
            // Sem campo de senha: `password` (ou qualquer outro) é campo desconhecido.
            fields(&["label", "user", "host", "port"], &["server_id", "key", "keepalive_s", "auto_reconnect"])?;
            let server_id = o.get("server_id").map(|_| id("server_id")).transpose()?;
            let label = text(s("label")?.trim(), 1, 40)?;
            let port = o.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p)).ok_or(InvalidMessage)? as u16;
            let key = match o.get("key") { None | Some(Value::Null) => None, Some(_) => Some(s("key")?.trim().to_owned()).filter(|k| !k.is_empty()) };
            let keepalive_s = match o.get("keepalive_s") { None => 0, Some(v) => v.as_u64().filter(|n| *n <= u16::MAX as u64).ok_or(InvalidMessage)? as u16 };
            let auto_reconnect = match o.get("auto_reconnect") { None => false, Some(v) => v.as_bool().ok_or(InvalidMessage)? };
            let target = crate::ssh::Ssh { user: s("user")?.trim().to_owned(), host: s("host")?.trim().to_owned(), port, key, keepalive_s, auto_reconnect };
            target.check().map_err(|_| InvalidMessage)?;
            Req::SshSave { server_id, label, target }
        }
        "ssh_remove" => { fields(&["server_id"], &[])?; Req::SshRemove { server_id: id("server_id")? } }
        "ssh_probe" => { fields(&["server_id"], &[])?; Req::SshProbe { server_id: id("server_id")? } }
        "ssh_reconnect" => { fields(&["session_id", "cols", "rows"], &[])?; Req::SshReconnect { session_id: sid("session_id")?, cols: dim("cols")?, rows: dim("rows")? } }
        "history_query" => {
            fields(&[], &["query", "limit", "days", "kind", "errors", "ext"])?;
            let query = opt_s("query")?.unwrap_or("").to_owned();
            if query.chars().count() > 256 { return Err(InvalidMessage); }
            let limit = match o.get("limit") { None => 50, Some(v) => v.as_u64().filter(|n| (1..=MAX_ITEMS as u64).contains(n)).ok_or(InvalidMessage)? as usize };
            let days = match o.get("days") { None | Some(Value::Null) => None, Some(v) => Some(v.as_u64().filter(|n| (1..=3650).contains(n)).ok_or(InvalidMessage)? as u32) };
            let kind = match o.get("kind") { None | Some(Value::Null) => None, Some(_) => Some(s("kind")?).filter(|k| HISTORY_KINDS.contains(k)).map(str::to_owned).ok_or(InvalidMessage).map(Some)? };
            let errors = match o.get("errors") { None => false, Some(v) => v.as_bool().ok_or(InvalidMessage)? };
            Req::HistoryQuery(HistoryFilter { query, limit, days, kind, errors, ext: ext()? })
        }
        "history_run" => { fields(&["entry_id", "session_id"], &[])?; Req::HistoryRun { entry_id: id("entry_id")?, session_id: sid("session_id")? } }
        "history_clear" => { fields(&[], &[])?; Req::HistoryClear }
        "folder_list" => { fields(&["path"], &[])?; Req::FolderList(text(s("path")?.trim(), 0, 1024)?) }
        "ssh_key_list" => { fields(&[], &["path"])?; Req::SshKeyList(opt_s("path")?.map(|p| text(p.trim(), 0, 1024)).transpose()?.unwrap_or_default()) }
        "agent_default_get" => { fields(&[], &[])?; Req::AgentDefaultGet }
        "agent_default_set" => {
            // Mesmas regras do `agent_create`: agente conhecido, args sem efeito de shell, pasta como texto curto.
            fields(&["agent"], &["cwd", "args"])?;
            let agent = s("agent")?;
            if !crate::proto::AGENTS.contains(&agent) { return Err(InvalidMessage); }
            let args = opt_s("args")?.unwrap_or("").trim().to_owned();
            if !crate::proto::valid_args(&args) { return Err(InvalidMessage); }
            let cwd = match o.get("cwd") { None | Some(Value::Null) => None, Some(_) => Some(text(s("cwd")?.trim(), 0, 1024)?).filter(|c| !c.is_empty()) };
            Req::AgentDefaultSet(AgentDefault { agent: agent.to_owned(), cwd, args })
        }
        "settings_get" => { fields(&[], &[])?; Req::SettingsGet }
        "settings_set" => {
            fields(&["settings"], &[])?;
            let m = o.get("settings").and_then(Value::as_object).filter(|m| !m.is_empty()).ok_or(InvalidMessage)?;
            // Confere lista branca, tipo e faixa já no parse (contra o padrão); o host aplica sobre o valor gravado.
            Appearance::default().patch(m)?;
            Req::SettingsSet(m.clone())
        }
        _ => return Err(InvalidMessage),
    })
}

// ---- dados ----

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentState { Running, Exited, Saved }
/// Sessão de IA. `session_id` é também o id da conversa (`--resume <id>`).
/// `profile_id`: perfil em que a sessão foi criada (retomar usa o mesmo; ausente ou apagado = o primeiro).
#[derive(Debug, Clone, PartialEq)]
pub struct Agent { pub session_id: [u8; 16], pub agent: String, pub args: String, pub label: String, pub cwd: Option<String>, pub state: AgentState, pub last_activity_ms: u64, pub profile_id: Option<String>,
    /// O CLI está parado num pedido de confirmação ([`waiting_prompt`]). Só sai como `state: "waiting"` com `ext`.
    pub waiting: bool }
/// `status`: alcance TCP como na tela desktop: `unknown`, `testing`, `ok`, `dns`, `timeout` ou `offline`.
#[derive(Debug, Clone, PartialEq)]
pub struct SshServer { pub server_id: String, pub label: String, pub host: String, pub user: String, pub port: u16, pub key: Option<String>, pub latency_ms: Option<u32>, pub status: &'static str,
    /// "Manter vivo" (s, 0 = desligado) e "Reconectar ao cair"; só saem com `ext`.
    pub keepalive_s: u16, pub auto_reconnect: bool }
/// Sessão SSH do hub: `connected = false` quando o ssh.exe saiu ou a sessão foi restaurada do disco.
#[derive(Debug, Clone, PartialEq)]
pub struct SshSession { pub session_id: [u8; 16], pub label: String, pub connected: bool }
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry { pub entry_id: String, pub command: String, pub cwd: Option<String>, pub at_ms: u64, pub exit_code: Option<i32>, pub kind: String, pub label: String, pub pc: String }
/// Resumo "Sessões" do Histórico: comandos por sessão (rótulo, tipo, PC) e dia local (`AAAA-MM-DD`).
#[derive(Debug, Clone, PartialEq)]
pub struct HistorySession { pub label: String, pub kind: String, pub pc: String, pub day: String, pub count: u32, pub errors: u32, pub last_ms: u64 }

/// Data civil (`AAAA-MM-DD`) de `ms` com o fuso `offset_min` (minutos a somar ao UTC).
pub fn local_day(ms: u64, offset_min: i32) -> String {
    let days = (ms as i64 / 1000 + offset_min as i64 * 60).div_euclid(86_400);
    // Algoritmo de Howard Hinnant (dias desde 1970-01-01 para ano/mês/dia).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Agrupa as entradas por sessão e dia, do grupo mais recente para o mais antigo.
pub fn summarize(entries: &[HistoryEntry], offset_min: i32) -> Vec<HistorySession> {
    let mut out: Vec<HistorySession> = vec![];
    for e in entries {
        let day = local_day(e.at_ms, offset_min);
        let bad = u32::from(e.exit_code.is_some_and(|c| c != 0));
        match out.iter_mut().find(|g| g.label == e.label && g.kind == e.kind && g.pc == e.pc && g.day == day) {
            Some(g) => { g.count += 1; g.errors += bad; g.last_ms = g.last_ms.max(e.at_ms); }
            None => out.push(HistorySession { label: e.label.clone(), kind: e.kind.clone(), pc: e.pc.clone(), day, count: 1, errors: bad, last_ms: e.at_ms }),
        }
    }
    out.sort_by(|a, b| b.last_ms.cmp(&a.last_ms));
    out
}

/// O CLI de IA está esperando o usuário confirmar? Olha só as últimas linhas não vazias da tela.
/// Padrões de saída do Claude Code e do Codex (perguntas de permissão e menus de aprovação).
pub fn waiting_prompt(screen: &str) -> bool {
    const PATTERNS: [&str; 10] = ["do you want to", "allow this", "allow command", "approve", "(y/n)", "[y/n]", "yes, and don't ask again",
        "press enter to confirm", "deseja continuar", "permitir"];
    let tail: Vec<String> = screen.lines().rev().map(str::trim).filter(|l| !l.is_empty()).take(12).map(str::to_lowercase).collect();
    tail.iter().any(|l| PATTERNS.iter().any(|p| l.contains(p)))
}

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
    /// Resumo "Sessões" (capacidade `history_sessions`). Padrão: agrupa as últimas [`MAX_ITEMS`] entradas do filtro.
    fn history_sessions(&self, f: &HistoryFilter, offset_min: i32) -> Vec<HistorySession> {
        summarize(&self.history(&HistoryFilter { limit: MAX_ITEMS, ..f.clone() }), offset_min)
    }
    fn history_clear(&self) -> Result<(), String> { Ok(()) }
    /// Ponto comum do histórico: toda entrada nova (frame `01` aceito ou "Reexecutar") de sessão sem agente.
    fn session_input(&self, _i: &Input) {}
    /// `command_completed` (OSC 133;D) da sessão: código de saída do último comando gravado.
    fn command_done(&self, _session_id: &[u8; 16], _code: Option<i32>) {}
    /// A sessão saiu da tabela: o provedor solta o que guardava dela.
    fn session_gone(&self, _session_id: &[u8; 16]) {}
    fn agent_default(&self) -> Option<AgentDefault> { None }
    fn agent_default_save(&self, _d: &AgentDefault) -> Result<(), String> { Err("NOT_FOUND".into()) }
    /// Aparência gravada; `None` = este PC não tem tela de configurações (`NOT_FOUND`).
    fn settings_get(&self) -> Option<Appearance> { None }
    /// Grava a aparência já validada e aplica nas janelas do desktop.
    fn settings_set(&self, _a: &Appearance) -> Result<(), String> { Err("NOT_FOUND".into()) }
    /// O CLI de IA passou a esperar (ou deixou de esperar) confirmação: a fatia desktop mostra no cartão.
    fn agent_waiting(&self, _session_id: &[u8; 16], _label: &str, _waiting: bool) {}
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
fn fit(ty: &str, key: &str, items: Vec<Value>, extra: Map<String, Value>) -> Value { fit_max(ty, key, items, extra, MAX_ITEMS) }
fn fit_max(ty: &str, key: &str, mut items: Vec<Value>, extra: Map<String, Value>, max: usize) -> Value {
    items.truncate(max);
    loop {
        let mut body = json!({"type": ty, "api": API, key: items});
        for (k, v) in &extra { body[k] = v.clone(); }
        if body.to_string().len() <= MAX_BODY || items.is_empty() { return body; }
        items.pop();
    }
}
/// `ext` (capacidade `agent_waiting`): agente vivo esperando confirmação sai como `state: "waiting"`.
pub fn agent_list_ok(agents: &[Agent], ext: bool) -> Value {
    fit("agent_list_ok", "agents", agents.iter().map(|a| json!({
        "session_id": crate::proto::uuid_str(&a.session_id), "agent": a.agent, "args": clip(&a.args), "label": clip(&a.label), "cwd": a.cwd.as_deref().map(clip),
        "state": match a.state { AgentState::Running if ext && a.waiting => "waiting", AgentState::Running => "running", AgentState::Exited => "exited", AgentState::Saved => "saved" },
        "last_activity_ms": crate::proto::u64s(a.last_activity_ms),
    })).collect(), Map::new())
}
/// Servidores e as sessões SSH do hub (até [`MAX_ITEMS`] cada; sessões são no máximo 16).
/// `ext` (capacidade `ssh_opts`): cada servidor leva também `keepalive_s` e `auto_reconnect`.
pub fn ssh_list_ok(servers: &[SshServer], sessions: &[SshSession], ext: bool) -> Value {
    let sess: Vec<Value> = sessions.iter().take(MAX_ITEMS).map(|s| json!({
        "session_id": crate::proto::uuid_str(&s.session_id), "label": clip(&s.label), "state": if s.connected { "connected" } else { "disconnected" },
    })).collect();
    let mut extra = Map::new();
    extra.insert("sessions".into(), sess.into());
    fit("ssh_list_ok", "servers", servers.iter().map(|s| {
        let mut v = json!({
            "server_id": s.server_id, "label": clip(&s.label), "host": clip(&s.host), "user": clip(&s.user), "port": s.port,
            "key": s.key.as_deref().map(clip), "latency_ms": s.latency_ms, "status": s.status,
        });
        if ext { v["keepalive_s"] = s.keepalive_s.into(); v["auto_reconnect"] = s.auto_reconnect.into(); }
        v
    }).collect(), extra)
}
/// `sessions` (com `ext`, capacidade `history_sessions`): resumo por sessão e dia, até [`MAX_ITEMS`] grupos.
pub fn history_query_ok(entries: &[HistoryEntry], sessions: Option<&[HistorySession]>) -> Value {
    let mut extra = Map::new();
    if let Some(g) = sessions {
        extra.insert("sessions".into(), g.iter().take(MAX_ITEMS).map(|g| json!({
            "label": clip(&g.label), "kind": clip(&g.kind), "pc": clip(&g.pc), "day": g.day, "count": g.count, "errors": g.errors, "last_ms": crate::proto::u64s(g.last_ms),
        })).collect::<Vec<_>>().into());
    }
    fit("history_query_ok", "entries", entries.iter().map(|e| json!({
        "entry_id": e.entry_id, "command": clip(&e.command), "cwd": e.cwd.as_deref().map(clip), "at_ms": crate::proto::u64s(e.at_ms), "exit_code": e.exit_code,
        "kind": clip(&e.kind), "label": clip(&e.label), "pc": clip(&e.pc),
    })).collect(), extra)
}
/// `settings_get_ok` / `settings_set_ok`: a aparência inteira, já com a mudança.
pub fn settings_ok(ty: &str, a: &Appearance) -> Value { json!({"type": ty, "api": API, "settings": a.json()}) }
/// Evento sem `request_id`, só para conexões inscritas por `agent_list` com `ext`.
pub fn agent_waiting_event(sid: &[u8; 16], label: &str, waiting: bool) -> Value {
    json!({"type": "agent_waiting", "api": API, "session_id": crate::proto::uuid_str(sid), "label": clip(label), "waiting": waiting})
}
pub fn session_ok(ty: &str, sid: &[u8; 16]) -> Value { json!({"type": ty, "api": API, "session_id": crate::proto::uuid_str(sid)}) }
pub fn ssh_save_ok(server_id: &str) -> Value { json!({"type": "ssh_save_ok", "api": API, "server_id": server_id}) }
/// Resposta sem dados (`history_run_ok`, `history_clear_ok`, `agent_remove_ok`, `ssh_remove_ok`, `ssh_probe_ok`).
pub fn empty_ok(ty: &str) -> Value { json!({"type": ty, "api": API}) }
pub fn history_run_ok() -> Value { empty_ok("history_run_ok") }
/// Subpastas de `path` (até [`MAX_ENTRIES`], cortadas para caber na mensagem).
pub fn folder_list_ok(path: &str, dirs: &[String]) -> Value {
    let mut extra = Map::new();
    extra.insert("path".into(), path.into());
    fit_max("folder_list_ok", "dirs", dirs.iter().map(|d| Value::from(clip(d))).collect(), extra, MAX_ENTRIES)
}
/// Chaves e subpastas de `path`, dentro de `root` (a `.ssh`); `path == root` no topo.
pub fn ssh_key_list_ok(root: &str, path: &str, dirs: &[String], keys: &[String]) -> Value {
    let mut extra = Map::new();
    extra.insert("root".into(), root.into());
    extra.insert("path".into(), path.into());
    // As chaves vão inteiras (são poucas); as pastas é que se cortam para caber.
    extra.insert("keys".into(), keys.iter().take(MAX_ITEMS).map(|k| Value::from(clip(k))).collect::<Vec<_>>().into());
    fit_max("ssh_key_list_ok", "dirs", dirs.iter().map(|d| Value::from(clip(d))).collect(), extra, MAX_ENTRIES)
}
/// Padrão salvo, ou `agent: null` se nunca foi salvo.
pub fn agent_default_ok(d: Option<&AgentDefault>) -> Value {
    match d {
        Some(d) => json!({"type": "agent_default_ok", "api": API, "agent": d.agent, "cwd": d.cwd, "args": d.args}),
        None => json!({"type": "agent_default_ok", "api": API, "agent": null, "cwd": null, "args": ""}),
    }
}

/// `%USERPROFILE%\.ssh` deste usuário (`None` sem perfil).
pub fn ssh_root() -> Option<std::path::PathBuf> { std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join(".ssh")) }

/// Lista do `ssh_key_list`: `path` vazio é `root`; fora de `root` (inclusive por `..` ou link) é `NOT_FOUND`.
/// Devolve `(caminho mostrado, subpastas, chaves)`; chave é arquivo de até 64 KiB que não é `.pub`, `config`, `known_hosts`…
pub fn ssh_keys(root: &std::path::Path, path: &str) -> R<(String, Vec<String>, Vec<String>)> {
    let croot = std::fs::canonicalize(root).map_err(|_| NotFound)?;
    let want = if path.is_empty() { root.to_path_buf() } else { std::path::PathBuf::from(path) };
    if !want.is_absolute() { return Err(NotFound); }
    let cwant = std::fs::canonicalize(&want).map_err(|_| NotFound)?;
    if !cwant.starts_with(&croot) || !cwant.is_dir() { return Err(NotFound); }
    // Caminho como o usuário vê (sem `\\?\`): a raiz mais o trecho relativo já conferido.
    let shown = root.join(cwant.strip_prefix(&croot).unwrap_or(std::path::Path::new("")));
    let shown = shown.to_string_lossy().trim_end_matches(['\\', '/']).to_owned();
    let rd = std::fs::read_dir(&cwant).map_err(|_| IoError)?;
    let (mut dirs, mut keys) = (vec![], vec![]);
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        if name.chars().any(char::is_control) { continue; }
        // Pasta que é link para fora da `.ssh` aparece, mas abrir dá NOT_FOUND (a canonização acima confere).
        let Ok(m) = std::fs::metadata(e.path()) else { continue };
        let lower = name.to_lowercase();
        if m.is_dir() { if dirs.len() < MAX_ENTRIES { dirs.push(name); } }
        else if m.is_file() && m.len() <= MAX_KEY_FILE && !lower.ends_with(".pub") && !NOT_KEYS.contains(&lower.as_str()) && keys.len() < MAX_ENTRIES { keys.push(name); }
    }
    dirs.sort_by_key(|n| n.to_lowercase());
    keys.sort_by_key(|n| n.to_lowercase());
    Ok((shown, dirs, keys))
}

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
            ("folder_list", json!({"type":"folder_list","api":1,"path":"C:\\proj"})),
            ("folder_list_unidades", json!({"type":"folder_list","api":1,"path":""})),
            ("ssh_key_list", json!({"type":"ssh_key_list","api":1})),
            ("ssh_key_list_pasta", json!({"type":"ssh_key_list","api":1,"path":"C:\\Users\\eu\\.ssh\\work"})),
            ("agent_default_get", json!({"type":"agent_default_get","api":1})),
            ("agent_default_set", json!({"type":"agent_default_set","api":1,"agent":"claude","cwd":"C:\\proj\\api","args":"--model opus"})),
            ("agent_default_set_minimo", json!({"type":"agent_default_set","api":1,"agent":"codex"})),
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
            ("folder_sem_path", json!({"type":"folder_list","api":1})),
            ("folder_path_1025", json!({"type":"folder_list","api":1,"path":"x".repeat(1025)})),
            ("folder_path_controle", json!({"type":"folder_list","api":1,"path":"C:\\a\nb"})),
            ("chave_path_numero", json!({"type":"ssh_key_list","api":1,"path":1})),
            ("chave_campo_extra", json!({"type":"ssh_key_list","api":1,"filter":"*"})),
            ("padrao_agente_desconhecido", json!({"type":"agent_default_set","api":1,"agent":"bash"})),
            ("padrao_args_shell", json!({"type":"agent_default_set","api":1,"agent":"claude","args":"& calc"})),
            ("padrao_sem_agente", json!({"type":"agent_default_set","api":1,"args":"--verbose"})),
            // Capacidades novas (opt-in).
            ("agent_list_ext", json!({"type":"agent_list","api":1,"ext":true})),
            ("ssh_list_ext", json!({"type":"ssh_list","api":1,"ext":true})),
            ("ssh_save_opcoes", json!({"type":"ssh_save","api":1,"label":"Produção","user":"deploy","host":"10.0.0.5","port":22,"key":null,"keepalive_s":30,"auto_reconnect":true})),
            ("history_query_ext", json!({"type":"history_query","api":1,"limit":50,"ext":true})),
            ("settings_get", json!({"type":"settings_get","api":1})),
            ("settings_set", json!({"type":"settings_set","api":1,"settings":{"theme":"cosmo","font_size":14,"glow":false}})),
            ("settings_set_tudo", json!({"type":"settings_set","api":1,"settings":{"theme":"magenta","font_family":"Consolas","font_size":10,"opacity":60,"blur":0,"glow":true,"animations":false}})),
            ("ext_texto", json!({"type":"agent_list","api":1,"ext":"sim"})),
            ("save_keepalive_4", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":22,"keepalive_s":4})),
            ("save_keepalive_texto", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":22,"keepalive_s":"30"})),
            ("save_reconectar_numero", json!({"type":"ssh_save","api":1,"label":"x","user":"u","host":"h","port":22,"auto_reconnect":1})),
            ("set_vazio", json!({"type":"settings_set","api":1,"settings":{}})),
            ("set_campo_desconhecido", json!({"type":"settings_set","api":1,"settings":{"server":"wss://x"}})),
            ("set_tema_desconhecido", json!({"type":"settings_set","api":1,"settings":{"theme":"neon"}})),
            ("set_fonte_livre", json!({"type":"settings_set","api":1,"settings":{"font_family":"Comic Sans"}})),
            ("set_fonte_25", json!({"type":"settings_set","api":1,"settings":{"font_size":25}})),
            ("set_opacidade_59", json!({"type":"settings_set","api":1,"settings":{"opacity":59}})),
            ("set_desfoque_61", json!({"type":"settings_set","api":1,"settings":{"blur":61}})),
            ("set_brilho_texto", json!({"type":"settings_set","api":1,"settings":{"glow":"true"}})),
            ("set_sem_settings", json!({"type":"settings_set","api":1,"theme":"dark"})),
            ("get_com_campo", json!({"type":"settings_get","api":1,"x":1})),
        ]
    }

    fn agents() -> Vec<Agent> {
        vec![
            Agent { session_id: sid(), agent: "claude".into(), args: "--model opus".into(), label: "api".into(), cwd: Some("C:/proj/api".into()), state: AgentState::Running, last_activity_ms: 1_700_000_000_000, profile_id: None, waiting: true },
            Agent { session_id: uuid_v4_lower("2f1e2d3c-4b5a-4968-8778-695a4b3c2d1e").unwrap(), agent: "codex".into(), args: String::new(), label: String::new(), cwd: None, state: AgentState::Saved, last_activity_ms: 5, profile_id: None, waiting: false },
        ]
    }

    #[test]
    fn pedidos() {
        let wrap = |mut v: Value| { v["v"] = 1.into(); v["connection_id"] = CID.into(); v["request_id"] = RID.into(); v.to_string() };
        let ok = ["agent_list", "agent_resume", "agent_create", "agent_create_minimo", "agent_remove", "ssh_list", "ssh_connect", "ssh_save", "ssh_save_editar",
            "ssh_remove", "ssh_probe", "ssh_reconnect", "history_query", "history_query_vazio", "history_query_filtros", "history_run", "history_clear", "api_2",
            "folder_list", "folder_list_unidades", "ssh_key_list", "ssh_key_list_pasta", "agent_default_get", "agent_default_set", "agent_default_set_minimo",
            "agent_list_ext", "ssh_list_ext", "ssh_save_opcoes", "history_query_ext", "settings_get", "settings_set", "settings_set_tudo"];
        let mut cases = vec![];
        for (name, body) in requests() {
            let r = parse_control(&wrap(body.clone()), CID);
            assert_eq!(res(r.clone()), if ok.contains(&name) { "ok" } else { "INVALID_MESSAGE" }, "{name}");
            cases.push(json!({"name": name, "body": body, "result": res(r)}));
        }
        let get = |n: &str| { let b = requests().into_iter().find(|c| c.0 == n).unwrap().1; let Ok(Control::Request { req: P::Extra(r), .. }) = parse_control(&wrap(b), CID) else { panic!("{n}") }; r };
        assert_eq!(get("api_2"), Req::Unsupported);
        assert_eq!(get("history_query_vazio"), Req::HistoryQuery(HistoryFilter { query: String::new(), limit: 50, ..Default::default() }));
        assert_eq!(get("history_query_filtros"), Req::HistoryQuery(HistoryFilter { query: String::new(), limit: 200, days: Some(7), kind: Some("pwsh".into()), errors: true, ext: false }));
        assert_eq!(get("ssh_connect"), Req::SshConnect { server_id: "srv-1".into(), cols: 80, rows: 24 });
        assert_eq!(get("history_run"), Req::HistoryRun { entry_id: "h_42".into(), session_id: sid() });
        assert_eq!(get("agent_create_minimo"), Req::AgentCreate { agent: "codex".into(), args: String::new(), cwd: None, label: None, cols: 80, rows: 24 });
        assert_eq!(get("ssh_key_list"), Req::SshKeyList(String::new()));
        assert_eq!(get("folder_list"), Req::FolderList("C:\\proj".into()));
        assert_eq!(get("agent_default_set_minimo"), Req::AgentDefaultSet(AgentDefault { agent: "codex".into(), cwd: None, args: String::new() }));
        assert_eq!(get("agent_default_set"), Req::AgentDefaultSet(AgentDefault { agent: "claude".into(), cwd: Some("C:\\proj\\api".into()), args: "--model opus".into() }));
        assert_eq!(get("ssh_save"), Req::SshSave { server_id: None, label: "Produção".into(), target: crate::ssh::Ssh { user: "deploy".into(), host: "10.0.0.5".into(), port: 22, key: None, ..Default::default() } });
        assert_eq!(get("ssh_save_opcoes"), Req::SshSave { server_id: None, label: "Produção".into(), target: crate::ssh::Ssh { user: "deploy".into(), host: "10.0.0.5".into(), port: 22, key: None, keepalive_s: 30, auto_reconnect: true } });
        assert_eq!(get("agent_list"), Req::AgentList { ext: false });
        assert_eq!(get("agent_list_ext"), Req::AgentList { ext: true });
        assert_eq!(get("ssh_list_ext"), Req::SshList { ext: true });
        assert_eq!(get("history_query_ext"), Req::HistoryQuery(HistoryFilter { limit: 50, ext: true, ..Default::default() }));
        let Req::SettingsSet(m) = get("settings_set") else { panic!() };
        let a = Appearance::default().patch(&m).unwrap();
        assert_eq!((a.theme.as_str(), a.font_size, a.glow, a.blur), ("cosmo", 14, false, 30), "só os campos mandados mudam");
        // Pergunta de capacidades: um `session_attach` que o host 0.1.8 já entende (responde NOT_FOUND, não fatal).
        let probe = json!({"type": "session_attach", "session_id": CAPS_PROBE, "from_offset": null});
        let Ok(Control::Request { req: P::Attach { session_id, from_offset: None, size: None }, .. }) = parse_control(&wrap(probe.clone()), CID) else { panic!("probe") };
        assert!(is_caps_probe(&session_id) && !is_caps_probe(&sid()));
        let caps = json!({"request": probe, "reply": encode_control(CID, Some(RID), caps_ok()), "reply_host_antigo": encode_control(CID, Some(RID), crate::proto::error(NotFound)),
            "auth_ok_host_antigo": format!(r#"{{"type":"auth_ok","v":1,"connection_id":"{CID}"}}"#)});
        fixture(json!({"connection_id": CID, "caps": CAPS, "caps_probe": caps, "requests": cases, "replies": replies()}));
    }

    /// Respostas do host (construtores reais) e mutações; resultado esperado conferido no app.
    fn replies() -> Vec<Value> {
        let wrap = |b: Value| encode_control(CID, Some(RID), b);
        let with = |mut v: Value, path: &[&str], x: Value| { { let mut p = &mut v; for k in &path[..path.len() - 1] { p = if let Ok(i) = k.parse::<usize>() { &mut p[i] } else { &mut p[*k] }; } p[path[path.len() - 1]] = x; } v };
        let al = agent_list_ok(&agents(), false);
        let alx = agent_list_ok(&agents(), true);
        let servers = [SshServer { server_id: "srv-1".into(), label: "Produção".into(), host: "10.0.0.5".into(), user: "deploy".into(), port: 22, key: None, latency_ms: Some(38), status: "ok", keepalive_s: 30, auto_reconnect: true },
            SshServer { server_id: "srv-2".into(), label: "NAS".into(), host: "nas.local".into(), user: "admin".into(), port: 2222, key: Some("C:\\k".into()), latency_ms: None, status: "timeout", keepalive_s: 0, auto_reconnect: false }];
        let ss = [SshSession { session_id: sid(), label: "Produção".into(), connected: false }];
        let sl = ssh_list_ok(&servers, &ss, false);
        let slx = ssh_list_ok(&servers, &ss, true);
        let he = [HistoryEntry { entry_id: "h_42".into(), command: "git status".into(), cwd: Some("C:/proj".into()), at_ms: 1_700_000_000_000, exit_code: Some(0), kind: "pwsh".into(), label: "PowerShell".into(), pc: "Este PC".into() },
            HistoryEntry { entry_id: "h_43".into(), command: "npm test".into(), cwd: None, at_ms: 7, exit_code: None, kind: "ssh".into(), label: "srv".into(), pc: "Este PC".into() }];
        let hq = history_query_ok(&he, None);
        let hqx = history_query_ok(&he, Some(&summarize(&he, -180)));
        let ap = Appearance::default();
        let cases: Vec<(&str, String, &str)> = vec![
            ("agent_list_ok", wrap(al.clone()), "ok"),
            ("agent_list_vazia", wrap(agent_list_ok(&[], false)), "ok"),
            ("agent_list_ext", wrap(alx.clone()), "ok"),
            ("ssh_list_ext", wrap(slx.clone()), "ok"),
            ("history_query_ext", wrap(hqx.clone()), "ok"),
            ("settings_get_ok", wrap(settings_ok("settings_get_ok", &ap)), "ok"),
            ("settings_set_ok", wrap(settings_ok("settings_set_ok", &Appearance { theme: "cosmo".into(), ..ap.clone() })), "ok"),
            ("agent_waiting", encode_control(CID, None, agent_waiting_event(&sid(), "api", true)), "ok"),
            ("agente_waiting_sem_ext", wrap(with(al.clone(), &["agents", "0", "state"], "waiting".into())), "ok"),
            ("ssh_keepalive_texto", wrap(with(slx.clone(), &["servers", "0", "keepalive_s"], "30".into())), "INVALID_MESSAGE"),
            ("ssh_reconectar_numero", wrap(with(slx.clone(), &["servers", "0", "auto_reconnect"], 1.into())), "INVALID_MESSAGE"),
            ("sessoes_count_texto", wrap(with(hqx.clone(), &["sessions", "0", "count"], "2".into())), "INVALID_MESSAGE"),
            ("sessoes_dia_invalido", wrap(with(hqx.clone(), &["sessions", "0", "day"], "17/11/2026".into())), "INVALID_MESSAGE"),
            ("settings_tema_invalido", wrap(with(settings_ok("settings_get_ok", &ap), &["settings", "theme"], "neon".into())), "INVALID_MESSAGE"),
            ("settings_campo_extra", wrap(with(settings_ok("settings_get_ok", &ap), &["settings", "x"], 1.into())), "INVALID_MESSAGE"),
            ("settings_sem_campo", wrap({ let mut v = settings_ok("settings_get_ok", &ap); v["settings"].as_object_mut().unwrap().remove("glow"); v }), "INVALID_MESSAGE"),
            ("waiting_com_request_id", wrap(agent_waiting_event(&sid(), "api", true)), "INVALID_MESSAGE"),
            ("waiting_texto", encode_control(CID, None, with(agent_waiting_event(&sid(), "api", true), &["waiting"], "sim".into())), "INVALID_MESSAGE"),
            ("agent_resume_ok", wrap(session_ok("agent_resume_ok", &sid())), "ok"),
            ("agent_create_ok", wrap(session_ok("agent_create_ok", &sid())), "ok"),
            ("agent_remove_ok", wrap(empty_ok("agent_remove_ok")), "ok"),
            ("ssh_list_ok", wrap(sl.clone()), "ok"),
            ("ssh_list_vazia", wrap(ssh_list_ok(&[], &[], false)), "ok"),
            ("ssh_connect_ok", wrap(session_ok("ssh_connect_ok", &sid())), "ok"),
            ("ssh_reconnect_ok", wrap(session_ok("ssh_reconnect_ok", &sid())), "ok"),
            ("ssh_save_ok", wrap(ssh_save_ok("srv-1")), "ok"),
            ("ssh_remove_ok", wrap(empty_ok("ssh_remove_ok")), "ok"),
            ("ssh_probe_ok", wrap(empty_ok("ssh_probe_ok")), "ok"),
            ("history_query_ok", wrap(hq.clone()), "ok"),
            ("history_query_vazia", wrap(history_query_ok(&[], None)), "ok"),
            ("history_run_ok", wrap(history_run_ok()), "ok"),
            ("history_clear_ok", wrap(empty_ok("history_clear_ok")), "ok"),
            ("folder_list_ok", wrap(folder_list_ok("C:\\proj", &["api".into(), "web".into()])), "ok"),
            ("folder_list_unidades", wrap(folder_list_ok("", &["C:\\".into(), "D:\\".into()])), "ok"),
            ("ssh_key_list_ok", wrap(ssh_key_list_ok("C:\\Users\\eu\\.ssh", "C:\\Users\\eu\\.ssh", &["work".into()], &["id_ed25519".into(), "id_rsa".into()])), "ok"),
            ("ssh_key_list_vazia", wrap(ssh_key_list_ok("C:\\Users\\eu\\.ssh", "C:\\Users\\eu\\.ssh\\work", &[], &[])), "ok"),
            ("agent_default_ok", wrap(agent_default_ok(Some(&AgentDefault { agent: "claude".into(), cwd: Some("C:\\proj\\api".into()), args: "--model opus".into() }))), "ok"),
            ("agent_default_sem_cwd", wrap(agent_default_ok(Some(&AgentDefault { agent: "codex".into(), cwd: None, args: String::new() }))), "ok"),
            ("agent_default_nunca_salvo", wrap(agent_default_ok(None)), "ok"),
            ("agent_default_set_ok", wrap(empty_ok("agent_default_set_ok")), "ok"),
            ("folder_dir_numero", wrap(json!({"type":"folder_list_ok","api":1,"path":"C:\\","dirs":[1]})), "INVALID_MESSAGE"),
            ("folder_sem_path", wrap(json!({"type":"folder_list_ok","api":1,"dirs":[]})), "INVALID_MESSAGE"),
            ("chave_sem_keys", wrap(json!({"type":"ssh_key_list_ok","api":1,"root":"C:\\","path":"C:\\","dirs":[]})), "INVALID_MESSAGE"),
            ("chave_nome_com_barra", wrap(json!({"type":"ssh_key_list_ok","api":1,"root":"C:\\s","path":"C:\\s","dirs":[],"keys":["..\\x"]})), "INVALID_MESSAGE"),
            ("padrao_agente_desconhecido", wrap(json!({"type":"agent_default_ok","api":1,"agent":"bash","cwd":null,"args":""})), "INVALID_MESSAGE"),
            ("padrao_sem_args", wrap(json!({"type":"agent_default_ok","api":1,"agent":"claude","cwd":null})), "INVALID_MESSAGE"),
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
        let v = history_query_ok(&big, None);
        assert!(v.to_string().len() <= MAX_BODY);
        let n = v["entries"].as_array().unwrap().len();
        assert!(n > 0 && n < 200);
        assert_eq!(v["entries"][0]["command"].as_str().unwrap().chars().count(), MAX_TEXT);
        let servers: Vec<SshServer> = (0..200).map(|i| SshServer { server_id: format!("s{i}"), label: "l".repeat(600), host: "h".repeat(253), user: "u".into(), port: 22, key: Some("k".repeat(600)), latency_ms: None, status: "unknown", keepalive_s: 0, auto_reconnect: false }).collect();
        let v = ssh_list_ok(&servers, &[SshSession { session_id: sid(), label: "x".into(), connected: true }], true);
        assert!(v.to_string().len() <= MAX_BODY);
        assert_eq!(v["sessions"].as_array().unwrap().len(), 1, "sessões ficam mesmo cortando servidores");
    }

    #[test]
    fn chaves_so_dentro_da_ssh() {
        let base = std::env::temp_dir().join(format!("live-cmd-chaves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join(".ssh");
        std::fs::create_dir_all(root.join("work")).unwrap();
        std::fs::create_dir_all(base.join("fora")).unwrap();
        for f in ["id_ed25519", "id_ed25519.pub", "config", "known_hosts", "Known_Hosts.old", "authorized_keys", "B_rsa"] { std::fs::write(root.join(f), "k").unwrap(); }
        std::fs::write(root.join("grande"), vec![b'x'; 65 << 10]).unwrap();
        std::fs::write(root.join("work").join("deploy"), "k").unwrap();
        let (shown, dirs, keys) = ssh_keys(&root, "").unwrap();
        assert_eq!(shown, root.to_string_lossy());
        assert_eq!(dirs, vec!["work"]);
        assert_eq!(keys, vec!["B_rsa", "id_ed25519"], "sem .pub, config, known_hosts, authorized_keys nem arquivo grande");
        let (shown, _, keys) = ssh_keys(&root, &root.join("work").to_string_lossy()).unwrap();
        assert_eq!((shown, keys), (root.join("work").to_string_lossy().into_owned(), vec!["deploy".to_owned()]));
        // Fora da `.ssh`, relativo, inexistente ou arquivo: NOT_FOUND.
        for p in [base.join("fora").to_string_lossy().into_owned(), root.join("..").join("fora").to_string_lossy().into_owned(), "work".into(),
            root.join("nada").to_string_lossy().into_owned(), root.join("id_ed25519").to_string_lossy().into_owned(), r"C:\Windows".into()] {
            assert_eq!(ssh_keys(&root, &p), Err(NotFound), "{p}");
        }
        assert_eq!(ssh_keys(&base.join("sem-ssh"), ""), Err(NotFound));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn listas_cabem_numa_mensagem() {
        let dirs: Vec<String> = (0..800).map(|i| format!("{i}-{}", "d".repeat(300))).collect();
        let v = folder_list_ok("C:\\x", &dirs);
        assert!(v.to_string().len() <= MAX_BODY);
        assert!(!v["dirs"].as_array().unwrap().is_empty());
        let few: Vec<String> = (0..800).map(|i| format!("p{i}")).collect();
        assert_eq!(folder_list_ok("C:\\x", &few)["dirs"].as_array().unwrap().len(), MAX_ENTRIES);
        let v = ssh_key_list_ok("r", "r", &dirs, &["id".into()]);
        assert!(v.to_string().len() <= MAX_BODY);
        assert_eq!(v["keys"][0], "id");
    }

    #[test]
    fn espera_de_confirmacao() {
        assert!(waiting_prompt("● Bash(rm -rf build)\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n"));
        assert!(waiting_prompt("Allow command? (y/n)\n"));
        assert!(waiting_prompt("› Approve this edit?\n  Yes, and don't ask again\n"));
        assert!(!waiting_prompt("> escreva sua mensagem\n"));
        // Pergunta antiga, já rolada para cima, não conta.
        let old = format!("Do you want to proceed?\n{}", "linha\n".repeat(20));
        assert!(!waiting_prompt(&old));
    }

    #[test]
    fn resumo_por_sessao_e_dia() {
        assert_eq!(local_day(0, 0), "1970-01-01");
        assert_eq!(local_day(1_700_000_000_000, 0), "2023-11-14");
        assert_eq!(local_day(1_700_000_000_000, -180 * 4), "2023-11-14");
        assert_eq!(local_day(951_782_400_000, 0), "2000-02-29");
        assert_eq!(local_day(1_699_920_000_000, -180), "2023-11-13", "fuso do Brasil volta o dia");
        let e = |id: &str, at: u64, code: Option<i32>, label: &str| HistoryEntry { entry_id: id.into(), command: "x".into(), cwd: None, at_ms: at, exit_code: code, kind: "pwsh".into(), label: label.into(), pc: "Este PC".into() };
        let day = 1_700_000_000_000;
        let g = summarize(&[e("1", day, Some(0), "A"), e("2", day + 1000, Some(1), "A"), e("3", day + 2000, None, "B"), e("4", day - 86_400_000, Some(0), "A")], 0);
        assert_eq!(g.len(), 3);
        assert_eq!((g[0].label.as_str(), g[0].count), ("B", 1));
        assert_eq!((g[1].label.as_str(), g[1].count, g[1].errors, g[1].day.as_str()), ("A", 2, 1, "2023-11-14"));
        assert_eq!(g[2].day, "2023-11-13");
    }

    #[test]
    fn aparencia_valida() {
        let a = Appearance::default();
        assert!(a.valid());
        let m = |v: Value| v.as_object().unwrap().clone();
        assert_eq!(a.patch(&m(json!({"opacity": 82}))).unwrap().opacity, 82);
        assert!(a.patch(&m(json!({"opacity": 101}))).is_err());
        assert!(a.patch(&m(json!({"font_family": "Fira Code\u{0}"}))).is_err());
        let back: Appearance = serde_json::from_value(json!({"theme": "ametista"})).unwrap();
        assert_eq!(back, Appearance { theme: "ametista".into(), ..a.clone() }, "settings.json antigo ou parcial cai no padrão");
        assert_eq!((legacy_theme("light"), legacy_theme("contrast"), legacy_theme("cosmo")), ("dark", "dark", "cosmo"));
        assert!(a.patch(&m(json!({"theme": "light"}))).is_err(), "claro e alto contraste saíram da lista branca");
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
        assert_eq!(p.agent_default(), None);
        assert_eq!(p.agent_default_save(&AgentDefault::default()), Err("NOT_FOUND".into()));
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

//! Tela "Agentes de IA": registro local (`agents.json`) das sessões de agente criadas neste app,
//! para listar por pasta raiz e retomar com `--resume` depois que a sessão foi encerrada.
//! O estado ao vivo vem do `session_list` da conexão; o registro só guarda o que o host esquece no close.
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::AppHandle;

use crate::{settings, term};

const FILE: &str = "agents.json";
/// Entradas guardadas; as usadas há mais tempo saem primeiro.
const MAX: usize = 200;
/// Leitura-alteração-gravação do arquivo em série (comandos Tauri rodam em threads).
static LOCK: Mutex<()> = Mutex::new(());

/// Uma sessão de agente conhecida. `session_id` é também o id da conversa do CLI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Agent {
    pub conn: String,
    pub session_id: String,
    pub profile_id: String,
    pub agent: String,
    #[serde(default)] pub args: String,
    #[serde(default)] pub cwd: Option<String>,
    #[serde(default)] pub label: String,
    #[serde(default)] pub used_ms: u64,
}

fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }

pub fn read(dir: &Path) -> Vec<Agent> {
    std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write(dir: &Path, list: &[Agent]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("agents.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(list).map_err(|_| "IO_ERROR")?).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|_| "IO_ERROR".into())
}

/// Aplica `f` à lista do disco e grava, com o lock tomado.
fn edit(dir: &Path, f: impl FnOnce(&mut Vec<Agent>)) -> Result<(), String> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = read(dir);
    f(&mut list);
    list.sort_by(|a, b| b.used_ms.cmp(&a.used_ms));
    list.truncate(MAX);
    write(dir, &list)
}

/// Registra (ou atualiza) a sessão como a usada mais recentemente.
pub fn record(dir: &Path, mut a: Agent) -> Result<(), String> {
    a.used_ms = now_ms();
    edit(dir, |l| { l.retain(|x| !(x.conn == a.conn && x.session_id == a.session_id)); l.insert(0, a); })
}

/// "Salvar como padrão" da nova sessão de agente (`agent-default.json`): o diálogo desktop e o celular
/// (`agent_default_get`/`agent_default_set`, via `mobile.rs`) leem e gravam o mesmo arquivo.
const DEFAULT_FILE: &str = "agent-default.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct AiDefault {
    pub agent: String,
    #[serde(default)] pub cwd: Option<String>,
    #[serde(default)] pub args: String,
}

/// Mesmas regras do host para agente, comandos extras e pasta; apara espaços e pasta vazia vira `None`.
fn checked(mut d: AiDefault) -> Result<AiDefault, String> {
    d.args = d.args.trim().into();
    d.cwd = d.cwd.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty());
    let cwd_ok = d.cwd.as_ref().is_none_or(|c| c.chars().count() <= 1024 && !c.chars().any(char::is_control));
    if !live_cmd_echo::AGENTS.contains(&d.agent.as_str()) || !live_cmd_echo::valid_args(&d.args) || !cwd_ok { return Err("INVALID_MESSAGE".into()); }
    Ok(d)
}

/// Padrão salvo; ausente, ilegível ou inválido (arquivo editado à mão) = nenhum.
pub fn read_default(dir: &Path) -> Option<AiDefault> {
    std::fs::read(dir.join(DEFAULT_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).and_then(|d| checked(d).ok())
}

pub fn save_default(dir: &Path, d: AiDefault) -> Result<AiDefault, String> {
    let d = checked(d)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("agent-default.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&d).map_err(|_| "IO_ERROR")?).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(DEFAULT_FILE)).map_err(|_| "IO_ERROR")?;
    Ok(d)
}

#[tauri::command]
pub fn agent_default_get() -> Option<AiDefault> { read_default(&settings::data_dir().ok()?) }

#[tauri::command]
pub fn agent_default_set(agent: String, cwd: Option<String>, args: String) -> Result<AiDefault, String> {
    save_default(&settings::data_dir()?, AiDefault { agent, cwd, args })
}

pub fn remove_entry(dir: &Path, conn: &str, sid: &str) -> Result<(), String> {
    edit(dir, |l| l.retain(|x| !(x.conn == conn && x.session_id == sid)))
}

/// Pasta raiz do agrupamento: (chave normalizada, nome exibido). Sem pasta, a pasta inicial do perfil ("~").
pub fn root(cwd: Option<&str>) -> (String, String) {
    let p = cwd.unwrap_or("").trim().trim_end_matches(['/', '\\']);
    let name = p.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("~").to_owned();
    (p.replace('/', "\\").to_lowercase(), name)
}

/// Monta a resposta da tela: grupos por pasta, mais recente primeiro, com o estado ao vivo.
/// `live`: `sessions` do `session_list_ok`, ou `None` quando a conexão não respondeu.
pub fn view(list: &[Agent], conn: &str, live: Option<&Value>) -> Value {
    let mut mine: Vec<&Agent> = list.iter().filter(|a| a.conn == conn).collect();
    mine.sort_by(|a, b| b.used_ms.cmp(&a.used_ms));
    let mut groups: Vec<(String, String, String, Vec<Value>)> = vec![];
    for a in mine {
        let s = live.and_then(|l| l.as_array()).and_then(|l| l.iter().find(|s| s["session_id"] == a.session_id.as_str()));
        let status = match (live, s) {
            (None, _) => "desconhecido",
            (_, Some(s)) if s["state"] == "exited" => "saiu",
            (_, Some(_)) => "rodando",
            (_, None) => "encerrada",
        };
        let label = s.and_then(|s| s["label"].as_str()).filter(|l| !l.is_empty()).unwrap_or(&a.label);
        let item = json!({ "session_id": a.session_id, "agent": a.agent, "args": a.args, "label": label, "status": status, "used_ms": a.used_ms });
        let (key, name) = root(a.cwd.as_deref());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.3.push(item),
            None => groups.push((key, name, a.cwd.clone().unwrap_or_default(), vec![item])),
        }
    }
    json!({ "online": live.is_some(), "groups": groups.into_iter().map(|(_, name, path, sessions)| json!({ "name": name, "path": path, "sessions": sessions })).collect::<Vec<_>>() })
}

fn dir() -> Result<std::path::PathBuf, String> { settings::data_dir() }

/// `sessions` ao vivo da conexão; `None` se ela não responde (PC remoto desconectado).
fn live(app: &AppHandle, conn: &str) -> Option<Value> {
    term::term_list(app.clone(), conn.to_owned()).ok()
}

#[tauri::command(async)]
pub fn agent_list(app: AppHandle, conn: String) -> Result<Value, String> {
    let l = live(&app, &conn);
    Ok(view(&read(&dir()?), &conn, l.as_ref().map(|v| &v["sessions"])))
}

/// Retoma: sessão viva só volta para a frente; encerrada é recriada no host com o mesmo id e `--resume`.
/// Devolve `{session_id, label, state}` para a WebView abrir a aba.
#[tauri::command(async)]
pub fn agent_resume(app: AppHandle, conn: String, session_id: String, cols: u16, rows: u16) -> Result<Value, String> {
    let d = dir()?;
    let a = read(&d).into_iter().find(|a| a.conn == conn && a.session_id == session_id).ok_or("NOT_FOUND")?;
    let list = live(&app, &conn).ok_or("CONNECTION_LOST")?;
    if let Some(s) = list["sessions"].as_array().and_then(|l| l.iter().find(|s| s["session_id"] == session_id.as_str())) {
        record(&d, a)?;
        return Ok(json!({ "session_id": session_id, "label": s["label"], "state": s["state"] }));
    }
    // Perfil apagado no host desde a criação: usa o primeiro disponível.
    let profiles = list["profiles"].as_array().cloned().unwrap_or_default();
    let profile = profiles.iter().find(|p| p["profile_id"] == a.profile_id.as_str()).or(profiles.first())
        .and_then(|p| p["profile_id"].as_str()).ok_or("NOT_FOUND")?.to_owned();
    let sid = term::term_create(app, conn, profile, a.label.clone(), cols, rows, a.cwd.clone(), Some(a.agent.clone()), Some(a.args.clone()), Some(session_id))?;
    Ok(json!({ "session_id": sid, "label": a.label, "state": "running" }))
}

/// Tira a sessão da lista; se ainda estiver aberta no host, encerra o terminal antes.
#[tauri::command(async)]
pub fn agent_remove(app: AppHandle, conn: String, session_id: String) -> Result<(), String> {
    if let Some(l) = live(&app, &conn) {
        if l["sessions"].as_array().is_some_and(|s| s.iter().any(|s| s["session_id"] == session_id.as_str())) {
            term::term_close(app.clone(), conn.clone(), session_id.clone())?;
        }
    }
    remove_entry(&dir()?, &conn, &session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ag(conn: &str, sid: &str, cwd: Option<&str>, used: u64) -> Agent {
        Agent { conn: conn.into(), session_id: sid.into(), profile_id: "p".into(), agent: "claude".into(), args: String::new(), cwd: cwd.map(Into::into), label: format!("l-{sid}"), used_ms: used }
    }

    #[test]
    fn root_normaliza_pasta() {
        assert_eq!(root(Some(r"C:\Projetos\App\")), (r"c:\projetos\app".into(), "App".into()));
        assert_eq!(root(Some("C:/Projetos/App")), (r"c:\projetos\app".into(), "App".into()));
        assert_eq!(root(None), (String::new(), "~".into()));
        assert_eq!(root(Some("C:\\")), ("c:".into(), "C:".into()));
    }

    #[test]
    fn view_agrupa_e_marca_estado() {
        let list = vec![ag("local", "a", Some(r"C:\x\app"), 3), ag("local", "b", Some("C:/x/app/"), 2), ag("local", "c", None, 5), ag("pc2", "d", None, 9)];
        let live = json!([{"session_id":"a","state":"running","label":"novo"},{"session_id":"b","state":"exited","label":""}]);
        let v = view(&list, "local", Some(&live));
        let g = v["groups"].as_array().unwrap();
        assert_eq!(g.len(), 2, "outra conexão fica fora; mesma pasta com barra diferente junta");
        assert_eq!((g[0]["name"].as_str(), g[0]["sessions"][0]["status"].as_str()), (Some("~"), Some("encerrada")));
        assert_eq!(g[1]["name"], "app");
        assert_eq!((g[1]["sessions"][0]["status"].as_str(), g[1]["sessions"][0]["label"].as_str()), (Some("rodando"), Some("novo")));
        assert_eq!((g[1]["sessions"][1]["status"].as_str(), g[1]["sessions"][1]["label"].as_str()), (Some("saiu"), Some("l-b")));
        let off = view(&list, "local", None);
        assert_eq!((off["online"].as_bool(), off["groups"][0]["sessions"][0]["status"].as_str()), (Some(false), Some("desconhecido")));
    }

    #[test]
    fn registro_grava_atualiza_e_remove() {
        let d = std::env::temp_dir().join(format!("live-cmd-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        record(&d, ag("local", "a", None, 0)).unwrap();
        record(&d, ag("local", "b", None, 0)).unwrap();
        let mut a2 = ag("local", "a", Some(r"C:\y"), 0);
        a2.args = "--model opus".into();
        record(&d, a2).unwrap();
        let l = read(&d);
        assert_eq!(l.len(), 2, "mesmo id não duplica");
        assert_eq!((l[0].session_id.as_str(), l[0].args.as_str()), ("a", "--model opus"));
        remove_entry(&d, "local", "a").unwrap();
        assert_eq!(read(&d).iter().map(|a| a.session_id.as_str()).collect::<Vec<_>>(), ["b"]);
        std::fs::write(d.join(FILE), "{").unwrap();
        assert!(read(&d).is_empty(), "arquivo ilegível vira lista vazia");
        let _ = std::fs::remove_dir_all(d);
    }
}

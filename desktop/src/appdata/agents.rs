//! Registro local (`agents.json`) das sessões de agente e o padrão do diálogo (`agent-default.json`),
//! lidos e gravados pela tela Agentes do app e pelo provedor do celular no daemon.
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
    if !crate::AGENTS.contains(&d.agent.as_str()) || !crate::valid_args(&d.args) || !cwd_ok { return Err("INVALID_MESSAGE".into()); }
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

pub fn remove_entry(dir: &Path, conn: &str, sid: &str) -> Result<(), String> {
    edit(dir, |l| l.retain(|x| !(x.conn == conn && x.session_id == sid)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ag(conn: &str, sid: &str, cwd: Option<&str>, used: u64) -> Agent {
        Agent { conn: conn.into(), session_id: sid.into(), profile_id: "p".into(), agent: "claude".into(), args: String::new(), cwd: cwd.map(Into::into), label: format!("l-{sid}"), used_ms: used }
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

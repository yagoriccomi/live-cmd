//! Tela "Agentes de IA": registro local (`agents.json`) das sessões de agente criadas neste app,
//! para listar por pasta raiz e retomar com `--resume` depois que a sessão foi encerrada.
//! O estado ao vivo vem do `session_list` da conexão; o registro só guarda o que o host esquece no close.

use serde_json::{Value, json};
use tauri::AppHandle;

use crate::{settings, term};

pub use live_cmd_echo::appdata::agents::{Agent, read, record, remove_entry};

/// Padrão do agente (agente + comandos extras) salvo no host da conexão (#15):
/// todo cliente do mesmo PC lê o mesmo. `null` quando nunca foi salvo.
#[tauri::command(async)]
pub fn agent_default_get(app: AppHandle, conn: String) -> Result<Value, String> {
    let v = term::client(&app, &conn)?.request(json!({ "type": "agent_default_get" }))?;
    Ok(if v["agent"].is_string() { json!({ "agent": v["agent"], "args": v["args"] }) } else { Value::Null })
}

#[tauri::command(async)]
pub fn agent_default_set(app: AppHandle, conn: String, agent: String, args: String) -> Result<(), String> {
    term::client(&app, &conn)?.request(json!({ "type": "agent_default_set", "agent": agent, "cwd": null, "args": args })).map(drop)
}

/// Ícone enviado pelo usuário, salvo no host (#40): data URL ou `null`.
#[tauri::command(async)]
pub fn agent_icon_get(app: AppHandle, conn: String, agent: String) -> Result<Value, String> {
    term::client(&app, &conn)?.request(json!({ "type": "agent_icon_get", "agent": agent })).map(|v| v["image"].clone())
}

/// `image`: base64 de PNG/JPEG/WebP até 48 KB (o host confere); `null` apaga.
#[tauri::command(async)]
pub fn agent_icon_set(app: AppHandle, conn: String, agent: String, image: Option<String>) -> Result<(), String> {
    term::client(&app, &conn)?.request(json!({ "type": "agent_icon_set", "agent": agent, "image": image })).map(drop)
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

}

//! Tela "Servidores SSH": cadastro em `ssh.json`, teste de alcance TCP e "Conectar", que abre uma
//! sessão local rodando o `ssh.exe` do Windows (o destino sai do disco, nunca de texto da UI).
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager, State};

use crate::{App, settings, term};

#[tauri::command]
pub fn ssh_list(app: State<App>) -> Result<Vec<settings::SshServer>, String> { settings::ssh_servers(&app.data_dir) }

#[tauri::command]
pub fn ssh_save(app: State<App>, server: settings::SshServer) -> Result<settings::SshServer, String> {
    settings::save_ssh_server(&app.data_dir, server)
}

#[tauri::command]
pub fn ssh_remove(app: State<App>, id: String) -> Result<(), String> { settings::remove_ssh_server(&app.data_dir, &id) }

/// Abre `porta` no host em até `timeout`; `{ok, ms}` ou `{ok:false, code}` (`DNS`, `TIMEOUT`, `OFFLINE`).
pub fn probe(host: &str, port: u16, timeout: Duration) -> Value {
    if !live_cmd_echo::ssh::valid_host(host) || port == 0 { return json!({ "ok": false, "code": "INVALID_SSH_HOST" }); }
    let Ok(addrs) = (host, port).to_socket_addrs() else { return json!({ "ok": false, "code": "DNS" }) };
    let mut code = "DNS";
    for a in addrs {
        let t = Instant::now();
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(_) => return json!({ "ok": true, "ms": t.elapsed().as_millis() as u64 }),
            Err(e) => code = if e.kind() == std::io::ErrorKind::TimedOut { "TIMEOUT" } else { "OFFLINE" },
        }
    }
    json!({ "ok": false, "code": code })
}

/// Status da lista: alcance TCP da porta, com latência.
#[tauri::command(async)]
pub fn ssh_probe(host: String, port: u16) -> Value { probe(&host, port, Duration::from_secs(3)) }

/// Nova sessão neste PC com o ssh.exe do servidor `id`; devolve o `session_id` para a UI anexar.
#[tauri::command(async)]
pub fn ssh_connect(app: AppHandle, id: String, cols: u16, rows: u16) -> Result<String, String> {
    let s = settings::ssh_servers(&app.state::<App>().data_dir)?.into_iter().find(|s| s.id == id).ok_or("NOT_FOUND")?;
    let target = s.target();
    target.check()?;
    let label: String = s.name.chars().take(64).collect();
    let req = json!({ "type": "session_create", "profile_id": "ssh", "label": label, "cols": cols, "rows": rows, "ssh": target });
    let v = term::client(&app, term::LOCAL)?.request(req)?;
    Ok(v["session_id"].as_str().unwrap_or_default().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alcance_tcp() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let ok = probe("127.0.0.1", port, Duration::from_secs(2));
        assert_eq!(ok["ok"], true, "{ok}");
        assert!(ok["ms"].is_u64());
        drop(l);
        assert_eq!(probe("127.0.0.1", port, Duration::from_secs(2))["ok"], false, "porta fechada");
        assert_eq!(probe("-oProxyCommand=x", 22, Duration::from_secs(1))["code"], "INVALID_SSH_HOST");
        assert_eq!(probe("h", 0, Duration::from_secs(1))["code"], "INVALID_SSH_HOST");
    }
}

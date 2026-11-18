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

/// `Host` do `~/.ssh/config` como servidores (sem curinga); `HostName`, `Port`, `User`, `IdentityFile`.
/// ponytail: sem `Include`/`Match`; entradas inválidas são puladas.
pub fn parse_config(text: &str) -> Vec<settings::SshServer> {
    let mut out: Vec<settings::SshServer> = vec![];
    let mut cur: Vec<usize> = vec![];
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (k, v) = line.split_once(|c: char| c.is_whitespace() || c == '=').unwrap_or((line, ""));
        let v = v.trim_start_matches(|c: char| c.is_whitespace() || c == '=').trim().trim_matches('"');
        match k.to_ascii_lowercase().as_str() {
            "host" => {
                cur.clear();
                for a in v.split_whitespace().filter(|a| !a.contains(['*', '?', '!'])) {
                    cur.push(out.len());
                    out.push(settings::SshServer { name: a.chars().take(40).collect(), host: a.into(), port: 22, ..Default::default() });
                }
            }
            "match" => cur.clear(),
            key => for &i in &cur {
                let s = &mut out[i];
                match key {
                    "hostname" => s.host = v.into(),
                    "port" => s.port = v.parse().unwrap_or(22),
                    "user" => s.user = v.into(),
                    "identityfile" => s.key = Some(v.replacen('~', &home, 1).replace('/', "\\")),
                    _ => {}
                }
            },
        }
    }
    out.retain(|s| s.target().check().is_ok());
    out
}

/// Importa `%USERPROFILE%\.ssh\config`; pula quem já existe (mesmo nome). Devolve quantos entraram.
#[tauri::command]
pub fn ssh_import(app: State<App>) -> Result<usize, String> {
    let p = std::path::PathBuf::from(std::env::var("USERPROFILE").map_err(|_| "NOT_FOUND")?).join(".ssh").join("config");
    let text = std::fs::read_to_string(p).map_err(|_| "NOT_FOUND")?;
    let have: Vec<String> = settings::ssh_servers(&app.data_dir)?.into_iter().map(|s| s.name).collect();
    let mut n = 0;
    for s in parse_config(&text).into_iter().filter(|s| !have.contains(&s.name)) {
        settings::save_ssh_server(&app.data_dir, s)?;
        n += 1;
    }
    Ok(n)
}

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

/// Sessões SSH do host deste PC (`[{session_id, label, state: connected|disconnected}]`), pelo mesmo
/// pedido `ssh_list` do celular: as restauradas ao reiniciar voltam desconectadas, à espera de Reconectar.
#[tauri::command(async)]
pub fn ssh_sessions(app: AppHandle) -> Result<Value, String> {
    let v = term::client(&app, term::LOCAL)?.request(json!({ "type": "ssh_list", "api": live_cmd_echo::extras::API }))?;
    Ok(v["sessions"].clone())
}

/// Abre de novo o ssh.exe de uma sessão desconectada, com o mesmo id (a aba continua a mesma).
#[tauri::command(async)]
pub fn ssh_reconnect(app: AppHandle, session_id: String, cols: u16, rows: u16) -> Result<String, String> {
    let req = json!({ "type": "ssh_reconnect", "api": live_cmd_echo::extras::API, "session_id": session_id, "cols": cols, "rows": rows });
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
        let c = parse_config("Host a b
  HostName 10.0.0.1
  Port 2222
Host *
  User x
Host c
  User=deploy
");
        assert_eq!(c.iter().map(|s| (s.name.as_str(), s.host.as_str(), s.port, s.user.as_str())).collect::<Vec<_>>(),
            [("a", "10.0.0.1", 2222, ""), ("b", "10.0.0.1", 2222, ""), ("c", "c", 22, "deploy")]);
        assert_eq!(probe("h", 0, Duration::from_secs(1))["code"], "INVALID_SSH_HOST");
    }
}

//! Tela "Servidores SSH": cadastro em `ssh.json`, teste de alcance TCP e "Conectar", que abre uma
//! sessão local rodando o `ssh.exe` do Windows (o destino sai do disco, nunca de texto da UI).
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

/// Teste de alcance TCP (o mesmo do provedor do celular no daemon).
pub use live_cmd_echo::appdata::probe;

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

/// Servidor de uma aba SSH pelo destino que o host lista (`{user, host, port}`); a chave sai do `ssh.json`.
pub fn server_for(app: &AppHandle, t: &Value) -> Result<settings::SshServer, String> {
    let s = settings::ssh_servers(&app.state::<App>().data_dir)?.into_iter()
        .find(|s| s.host == t["host"].as_str().unwrap_or_default() && s.user == t["user"].as_str().unwrap_or_default() && Some(s.port as u64) == t["port"].as_u64())
        .ok_or("NOT_FOUND")?;
    s.target().check()?;
    Ok(s)
}

/// Texto entre aspas simples do sh remoto (`'` vira `'\''`).
pub fn sq(s: &str) -> String { format!("'{}'", s.replace('\'', r"'\''")) }

/// Roda `cmd` no sh do servidor sem PTY. `BatchMode`: só chave/agente; `timeout` mata o ssh.exe.
// ponytail: uma conexão ssh por consulta (git/CPU a cada 15/3 s); OpenSSH do Windows não tem ControlMaster.
pub fn exec(s: &settings::SshServer, cmd: &str, timeout: Duration) -> Option<String> { exec_in(s, cmd, None, timeout) }

/// `exec` com `input` no stdin do comando remoto.
pub fn exec_in(s: &settings::SshServer, cmd: &str, input: Option<Vec<u8>>, timeout: Duration) -> Option<String> {
    let mut c = std::process::Command::new(live_cmd_echo::ssh::exe());
    c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=3", "-T", "-p", &s.port.to_string()]);
    if !s.user.is_empty() { c.arg("-l").arg(&s.user); }
    if let Some(k) = &s.key { c.arg("-i").arg(k); }
    // `--` antes do host: nada depois vira opção do ssh.
    c.arg("--").arg(&s.host).arg(cmd);
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
    let stdin = if input.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() };
    let mut ch = c.stdin(stdin).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    if let (Some(data), Some(mut w)) = (input, ch.stdin.take()) { std::thread::spawn(move || { let _ = std::io::Write::write_all(&mut w, &data); }); }
    let mut out = ch.stdout.take()?;
    let rd = std::thread::spawn(move || { let mut v = Vec::new(); std::io::Read::read_to_end(&mut out, &mut v).map(|_| v) });
    let end = Instant::now() + timeout;
    let st = loop {
        if let Some(st) = ch.try_wait().ok()? { break st; }
        if Instant::now() > end { let _ = ch.kill(); let _ = ch.wait(); return None; }
        std::thread::sleep(Duration::from_millis(20));
    };
    let v = rd.join().ok()?.ok()?;
    st.success().then(|| String::from_utf8_lossy(&v).into_owned())
}

/// Grava `data` no servidor: `<cwd>/<sub>` ou `~/<fallback>` (cria a pasta). Devolve o caminho absoluto remoto.
/// Vai pelo stdin do ssh (`cat >`), sem scp: o caminho remoto não passa por outra regra de aspas.
pub fn put(s: &settings::SshServer, cwd: Option<&str>, sub: &str, fallback: &str, name: &str, data: Vec<u8>) -> Result<String, String> {
    put_via(|cmd, d| exec_in(s, cmd, Some(d), Duration::from_secs(120)), cwd, sub, fallback, name, data)
}

/// [`put`] sobre um transporte qualquer (`cmd`, stdin) -> stdout; os testes trocam o ssh por um falso.
fn put_via(exec: impl Fn(&str, Vec<u8>) -> Option<String>, cwd: Option<&str>, sub: &str, fallback: &str, name: &str, data: Vec<u8>) -> Result<String, String> {
    let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\') && !c.is_control()).collect();
    if name.is_empty() || name == "." || name == ".." { return Err("INVALID_MESSAGE".into()); }
    let dir = match cwd { Some(d) => format!("{}/{sub}", sq(d)), None => format!("\"$HOME\"/{fallback}") };
    let cmd = format!("d={dir} && mkdir -p \"$d\" && cd \"$d\" && cat > {} && printf '%s/%s' \"$PWD\" {}", sq(&name), sq(&name));
    if let Some(p) = exec(&cmd, data.clone()).filter(|p| p.starts_with('/')) { return Ok(p); }
    // Servidor Windows (OpenSSH com PowerShell como shell): o comando sh falha; grava pelo PowerShell.
    exec(&ps_put(cwd, sub, fallback, &name), data)
        .map(|p| p.trim().to_owned()).filter(|p| p.as_bytes().get(1) == Some(&b':')).ok_or_else(|| "SCP_FAILED".into())
}

/// Texto entre aspas simples do PowerShell (`'` vira `''`).
fn pq(s: &str) -> String { format!("'{}'", s.replace('\'', "''")) }

/// Versão PowerShell de [`put`]: stdin binário em `<cwd>/<sub>` ou `$HOME/<fallback>`; escreve o caminho absoluto.
fn ps_put(cwd: Option<&str>, sub: &str, fallback: &str, name: &str) -> String {
    let dir = match cwd { Some(d) => format!("Join-Path {} {}", pq(d), pq(sub)), None => format!("Join-Path $HOME {}", pq(fallback)) };
    format!("$d=({dir}); New-Item -ItemType Directory -Force -Path $d | Out-Null; $p=Join-Path $d {}; $f=[IO.File]::Create($p); [Console]::OpenStandardInput().CopyTo($f); $f.Close(); [Console]::Out.Write($p)", pq(name))
}

/// Branch e pendências da pasta remota (`{branch, dirty}`); `null` fora de repositório ou sem acesso.
#[tauri::command(async)]
pub fn ssh_git(app: AppHandle, target: Value, cwd: String) -> Result<Value, String> {
    let s = server_for(&app, &target)?;
    let Some(out) = exec(&s, &format!("git -C {} status --porcelain -b", sq(&cwd)), Duration::from_secs(3)) else { return Ok(Value::Null) };
    Ok(parse_git(&out))
}

/// `## main...origin/main` + uma linha por arquivo pendente.
pub fn parse_git(out: &str) -> Value {
    let mut l = out.lines();
    let head = l.next().and_then(|h| h.strip_prefix("## ")).unwrap_or_default();
    let branch = head.split("...").next().unwrap_or_default();
    let branch = if branch.starts_with("HEAD (") { "" } else { branch.strip_prefix("No commits yet on ").unwrap_or(branch) };
    json!({ "branch": branch, "dirty": l.filter(|x| !x.is_empty()).count() })
}

/// CPU (%) e memória do servidor Linux, no formato de `sys_stats`; outro SO devolve `null`.
/// CPU = diferença de `/proc/stat` em 0,5 s.
#[tauri::command(async)]
pub fn ssh_stats(app: AppHandle, target: Value) -> Result<Value, String> {
    let s = server_for(&app, &target)?;
    let cmd = "head -1 /proc/stat; sleep 0.5; head -1 /proc/stat; nproc; grep -E '^(MemTotal|MemAvailable):' /proc/meminfo; hostname";
    Ok(exec(&s, cmd, Duration::from_secs(4)).map_or(Value::Null, |o| parse_stats(&o)))
}

pub fn parse_stats(out: &str) -> Value {
    let l: Vec<&str> = out.lines().collect();
    let cpu = |x: &str| -> Option<(u64, u64)> {
        let n: Vec<u64> = x.strip_prefix("cpu ")?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        Some((n.iter().sum(), n.get(3)? + n.get(4).unwrap_or(&0)))
    };
    let (Some((t1, i1)), Some((t2, i2))) = (l.first().and_then(|x| cpu(x)), l.get(1).and_then(|x| cpu(x))) else { return Value::Null };
    let kb = |k: &str| l.iter().find_map(|x| x.strip_prefix(k)?.split_whitespace().next()?.parse::<u64>().ok()).map(|v| v * 1024);
    let (Some(total), Some(avail)) = (kb("MemTotal:"), kb("MemAvailable:")) else { return Value::Null };
    let dt = (t2 - t1).max(1);
    json!({ "cpu": 100.0 * (dt - (i2 - i1).min(dt)) as f64 / dt as f64, "cores": l.get(2).and_then(|x| x.trim().parse::<u64>().ok()),
        "mem_total": total, "mem_used": total - avail.min(total), "host": l.get(5).map(|h| h.trim()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_cai_no_powershell_quando_sh_falha() {
        let calls = std::cell::RefCell::new(Vec::new());
        // Servidor Windows falso: sh falha, PowerShell grava e devolve caminho com unidade.
        let r = put_via(|cmd, d| { calls.borrow_mut().push((cmd.to_owned(), d)); cmd.starts_with("$d=(").then(|| "C:\\u\\screenshots\\a.png\r\n".into()) },
            Some(r"C:\u"), "screenshots", "Pictures", "a.png", b"img".to_vec());
        assert_eq!(r.as_deref(), Ok(r"C:\u\screenshots\a.png"));
        let c = calls.borrow();
        assert_eq!(c.len(), 2);
        assert!(c[0].0.starts_with("d=") && c[1].0.starts_with(r"$d=(Join-Path 'C:\u' 'screenshots');"));
        assert!(c.iter().all(|(_, d)| d == b"img"));
        // Nenhum dos dois devolve caminho válido: SCP_FAILED.
        assert_eq!(put_via(|_, _| Some("lixo".into()), None, "s", "f", "a.png", vec![]), Err("SCP_FAILED".into()));
    }

    #[test]
    fn ps_put_cita_pasta_e_nome() {
        let c = ps_put(Some("C:\\o'k"), "screenshots", "Pictures/Screenshots", "a'b.png");
        assert!(c.starts_with("$d=(Join-Path 'C:\\o''k' 'screenshots');"));
        assert!(c.contains("Join-Path $d 'a''b.png';"));
        assert!(ps_put(None, "s", "Downloads/archives", "x.png").starts_with("$d=(Join-Path $HOME 'Downloads/archives');"));
    }

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

    #[test]
    fn remoto_git_cpu_aspas() {
        assert_eq!(sq("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(parse_git("## main...origin/main [ahead 1]
 M a
?? b
"), json!({"branch":"main","dirty":2}));
        assert_eq!(parse_git("## HEAD (no branch)
"), json!({"branch":"","dirty":0}));
        assert_eq!(parse_git("## No commits yet on dev
"), json!({"branch":"dev","dirty":0}));
        let v = parse_stats("cpu  100 0 100 800 0 0 0 0 0 0
cpu  150 0 150 900 0 0 0 0 0 0
4
MemTotal: 1000 kB
MemAvailable: 250 kB
srv
");
        assert_eq!((v["cpu"].as_f64().unwrap().round(), v["cores"].as_u64(), v["mem_used"].as_u64(), v["host"].as_str()), (50.0, Some(4), Some(768000), Some("srv")));
        assert_eq!(parse_stats("Darwin"), Value::Null);
    }
}

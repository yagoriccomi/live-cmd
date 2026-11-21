//! Configurações do app (JSON no diretório de dados) e validação do servidor. Lidas pelo app e pelo daemon.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Worker publicado (`wrangler deploy --env public`).
pub const DEFAULT_SERVER: &str = "wss://live-cmd-signaling.yago-riccomi-s.workers.dev";
/// Padrão anterior (`wrangler dev`): instalações que só o herdaram migram para o novo.
const OLD_DEFAULT: &str = "ws://127.0.0.1:8787";
const FILE: &str = "app.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings {
    pub server: String,
    /// Linhas de histórico por aba; `None` = 1000. Use [`Settings::scrollback`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<usize>,
}

impl Settings {
    pub fn scrollback(&self) -> usize { self.scrollback.unwrap_or(1000).clamp(100, 100_000) }
}

/// Mesmo diretório do host: `LIVE_CMD_DATA_DIR`, senão `%APPDATA%/live-cmd`.
pub fn data_dir() -> Result<PathBuf, String> {
    std::env::var_os("LIVE_CMD_DATA_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("live-cmd")))
        .ok_or_else(|| "DATA_DIR".into())
}

/// Lê `app.json`; sem arquivo (ou ilegível), cria um com o servidor padrão. A sala é a do host (`room.txt`).
pub fn load(dir: &Path) -> Result<Settings, String> {
    if let Some(s) = std::fs::read(dir.join(FILE)).ok().and_then(|t| serde_json::from_slice::<Settings>(&t).ok())
        && valid_server(&s.server) && s.server != OLD_DEFAULT { return Ok(s); }
    let s = Settings { server: DEFAULT_SERVER.into(), scrollback: None };
    save(dir, &s)?;
    Ok(s)
}

/// Grava em arquivo temporário e renomeia, para não deixar JSON pela metade.
pub fn save(dir: &Path, s: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("app.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap()).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|_| "IO_ERROR".into())
}

/// `wss://` com host, ou `ws://` só para `localhost`/`127.0.0.1` (design §5).
pub fn valid_server(url: &str) -> bool {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) { return false; }
    let (secure, rest) = match url.split_once("://") { Some(("wss", r)) => (true, r), Some(("ws", r)) => (false, r), _ => return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') { return false; }
    let (host, port) = authority.rsplit_once(':').unwrap_or((authority, ""));
    if !port.is_empty() && port.parse::<u16>().is_err() { return false; }
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') { return false; }
    secure || matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1")
}

/// Perfis em `profiles.json` (somente leitura). `Ok(None)` = sem arquivo (perfis padrão);
/// `Err(linha)` = JSON inválido.
pub fn profiles(dir: &Path) -> Result<Option<Vec<(String, String)>>, usize> {
    let Ok(t) = std::fs::read_to_string(dir.join("profiles.json")) else { return Ok(None) };
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.line())?;
    let list = v.as_array().ok_or(1usize)?;
    list.iter().map(|p| match (p["label"].as_str(), p["command"].as_str()) {
        (Some(l), Some(c)) => Ok((l.to_string(), c.to_string())),
        _ => Err(1),
    }).collect::<Result<_, _>>().map(Some)
}

/// Perfil editável. `profile_id`/`osc` vazios: gerado a partir do rótulo / sem integração OSC.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    #[serde(default)] pub profile_id: String,
    pub label: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub osc: bool,
}

/// Lista completa (com `profile_id` e `osc`) para o editor; mesmas regras de [`profiles`].
pub fn profiles_full(dir: &Path) -> Result<Option<Vec<Profile>>, usize> {
    let Ok(t) = std::fs::read_to_string(dir.join("profiles.json")) else { return Ok(None) };
    serde_json::from_str(&t).map(Some).map_err(|e| e.line())
}

/// Valida (rótulo 1..=40, comando não vazio, sem caracteres de controle), gera ids únicos e grava `profiles.json`.
pub fn save_profiles(dir: &Path, list: Vec<Profile>) -> Result<Vec<Profile>, String> {
    if list.is_empty() || list.len() > 50 { return Err("INVALID_PROFILE".into()); }
    let mut out: Vec<Profile> = Vec::new();
    for mut p in list {
        p.label = p.label.trim().into();
        p.command = p.command.trim().into();
        let n = p.label.chars().count();
        if n == 0 || n > 40 || p.command.is_empty() || p.command.len() > 4096 || format!("{}{}{}", p.label, p.command, p.profile_id).chars().any(char::is_control) {
            return Err("INVALID_PROFILE".into());
        }
        let base: String = if p.profile_id.is_empty() {
            p.label.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
        } else { p.profile_id.clone() };
        let base = if base.trim_matches('-').is_empty() { "perfil".to_string() } else { base };
        p.profile_id = (1..).map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") }).find(|id| out.iter().all(|o| &o.profile_id != id)).unwrap();
        out.push(p);
    }
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("profiles.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&out).unwrap()).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join("profiles.json")).map_err(|_| "IO_ERROR")?;
    Ok(out)
}

/// Shells comuns presentes no PATH (ou Git Bash no local padrão), como sugestões `(rótulo, comando)`.
pub fn detect_shells() -> Vec<(String, String)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let find = |exe: &str| std::env::split_paths(&path).map(|d| d.join(exe)).find(|p| p.is_file());
    let mut v: Vec<(String, String)> = [("PowerShell 7", "pwsh.exe"), ("Windows PowerShell", "powershell.exe"), ("Prompt de Comando", "cmd.exe"), ("WSL", "wsl.exe")]
        .into_iter().filter(|(_, e)| find(e).is_some()).map(|(l, e)| (l.into(), e.into())).collect();
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| r"C:\Program Files".into());
    let git = Some(pf.join(r"Git\bin\bash.exe")).filter(|p| p.is_file())
        .or_else(|| find("bash.exe").filter(|p| !p.to_string_lossy().to_lowercase().contains("system32")));
    if let Some(b) = git { v.push(("Git Bash".into(), format!("\"{}\" --login -i", b.display()))); }
    v
}

/// Aparência (tema, fonte, vidro, brilho, animações) em `appearance.json`, separada de `app.json`:
/// o celular também grava aqui (`settings_set`) sem disputar o estado do servidor e da rolagem.
const APPEARANCE_FILE: &str = "appearance.json";
pub use crate::extras::Appearance;

/// Sem arquivo, ilegível ou fora das faixas: o padrão.
pub fn appearance(dir: &Path) -> Appearance {
    std::fs::read(dir.join(APPEARANCE_FILE)).ok().and_then(|t| serde_json::from_slice::<Appearance>(&t).ok())
        .map(|mut a| { a.theme = crate::extras::legacy_theme(&a.theme).to_owned(); a })
        .filter(Appearance::valid).unwrap_or_default()
}

pub fn save_appearance(dir: &Path, a: &Appearance) -> Result<(), String> {
    if !a.valid() { return Err("INVALID_MESSAGE".into()); }
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("appearance.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(a).unwrap()).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(APPEARANCE_FILE)).map_err(|_| "IO_ERROR".into())
}

/// Servidor da tela "Servidores SSH" em `ssh.json`. Nunca guarda senha: só o caminho da chave.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct SshServer {
    #[serde(default)] pub id: String,
    pub name: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub key: Option<String>,
    /// "Manter vivo" em segundos (0 = desligado) e "Reconectar ao cair".
    #[serde(default, skip_serializing_if = "is_zero")] pub keepalive_s: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub auto_reconnect: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub tmux: bool,
}
fn is_zero(n: &u16) -> bool { *n == 0 }

const SSH_FILE: &str = "ssh.json";

impl SshServer {
    pub fn target(&self) -> crate::ssh::Ssh {
        crate::ssh::Ssh { user: self.user.clone(), host: self.host.clone(), port: self.port, key: self.key.clone(), keepalive_s: self.keepalive_s, auto_reconnect: self.auto_reconnect, tmux: self.tmux }
    }
}

/// Lista salva; sem arquivo = vazia.
pub fn ssh_servers(dir: &Path) -> Result<Vec<SshServer>, String> {
    match std::fs::read(dir.join(SSH_FILE)) {
        Ok(b) => serde_json::from_slice(&b).map_err(|_| "IO_ERROR".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err("IO_ERROR".into()),
    }
}

fn store_ssh(dir: &Path, list: &[SshServer]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("ssh.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).unwrap()).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(SSH_FILE)).map_err(|_| "IO_ERROR".into())
}

/// Valida (nome 1..=40, destino pelas regras do host), apara espaços e grava; `id` vazio = novo.
pub fn save_ssh_server(dir: &Path, mut s: SshServer) -> Result<SshServer, String> {
    s.name = s.name.trim().into();
    s.user = s.user.trim().into();
    s.host = s.host.trim().into();
    if s.port == 0 { s.port = 22; }
    s.key = s.key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty());
    let n = s.name.chars().count();
    if n == 0 || n > 40 || s.name.chars().any(char::is_control) { return Err("INVALID_SSH_NAME".into()); }
    s.target().check()?;
    let mut list = ssh_servers(dir)?;
    if s.id.is_empty() {
        if list.len() >= 200 { return Err("LIMIT_EXCEEDED".into()); }
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        s.id = (0..).map(|i| format!("{:x}", t + i)).find(|id| !list.iter().any(|x| &x.id == id)).unwrap();
        list.push(s.clone());
    } else {
        *list.iter_mut().find(|x| x.id == s.id).ok_or("NOT_FOUND")? = s.clone();
    }
    store_ssh(dir, &list)?;
    Ok(s)
}

pub fn remove_ssh_server(dir: &Path, id: &str) -> Result<(), String> {
    let mut list = ssh_servers(dir)?;
    list.retain(|x| x.id != id);
    store_ssh(dir, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("live-cmd-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn servidor_validado() {
        for ok in ["wss://sinal.exemplo.workers.dev", "wss://a.b:443/v1", "ws://localhost:8787", "ws://127.0.0.1:8787", "ws://LocalHost/"] {
            assert!(valid_server(ok), "{ok}");
        }
        for bad in ["ws://sinal.exemplo.dev", "http://localhost", "wss://", "wss://a b", "wss://u@h", "wss://h:99999", "ws://127.0.0.2", "sinal.dev", "wss://h\n"] {
            assert!(!valid_server(bad), "{bad}");
        }
    }

    #[test]
    fn settings_criadas_e_relidas() {
        let d = tmp("settings");
        let s = load(&d).unwrap();
        assert_eq!(s.server, DEFAULT_SERVER);
        assert_eq!(load(&d).unwrap(), s, "relido na segunda execução");
        let s2 = Settings { server: "wss://x.dev".into(), scrollback: Some(5) };
        assert_eq!((s.scrollback(), s2.scrollback(), Settings { scrollback: Some(1 << 30), ..s2.clone() }.scrollback()), (1000, 100, 100_000));
        save(&d, &s2).unwrap();
        assert_eq!(load(&d).unwrap(), s2);
        std::fs::write(d.join(FILE), "{").unwrap();
        assert_eq!(load(&d).unwrap().server, DEFAULT_SERVER, "arquivo corrompido volta ao padrão");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn perfis() {
        let d = tmp("profiles");
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(profiles(&d), Ok(None));
        std::fs::write(d.join("profiles.json"), r#"[{"profile_id":"p","label":"PowerShell","command":"pwsh.exe"}]"#).unwrap();
        assert_eq!(profiles(&d), Ok(Some(vec![("PowerShell".into(), "pwsh.exe".into())])));
        std::fs::write(d.join("profiles.json"), "[\n\n{").unwrap();
        assert_eq!(profiles(&d), Err(3));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn editor_de_perfis() {
        let d = tmp("profedit");
        let p = |id: &str, l: &str, c: &str| Profile { profile_id: id.into(), label: l.into(), command: c.into(), osc: false };
        let keep = Profile { osc: true, ..p("pwsh", "PS", "pwsh.exe") };
        let out = save_profiles(&d, vec![keep, p("", " Meu Bash ", "bash"), p("", "Meu Bash", "bash -l")]).unwrap();
        assert_eq!(out.iter().map(|p| p.profile_id.as_str()).collect::<Vec<_>>(), ["pwsh", "meu-bash", "meu-bash-2"]);
        assert!(out[0].osc, "osc preservado");
        assert_eq!(profiles_full(&d).unwrap().unwrap(), out);
        assert_eq!(profiles(&d).unwrap().unwrap()[1], ("Meu Bash".into(), "bash".into()), "formato lido pelo host");
        for bad in [p("", "", "x"), p("", &"a".repeat(41), "x"), p("", "a", " "), p("", "a\u{7}", "x"), p("", "a", "x\ny")] {
            assert!(save_profiles(&d, vec![bad.clone()]).is_err(), "{bad:?}");
        }
        assert!(save_profiles(&d, vec![]).is_err());
        assert!(save_profiles(&d, vec![p("", &"é".repeat(40), "x")]).is_ok());
        assert!(detect_shells().iter().any(|s| s.1 == "cmd.exe"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn servidores_ssh() {
        let d = tmp("ssh");
        let srv = |id: &str, name: &str, user: &str, host: &str, port: u16, key: Option<&str>| SshServer {
            id: id.into(), name: name.into(), user: user.into(), host: host.into(), port, key: key.map(Into::into), ..Default::default() };
        assert_eq!(ssh_servers(&d).unwrap(), vec![]);
        let a = save_ssh_server(&d, srv("", " Produção ", " deploy ", "srv.exemplo.com", 22, Some(" "))).unwrap();
        assert!(!a.id.is_empty());
        assert_eq!((a.name.as_str(), a.user.as_str(), a.key.clone()), ("Produção", "deploy", None), "aparado; chave vazia vira None");
        let b = save_ssh_server(&d, srv("", "NAS", "root", "10.0.0.5", 2222, Some(r"C:\k\id"))).unwrap();
        assert_ne!(a.id, b.id);
        save_ssh_server(&d, SshServer { port: 2200, ..a.clone() }).unwrap();
        let l = ssh_servers(&d).unwrap();
        assert_eq!((l.len(), l[0].port), (2, 2200), "editar troca no lugar");
        let p = save_ssh_server(&d, srv("", "sem-user", "", "h", 0, None)).unwrap();
        assert_eq!((p.port, p.user.as_str()), (22, ""), "porta vazia = 22, usuário opcional");
        remove_ssh_server(&d, &p.id).unwrap();
        let txt = std::fs::read_to_string(d.join("ssh.json")).unwrap();
        assert!(!txt.contains("password") && !txt.contains("senha"));
        for (bad, code) in [(srv("", "", "u", "h", 22, None), "INVALID_SSH_NAME"), (srv("", "x", "u;rm", "h", 22, None), "INVALID_SSH_USER"),
            (srv("", "x", "u", "h h", 22, None), "INVALID_SSH_HOST"),             (srv("", "x", "u", "h", 22, Some("id_rsa")), "INVALID_SSH_KEY"), (srv("nao-existe", "x", "u", "h", 22, None), "NOT_FOUND")] {
            assert_eq!(save_ssh_server(&d, bad).unwrap_err(), code);
        }
        assert!(serde_json::from_str::<SshServer>(r#"{"name":"x","user":"u","host":"h","port":22,"password":"p"}"#).map(|s| serde_json::to_string(&s).unwrap()).unwrap().find("password").is_none(), "senha nunca é regravada");
        remove_ssh_server(&d, &a.id).unwrap();
        assert_eq!(ssh_servers(&d).unwrap(), vec![b]);
        std::fs::remove_dir_all(&d).unwrap();
    }

}

//! Configurações do app (JSON no diretório de dados), validação do servidor e início com o Windows.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Worker publicado (`wrangler deploy --env public`).
pub const DEFAULT_SERVER: &str = "wss://live-cmd-signaling.yago-riccomi-s.workers.dev";
/// Padrão anterior (`wrangler dev`): instalações que só o herdaram migram para o novo.
const OLD_DEFAULT: &str = "ws://127.0.0.1:8787";
const FILE: &str = "app.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settings { pub server: String }

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
    let s = Settings { server: DEFAULT_SERVER.into() };
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

/// "Iniciar com o Windows": valor `live-cmd` em `HKCU\<key>` (o app usa [`RUN_KEY`]).
pub mod autostart {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
    use windows::core::{HSTRING, w};

    pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// Linha de comando gravada: o executável atual entre aspas.
    pub fn command() -> Result<String, String> {
        std::env::current_exe().map(|p| format!("\"{}\"", p.display())).map_err(|_| "EXE".into())
    }

    pub fn is_enabled(key: &str) -> bool {
        unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), w!("live-cmd"), RRF_RT_REG_SZ, None, None, None) == ERROR_SUCCESS }
    }

    pub fn set(key: &str, on: bool, command: &str) -> Result<(), String> {
        let key = HSTRING::from(key);
        let r = unsafe {
            if on {
                let data: Vec<u16> = command.encode_utf16().chain([0]).collect();
                RegSetKeyValueW(HKEY_CURRENT_USER, &key, w!("live-cmd"), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32)
            } else {
                match RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, w!("live-cmd")) { ERROR_FILE_NOT_FOUND => ERROR_SUCCESS, e => e }
            }
        };
        if r == ERROR_SUCCESS { Ok(()) } else { Err("AUTOSTART".into()) }
    }
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
        let s2 = Settings { server: "wss://x.dev".into() };
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

    /// Usa uma chave descartável em HKCU, nunca a `Run` real.
    #[test]
    fn autostart_liga_desliga() {
        let key = format!(r"Software\live-cmd-test-{}", std::process::id());
        assert!(!autostart::is_enabled(&key));
        autostart::set(&key, true, r#""C:\x\live-cmd.exe""#).unwrap();
        assert!(autostart::is_enabled(&key));
        autostart::set(&key, false, "").unwrap();
        assert!(!autostart::is_enabled(&key));
        autostart::set(&key, false, "").unwrap();
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &windows::core::HSTRING::from(key.as_str())) };
    }
}

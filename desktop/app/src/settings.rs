//! Configurações do app: arquivos em `live_cmd_echo::appdata::settings` (o daemon lê os mesmos);
//! aqui fica só o início com o Windows.
pub use live_cmd_echo::appdata::settings::*;

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

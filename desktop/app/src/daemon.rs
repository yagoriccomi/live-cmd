//! Acha ou sobe o daemon de terminais `live-cmd-termd` (dono das sessões; segue vivo quando o app fecha).
//!
//! O exe que vem no instalador fica ao lado do app; o que roda é uma cópia em
//! `%LOCALAPPDATA%\live-cmd\daemon\<versão>\`, para a atualização do app poder trocar os arquivos
//! do instalador com o daemon no ar. Daemon de outro `ipc_version` é encerrado e trocado.
use std::path::{Path, PathBuf};
use std::time::Duration;

use live_cmd_echo::ipc::{self, Endpoint, Fail, Hello, ServiceClient};

const EXE: &str = "live-cmd-termd.exe";

/// Garante um daemon compatível escutando e devolve o `hello` dele.
pub fn ensure(ep: &Endpoint) -> Result<Hello, String> {
    match ipc::probe(ep) {
        Ok(h) => return Ok(h),
        Err(Fail::Version(v)) => {
            live_cmd_echo::hlog(&format!("app: daemon com ipc {v}, trocando pelo {}", ipc::IPC_VERSION));
            let _ = ServiceClient::new(ep.clone()).shutdown();
            if !wait(|| matches!(ipc::probe(ep), Err(Fail::Absent)), 50) { return Err("DAEMON_BUSY".into()); }
        }
        Err(Fail::Denied) => return Err("DAEMON_DENIED".into()),
        Err(_) => {}
    }
    let exe = install()?;
    launch(&exe)?;
    let mut hello = None;
    wait(|| { hello = ipc::probe(ep).ok(); hello.is_some() }, 100);
    let h = hello.ok_or("DAEMON_TIMEOUT")?;
    cleanup(&exe);
    Ok(h)
}

/// `true` se `f` virou verdade em até `n` × 100 ms.
fn wait(mut f: impl FnMut() -> bool, n: u32) -> bool {
    (0..n).any(|_| { if f() { return true; } std::thread::sleep(Duration::from_millis(100)); false })
}

fn root() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("IO_ERROR")?;
    Ok(base.join("live-cmd").join("daemon"))
}

/// Exe a executar. Release: copia o do instalador para a pasta da versão, se ainda não estiver lá.
/// Debug: usa o que está ao lado do app (`cargo build --bin live-cmd-termd` no crate do host) ou `LIVE_CMD_TERMD`.
fn install() -> Result<PathBuf, String> {
    let seed = match std::env::var_os("LIVE_CMD_TERMD") {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe().map_err(|_| "IO_ERROR")?.with_file_name(EXE),
    };
    if !seed.is_file() { return Err("DAEMON_MISSING".into()); }
    if cfg!(debug_assertions) { return Ok(seed); }
    let dir = root()?.join(env!("CARGO_PKG_VERSION"));
    let dst = dir.join(EXE);
    if !dst.is_file() {
        std::fs::create_dir_all(&dir).map_err(|_| "IO_ERROR")?;
        let tmp = dir.join(format!("{EXE}.{}.tmp", std::process::id()));
        std::fs::copy(&seed, &tmp).map_err(|_| "IO_ERROR")?;
        // Outro app pode ter copiado ao mesmo tempo: o que chegou primeiro vale.
        if std::fs::rename(&tmp, &dst).is_err() { let _ = std::fs::remove_file(&tmp); }
        if !dst.is_file() { return Err("IO_ERROR".into()); }
    }
    Ok(dst)
}

/// Apaga as pastas de versões antigas. A de um daemon ainda rodando falha (exe em uso) e fica.
fn cleanup(current: &Path) {
    let (Ok(root), Some(keep)) = (root(), current.parent()) else { return };
    if !current.starts_with(&root) { return; }
    let Ok(rd) = std::fs::read_dir(&root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p != keep && p.is_dir() { let _ = std::fs::remove_dir_all(&p); }
    }
}

/// Processo solto do app: sem console, grupo próprio, fora do job do app (se o Windows deixar), sem herdar handles.
fn launch(exe: &Path) -> Result<(), String> {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CreateProcessW, DETACHED_PROCESS, PROCESS_INFORMATION, STARTUPINFOW};
    use windows::core::{HSTRING, PWSTR};
    let app = HSTRING::from(exe.as_os_str());
    let cwd = std::env::var_os("USERPROFILE").map(|h| HSTRING::from(h.as_os_str()));
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    for flags in [base | CREATE_BREAKAWAY_FROM_JOB, base] {
        let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
        let mut pi = PROCESS_INFORMATION::default();
        let mut cmd: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
        let r = unsafe {
            CreateProcessW(&app, Some(PWSTR(cmd.as_mut_ptr())), None, None, false, flags, None,
                cwd.as_ref().map_or(windows::core::PCWSTR::null(), |c| windows::core::PCWSTR(c.as_ptr())), &si, &mut pi)
        };
        match r {
            Ok(()) => {
                unsafe { let _ = CloseHandle(pi.hThread); let _ = CloseHandle(pi.hProcess); }
                return Ok(());
            }
            // Job do app proíbe sair dele: tenta sem o breakaway (o daemon morre junto só se o job for encerrado).
            Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() && flags != base => continue,
            Err(e) => { live_cmd_echo::hlog(&format!("app: daemon não iniciou ({e})")); return Err("DAEMON_START".into()); }
        }
    }
    Err("DAEMON_START".into())
}

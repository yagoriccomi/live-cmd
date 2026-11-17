//! ConPTY real (C3 §5, D3c): pseudoconsole sem janela; leitura sempre ativa em thread própria.
use crate::session::Pty;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::ffi::OsStrExt;
use std::sync::{mpsc, Arc, Mutex};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Console::{SetConsoleCtrlHandler, ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::*;

/// Sem janela visível (answers 017 item 1): o filho anexa ao pseudoconsole, que é headless.
/// CREATE_NO_WINDOW quebra o vínculo (saída vazia) e CREATE_NEW_CONSOLE abriria janela; ambos ficam fora.
pub const FLAGS: PROCESS_CREATION_FLAGS = EXTENDED_STARTUPINFO_PRESENT;

pub enum Ev { Output(Vec<u8>), Exit(i32) }

pub struct ConPty { input: mpsc::Sender<Vec<u8>>, hpc: Arc<Mutex<Option<isize>>>, process: Arc<OwnedHandle> }

fn io(e: windows::core::Error) -> io::Error { io::Error::other(e) }
fn owned(h: HANDLE) -> OwnedHandle { unsafe { OwnedHandle::from_raw_handle(h.0) } }
fn raw(h: &OwnedHandle) -> HANDLE { HANDLE(h.as_raw_handle()) }

impl ConPty {
    /// `on` roda na thread de leitura: toda a saída, na ordem, e por fim `Exit` (após o EOF).
    pub fn spawn(cmd: &str, cwd: Option<&std::path::Path>, cols: u16, rows: u16, mut on: impl FnMut(Ev) + Send + 'static) -> io::Result<Self> {
        unsafe {
            // Se o host herdou "Ctrl+C ignorado" (ex.: criado com CREATE_NEW_PROCESS_GROUP), o filho herdaria e o 0x03 não interromperia nada.
            let _ = SetConsoleCtrlHandler(None, false);
            let (mut in_r, mut in_w, mut out_r, mut out_w) = (HANDLE::default(), HANDLE::default(), HANDLE::default(), HANDLE::default());
            CreatePipe(&mut in_r, &mut in_w, None, 0).map_err(io)?;
            let (in_r, in_w) = (owned(in_r), owned(in_w));
            CreatePipe(&mut out_r, &mut out_w, None, 0).map_err(io)?;
            let (out_r, out_w) = (owned(out_r), owned(out_w));
            let hpc = CreatePseudoConsole(COORD { X: cols as i16, Y: rows as i16 }, raw(&in_r), raw(&out_w), 0).map_err(io)?;
            drop((in_r, out_w));
            let mut size = 0;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
            let mut buf = vec![0u8; size];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(buf.as_mut_ptr().cast());
            let mut si = STARTUPINFOEXW::default();
            si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            // Handles nulos: sem isso o filho herdaria o stdio redirecionado do host em vez do pseudoconsole.
            si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            si.lpAttributeList = list;
            let mut cmd: Vec<u16> = cmd.encode_utf16().chain([0]).collect();
            let dir: Option<Vec<u16>> = cwd.map(|d| d.as_os_str().encode_wide().chain([0]).collect());
            let mut pi = PROCESS_INFORMATION::default();
            let r = InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)
                .and_then(|_| UpdateProcThreadAttribute(list, 0, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize, Some(hpc.0 as *const _), size_of::<HPCON>(), None, None))
                .and_then(|_| CreateProcessW(PCWSTR::null(), Some(PWSTR(cmd.as_mut_ptr())), None, None, false, FLAGS, None, dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())), &si.StartupInfo, &mut pi));
            DeleteProcThreadAttributeList(list);
            if let Err(e) = r { ClosePseudoConsole(hpc); return Err(io(e)); }
            let _ = CloseHandle(pi.hThread);
            let process = Arc::new(owned(pi.hProcess));
            let hpc = Arc::new(Mutex::new(Some(hpc.0)));
            let (tx, rx) = mpsc::channel::<Vec<u8>>();
            // Escrita em thread própria: nunca bloqueia quem chama (o pipe pode encher).
            let mut w = std::fs::File::from(in_w);
            std::thread::spawn(move || { for d in rx { if w.write_all(&d).is_err() { break; } } });
            let (p, h) = (process.clone(), hpc.clone());
            // Fim do processo: fecha o pseudoconsole para a leitura chegar ao EOF.
            std::thread::spawn(move || {
                WaitForSingleObject(raw(&p), INFINITE);
                // Solta o lock antes: ClosePseudoConsole pode esperar a leitura drenar.
                let x = h.lock().unwrap().take();
                if let Some(x) = x { ClosePseudoConsole(HPCON(x)); }
            });
            let p = process.clone();
            let mut r = std::fs::File::from(out_r);
            std::thread::spawn(move || {
                let mut b = vec![0u8; 16384];
                while let Ok(n @ 1..) = r.read(&mut b) { on(Ev::Output(b[..n].to_vec())); }
                WaitForSingleObject(raw(&p), INFINITE);
                let mut code = 0u32;
                let _ = GetExitCodeProcess(raw(&p), &mut code);
                on(Ev::Exit(code as i32));
            });
            Ok(ConPty { input: tx, hpc, process })
        }
    }
    fn running(&self) -> bool { unsafe { WaitForSingleObject(raw(&self.process), 0) != WAIT_OBJECT_0 } }
}

/// Setas/Home/End viram win32-input-mode (o ConPTY sempre liga `?9001h`): programas que leem teclas cruas
/// (ssh.exe) recebem VK de verdade. Em VT puro chegam ESC, `[`, `A` soltos e o lado remoto vê `[A`.
fn win32_key(d: &[u8]) -> Option<Vec<u8>> {
    let k = match d { [0x1b, b'[' | b'O', k] => *k, _ => return None };
    let (vk, sc) = match k { b'A' => (38, 72), b'B' => (40, 80), b'C' => (39, 77), b'D' => (37, 75), b'H' => (36, 71), b'F' => (35, 79), _ => return None };
    // Cs 256 = ENHANCED_KEY (setas fora do teclado numérico).
    Some(format!("[{vk};{sc};0;1;256;1_[{vk};{sc};0;0;256;1_").into_bytes())
}
#[test]
fn win32_key_setas() {
    assert_eq!(win32_key(b"[A").unwrap(), b"[38;72;0;1;256;1_[38;72;0;0;256;1_");
    assert_eq!(win32_key(b"OB").unwrap(), b"[40;80;0;1;256;1_[40;80;0;0;256;1_");
    assert!(win32_key(b"[1;5A").is_none() && win32_key(b"a").is_none());
}

impl Pty for ConPty {
    fn write(&mut self, data: &[u8]) -> io::Result<()> { self.input.send(win32_key(data).unwrap_or_else(|| data.to_vec())).map_err(|_| io::ErrorKind::BrokenPipe.into()) }
    fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        match *self.hpc.lock().unwrap() {
            Some(x) => unsafe { ResizePseudoConsole(HPCON(x), COORD { X: cols as i16, Y: rows as i16 }).map_err(io) },
            None => Ok(()),
        }
    }
    fn kill(&mut self) -> io::Result<()> { if self.running() { unsafe { TerminateProcess(raw(&self.process), 1).map_err(io)?; } } Ok(()) }
}

impl Drop for ConPty { fn drop(&mut self) { let _ = self.kill(); } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Lê até `needle` e devolve o texto consumido; `Err(code)` se o processo saiu antes.
    fn wait(rx: &mpsc::Receiver<Ev>, acc: &mut String, needle: &str) -> Result<String, i32> {
        let end = Instant::now() + Duration::from_secs(15);
        while !acc.contains(needle) {
            match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(Ev::Output(d)) => acc.push_str(&String::from_utf8_lossy(&d)),
                Ok(Ev::Exit(c)) => return Err(c),
                Err(_) => panic!("sem {needle:?} na saída: {acc:?}"),
            }
        }
        Ok(acc.drain(..acc.find(needle).unwrap() + needle.len()).collect())
    }

    #[test]
    fn hidden_conpty_io_resize_ctrl_c_exit() {
        assert_ne!(FLAGS.0 & EXTENDED_STARTUPINFO_PRESENT.0, 0);
        assert_eq!(FLAGS.0 & (CREATE_NEW_CONSOLE.0 | DETACHED_PROCESS.0 | CREATE_NO_WINDOW.0), 0, "janela própria ou desvinculado do pseudoconsole");
        let (tx, rx) = mpsc::channel();
        let mut p = ConPty::spawn("cmd.exe", None, 80, 25, move |e| { let _ = tx.send(e); }).unwrap();
        let mut acc = String::new();
        p.write(b"echo live-cmd-ok\r").unwrap();
        wait(&rx, &mut acc, "live-cmd-ok\r\n").unwrap();
        p.resize(100, 30).unwrap();
        p.write(b"mode con & echo mode-done\r").unwrap();
        // 1º: eco da linha digitada; 2º: depois da saída do `mode`.
        wait(&rx, &mut acc, "mode-done").unwrap();
        let m = wait(&rx, &mut acc, "mode-done").unwrap();
        assert!(m.contains("100") && m.contains("30"), "{m:?}");
        p.write(b"ping -t 127.0.0.1\r").unwrap();
        wait(&rx, &mut acc, "TTL=").unwrap();
        p.write(b"\x03").unwrap();
        // Só aparece se o cmd voltou a ler a linha, ou seja, o ping foi interrompido.
        p.write(b"echo after-ctrl-c\r").unwrap();
        wait(&rx, &mut acc, "after-ctrl-c\r\n").unwrap();
        p.write(b"exit 7\r").unwrap();
        assert_eq!(wait(&rx, &mut acc, "\u{0}never"), Err(7));
    }

    #[test]
    fn cwd_inicial() {
        let (tx, rx) = mpsc::channel();
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let _p = ConPty::spawn("cmd.exe /c cd", Some(&dir), 80, 25, move |e| { let _ = tx.send(e); }).unwrap();
        let mut acc = String::new();
        assert!(wait(&rx, &mut acc, "\u{0}never").is_err());
        let want = dir.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
        assert!(acc.to_lowercase().contains(&want), "{acc:?}");
    }
}


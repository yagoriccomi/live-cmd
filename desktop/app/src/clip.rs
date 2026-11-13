//! Área de transferência: "Copiar link para outro PC" (design §6/§8 item 1a, P11) e o copiar/colar do terminal.
//!
//! DESVIO DE C2 §1, PENDENTE DE APROVAÇÃO: o URI de pareamento passa pela área de transferência.
//! Mitigação: formato `ExcludeClipboardContentFromMonitorProcessing` (fora do histórico Win+V e da
//! nuvem) e limpeza quando o QR expira, é usado ou a janela fecha.
//! Para remover: apagar este arquivo, `mod clip`, o comando `copy_link`, `clip::clear_if_ours` e o
//! botão `#copy` em `ui/`.
use std::sync::Mutex;

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::core::w;

const CF_UNICODETEXT: u32 = 13;

/// Número de sequência do clipboard logo após a nossa cópia; `None` se não há link nosso lá.
static OURS: Mutex<Option<u32>> = Mutex::new(None);

fn exclude_format() -> u32 { unsafe { RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")) } }

/// Texto em UTF-16 terminado em NUL, como `CF_UNICODETEXT` exige.
fn utf16z(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

unsafe fn global(bytes: &[u8]) -> windows::core::Result<HANDLE> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
        let p = GlobalLock(h) as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(h);
        Ok(HANDLE(h.0))
    }
}

/// Copia `text` marcado para não entrar no histórico nem em monitores de clipboard.
pub fn copy_secret(owner: HWND, text: &str) -> Result<(), String> {
    let text: Vec<u8> = utf16z(text).iter().flat_map(|c| c.to_le_bytes()).collect();
    unsafe {
        OpenClipboard(Some(owner)).map_err(|_| "CLIPBOARD")?;
        let r = (|| {
            EmptyClipboard()?;
            SetClipboardData(exclude_format(), Some(global(&0u32.to_le_bytes())?))?;
            SetClipboardData(CF_UNICODETEXT, Some(global(&text)?))?;
            windows::core::Result::Ok(())
        })();
        let _ = CloseClipboard();
        r.map_err(|_| "CLIPBOARD")?;
        *OURS.lock().unwrap() = Some(GetClipboardSequenceNumber());
    }
    Ok(())
}

/// Esvazia o clipboard se ele ainda tem o nosso link (ninguém copiou outra coisa depois).
pub fn clear_if_ours() {
    let mut ours = OURS.lock().unwrap();
    let Some(seq) = ours.take() else { return };
    unsafe {
        if GetClipboardSequenceNumber() == seq && OpenClipboard(None).is_ok() {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
}

/// Texto do clipboard já aberto; `None` se não há texto.
fn read_open() -> Option<String> {
    unsafe {
        let g = HGLOBAL(GetClipboardData(CF_UNICODETEXT).ok()?.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() { return None; }
        let max = GlobalSize(g) / 2;
        let len = (0..max).find(|&i| *p.add(i) == 0).unwrap_or(max);
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(g);
        Some(s)
    }
}

/// `Ctrl+Shift+V` do terminal.
pub fn read_text() -> Result<String, String> {
    unsafe {
        OpenClipboard(None).map_err(|_| "CLIPBOARD")?;
        let s = read_open();
        let _ = CloseClipboard();
        Ok(s.unwrap_or_default())
    }
}

/// `Ctrl+Shift+C` do terminal: texto comum, entra no histórico normalmente.
pub fn write_text(text: &str) -> Result<(), String> {
    let text: Vec<u8> = utf16z(text).iter().flat_map(|c| c.to_le_bytes()).collect();
    unsafe {
        OpenClipboard(None).map_err(|_| "CLIPBOARD")?;
        let r = (|| { EmptyClipboard()?; SetClipboardData(CF_UNICODETEXT, Some(global(&text)?))?; windows::core::Result::Ok(()) })();
        let _ = CloseClipboard();
        r.map_err(|_| "CLIPBOARD".into())
    }
}

/// "Adicionar PC": o link colado sai do clipboard se ainda estiver lá (design §6).
pub fn clear_if_text(text: &str) {
    unsafe {
        if OpenClipboard(None).is_err() { return; }
        if read_open().is_some_and(|s| s.trim() == text) { let _ = EmptyClipboard(); }
        let _ = CloseClipboard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formato_de_exclusao_registrado() {
        let f = exclude_format();
        assert!(f >= 0xC000, "formato registrado fica na faixa 0xC000-0xFFFF");
        assert_eq!(f, exclude_format(), "mesmo nome, mesmo id");
    }
    #[test]
    fn texto_utf16_com_nul() {
        assert_eq!(utf16z("aé"), vec![0x61, 0xE9, 0]);
    }
}

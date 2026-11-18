//! Desempenho (CPU/memória) do PC da aba em foco e opacidade da janela.
use serde_json::Value;
use tauri::AppHandle;

/// `conn` vazio ou "local": este PC; senão pede `sys_stats` ao host da conexão remota.
#[tauri::command(async)]
pub fn sys_stats(app: AppHandle, conn: Option<String>) -> Result<Value, String> {
    match conn.as_deref() {
        None | Some("" | "local") => Ok(live_cmd_echo::stats::sys_stats()),
        Some(c) => crate::term::client(&app, c)?.request(serde_json::json!({ "type": "sys_stats" })),
    }
}

/// "Opacidade" da aparência: a janela inteira fica translúcida (WS_EX_LAYERED + alfa). 100 = opaca, sem camada.
pub fn window_opacity(w: &tauri::WebviewWindow, pct: u8) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW, WS_EX_LAYERED};
    let Ok(h) = w.hwnd() else { return };
    let pct = pct.clamp(60, 100);
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        if pct == 100 {
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_LAYERED.0 as isize));
        } else {
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            let _ = SetLayeredWindowAttributes(h, COLORREF(0), (pct as u32 * 255 / 100) as u8, LWA_ALPHA);
        }
    }
}

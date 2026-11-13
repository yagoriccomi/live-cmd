//! live-cmd: app de bandeja do Windows (design `docs/design/desktop-app.md`).
//! Sobe só na bandeja; a janela (Parear | Aparelhos | Configurações) abre pelo menu.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clip;
mod host;
mod settings;

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use windows::Win32::UI::WindowsAndMessaging::{IDOK, MB_DEFBUTTON2, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_OKCANCEL, MB_SETFOREGROUND, MessageBoxW};
use windows::core::HSTRING;

use host::{Service, State as HostState};
use settings::autostart;

struct App {
    svc: Mutex<Service>,
    data_dir: std::path::PathBuf,
    settings: Mutex<settings::Settings>,
    /// URI da OTK ativa: só para o QR e o "Copiar link"; nunca vai para texto, log ou UI.
    link: Mutex<Option<String>>,
}

fn state_name(s: HostState) -> &'static str {
    match s { HostState::Running => "running", HostState::Connecting => "connecting", HostState::Error => "error" }
}

fn devices_text(n: usize) -> String {
    match n { 0 => "Nenhum aparelho conectado".into(), 1 => "1 aparelho conectado".into(), n => format!("{n} aparelhos conectados") }
}

/// QR do URI como SVG em data URL, para um `<img>`. O URI só existe nos módulos do desenho.
fn qr_data_url(uri: &str) -> Result<String, String> {
    let code = qrcode::QrCode::new(uri.as_bytes()).map_err(|_| "QR")?;
    let w = code.width();
    let q = 4;
    let mut d = String::new();
    for (i, c) in code.to_colors().into_iter().enumerate() {
        if c == qrcode::Color::Dark { d += &format!("M{} {}h1v1h-1z", i % w + q, i / w + q); }
    }
    let n = w + 2 * q;
    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n} {n}" shape-rendering="crispEdges"><rect width="{n}" height="{n}" fill="#fff"/><path d="{d}" fill="#000"/></svg>"##);
    Ok(format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg)))
}

#[tauri::command]
fn status(app: State<App>) -> Value {
    let s = app.svc.lock().unwrap().status();
    json!({ "state": state_name(s.state), "connected": s.connected, "pairing_expires_ms": s.pairing_expires_ms, "now_ms": host::now_ms() })
}

/// Nova OTK (invalida a anterior, C2 §1) e o QR dela.
#[tauri::command]
fn pair(app: State<App>) -> Result<Value, String> {
    clip::clear_if_ours();
    let svc = app.svc.lock().unwrap();
    let uri = svc.pair()?;
    let qr = qr_data_url(&uri)?;
    *app.link.lock().unwrap() = Some(uri);
    Ok(json!({ "qr": qr, "expires_ms": svc.status().pairing_expires_ms, "now_ms": host::now_ms() }))
}

fn cancel(app: &App) {
    app.svc.lock().unwrap().cancel_pair();
    *app.link.lock().unwrap() = None;
    clip::clear_if_ours();
}

#[tauri::command]
fn cancel_pair(app: State<App>) { cancel(&app) }

/// Desvio de C2 §1 pendente de aprovação (P11); ver `clip.rs`.
#[tauri::command]
fn copy_link(app: State<App>, window: tauri::WebviewWindow) -> Result<(), String> {
    let link = app.link.lock().unwrap().clone().ok_or("EXPIRED")?;
    clip::copy_secret(window.hwnd().map_err(|_| "CLIPBOARD")?, &link)
}

#[tauri::command]
fn devices(app: State<App>) -> Vec<Value> {
    let mut list = app.svc.lock().unwrap().devices();
    list.sort_by_key(|d| (!d.connected, std::cmp::Reverse(d.paired_at_ms)));
    list.into_iter().map(|d| json!({ "id": URL_SAFE_NO_PAD.encode(d.client_pk), "label": d.label, "paired_at_ms": d.paired_at_ms, "connected": d.connected })).collect()
}

#[tauri::command]
fn revoke(app: State<App>, id: String) -> Result<(), String> {
    let pk: [u8; 32] = URL_SAFE_NO_PAD.decode(id).ok().and_then(|b| b.try_into().ok()).ok_or("INVALID_MESSAGE")?;
    app.svc.lock().unwrap().revoke(pk)
}

#[tauri::command]
fn get_settings(app: State<App>) -> Value {
    let (profiles, profiles_error_line) = match settings::profiles(&app.data_dir) {
        Ok(p) => (p.map(|l| l.into_iter().map(|(n, c)| json!({ "name": n, "command": c })).collect::<Vec<_>>()), None),
        Err(line) => (None, Some(line)),
    };
    json!({
        "autostart": autostart::is_enabled(autostart::RUN_KEY),
        "server": app.settings.lock().unwrap().server,
        "profiles": profiles, "profiles_error_line": profiles_error_line,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn apply_autostart(app: &AppHandle, on: bool) -> Result<(), String> {
    let r = autostart::command().and_then(|c| autostart::set(autostart::RUN_KEY, on, &c));
    refresh_tray(app, true);
    r
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<(), String> { apply_autostart(&app, on) }

/// Valida, grava e reinicia o host com o novo servidor.
#[tauri::command]
fn set_server(app: State<App>, server: String) -> Result<(), String> {
    let server = server.trim().to_string();
    if !settings::valid_server(&server) { return Err("INVALID_SERVER".into()); }
    let mut s = app.settings.lock().unwrap();
    let new = settings::Settings { server };
    settings::save(&app.data_dir, &new)?;
    let mut svc = app.svc.lock().unwrap();
    svc.stop();
    *app.link.lock().unwrap() = None;
    clip::clear_if_ours();
    *svc = Service::start(host::Config { data_dir: app.data_dir.clone(), server: new.server.clone(), room: host::room_id(&app.data_dir)? })?;
    *s = new;
    Ok(())
}

/// Abre a pasta de dados (perfis, trust store e, no futuro, logs) no Explorer.
#[tauri::command]
fn open_data_dir(app: State<App>) -> Result<(), String> {
    std::process::Command::new("explorer").arg(&app.data_dir).spawn().map(drop).map_err(|_| "IO_ERROR".into())
}

/// Abre (ou traz para frente) a janela já na aba pedida.
fn open_window(app: &AppHandle, tab: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.emit("tab", tab);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App(format!("index.html#{tab}").into()))
        .title("live-cmd").inner_size(420.0, 560.0).resizable(false).maximizable(false).center().build();
}

fn ask(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> bool {
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("live-cmd"), flags | MB_SETFOREGROUND) == IDOK }
}

fn quit(app: &AppHandle) {
    let n = app.state::<App>().svc.lock().unwrap().status().connected;
    // ponytail: MessageBox nativa tem botões OK/Cancelar, não "Sair"/"Cancelar"; usar tauri-plugin-dialog se o rótulo importar.
    let msg = format!("Sair do live-cmd?\n\n{} serão desconectados e os terminais abertos serão encerrados.",
        if n == 1 { "1 aparelho".to_string() } else { format!("{n} aparelhos") });
    if n > 0 && !ask(&msg, MB_OKCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2) { return; }
    let a = app.state::<App>();
    cancel(&a);
    a.svc.lock().unwrap().stop();
    app.exit(0);
}

fn build_menu(app: &AppHandle, state: HostState, connected: usize) -> tauri::Result<Menu<tauri::Wry>> {
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::new(app)?;
    if state == HostState::Error {
        menu.append(&MenuItem::with_id(app, "status", "⚠ Sem conexão com o servidor — tentando de novo", false, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "settings", "Ver detalhes", true, None::<&str>)?)?;
    } else if state == HostState::Connecting {
        menu.append(&MenuItem::with_id(app, "status", "Conectando ao servidor…", false, None::<&str>)?)?;
    } else {
        menu.append(&MenuItem::with_id(app, "status", format!("● Pronto · {}", devices_text(connected)), false, None::<&str>)?)?;
    }
    menu.append(&sep()?)?;
    // ponytail: Terminal Desktop é a fatia D5c; item visível e desabilitado até lá.
    menu.append(&MenuItem::with_id(app, "terminal", "Abrir Terminal Desktop", false, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "pair", "Novo Pareamento (Exibir QR Code)", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "devices", "Dispositivos Conectados", true, None::<&str>)?)?;
    menu.append(&sep()?)?;
    menu.append(&CheckMenuItem::with_id(app, "autostart", "Iniciar com o Windows", true, autostart::is_enabled(autostart::RUN_KEY), None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Configurações", true, None::<&str>)?)?;
    menu.append(&sep()?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?)?;
    Ok(menu)
}

/// Último (estado, conectados) desenhado na bandeja.
static TRAY_SHOWN: Mutex<Option<(HostState, usize)>> = Mutex::new(None);

/// Atualiza ícone, tooltip e menu quando o estado muda (ou `force`).
fn refresh_tray(app: &AppHandle, force: bool) {
    let s = app.state::<App>().svc.lock().unwrap().status();
    let key = (s.state, s.connected);
    {
        let mut shown = TRAY_SHOWN.lock().unwrap();
        if !force && *shown == Some(key) { return; }
        *shown = Some(key);
    }
    let Some(tray) = app.tray_by_id("main") else { return };
    let (icon, tip): (&[u8], String) = match s.state {
        HostState::Running => (include_bytes!("../icons/tray-running.png"), format!("live-cmd — pronto · {}", devices_text(s.connected))),
        HostState::Connecting => (include_bytes!("../icons/tray-connecting.png"), "live-cmd — conectando ao servidor…".into()),
        HostState::Error => (include_bytes!("../icons/tray-error.png"), "live-cmd — sem conexão com o servidor. Clique para ver.".into()),
    };
    let _ = tray.set_icon(Image::from_bytes(icon).ok());
    let _ = tray.set_tooltip(Some(tip));
    if let Ok(m) = build_menu(app, s.state, s.connected) { let _ = tray.set_menu(Some(m)); }
}

fn tray(app: &AppHandle) -> tauri::Result<TrayIcon> {
    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!("../icons/tray-connecting.png"))?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "pair" => open_window(app, "pair"),
            "devices" => open_window(app, "devices"),
            "settings" => open_window(app, "settings"),
            "autostart" => {
                let on = !autostart::is_enabled(autostart::RUN_KEY);
                if apply_autostart(app, on).is_err() {
                    // ponytail: MessageBox no lugar da notificação do Windows; trocar por tauri-plugin-notification se incomodar.
                    ask("Não foi possível ativar o início com o Windows.", MB_OK | MB_ICONWARNING);
                }
            }
            "quit" => quit(app),
            _ => {}
        })
        .build(app)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| open_window(app, "devices")))
        .invoke_handler(tauri::generate_handler![status, pair, cancel_pair, copy_link, devices, revoke, get_settings, set_autostart, set_server, open_data_dir])
        .setup(|a| {
            let dir = settings::data_dir()?;
            let s = settings::load(&dir)?;
            let svc = Service::start(host::Config { data_dir: dir.clone(), server: s.server.clone(), room: host::room_id(&dir)? })?;
            a.manage(App { svc: Mutex::new(svc), data_dir: dir, settings: Mutex::new(s), link: Mutex::new(None) });
            tray(a.handle())?;
            refresh_tray(a.handle(), true);
            // Laço de 1 s: bandeja segue o host; link sai do clipboard quando a OTK expira ou é usada.
            let h = a.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_secs(1));
                let app = h.state::<App>();
                if app.svc.lock().unwrap().status().pairing_expires_ms.is_none() && app.link.lock().unwrap().take().is_some() {
                    clip::clear_if_ours();
                }
                refresh_tray(&h, false);
            });
            Ok(())
        })
        .on_window_event(|w, e| {
            // Fechar a janela invalida a OTK; trocar de aba não (P13).
            if matches!(e, WindowEvent::CloseRequested { .. }) { cancel(&w.state::<App>()); }
        })
        .build(tauri::generate_context!())
        .expect("tauri")
        .run(|_, e| {
            // Sem janela o app continua na bandeja; só "Sair" encerra.
            if let RunEvent::ExitRequested { code: None, api, .. } = e { api.prevent_exit(); }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qr_nao_carrega_uri_em_texto() {
        let uri = "remoteterm://pair?v=1&session=s&otk=SEGREDO&host_pk=p&server=wss://x.dev";
        let url = qr_data_url(uri).unwrap();
        let svg = String::from_utf8(STANDARD.decode(url.strip_prefix("data:image/svg+xml;base64,").unwrap()).unwrap()).unwrap();
        assert!(svg.starts_with("<svg") && svg.contains("<path d=\"M"));
        assert!(!svg.contains("SEGREDO") && !svg.contains("remoteterm") && !url.contains("SEGREDO"));
    }
    #[test]
    fn texto_de_aparelhos() {
        assert_eq!(devices_text(0), "Nenhum aparelho conectado");
        assert_eq!(devices_text(1), "1 aparelho conectado");
        assert_eq!(devices_text(2), "2 aparelhos conectados");
    }
}

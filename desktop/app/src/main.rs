//! live-cmd: app de bandeja do Windows (design `docs/design/desktop-app.md`).
//! Sobe só na bandeja; a janela (Parear | Aparelhos | Configurações) abre pelo menu.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod clip;
mod daemon;
mod hist;
mod host;

mod settings;
mod ssh;
mod sys;
mod term;

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

use host::State as HostState;
use live_cmd_echo::ipc::{self, ServiceClient};
use settings::autostart;

struct App {
    /// O host mora no daemon `live-cmd-termd`; isto é só o cliente do pipe.
    svc: ServiceClient,
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
    // Sem o app, a câmera abre o release do APK; o leitor do app usa o que vem depois do `#`.
    let wrapped = format!("https://github.com/yagoriccomi/live-cmd/releases/latest#{uri}");
    let code = qrcode::QrCode::new(wrapped.as_bytes()).map_err(|_| "QR")?;
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

use tauri_plugin_updater::UpdaterExt;

/// Versão nova disponível no GitHub Releases, se houver.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    let u = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(u.map(|u| u.version))
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let u = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?.ok_or("sem atualização")?;
    // No Windows o updater lança o instalador NSIS e sai com `exit(0)` dentro de `download_and_install`;
    // o instalador reabre o app (`restart_after_install` é true por padrão). Logo, nada roda depois daqui.
    u.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn status(app: State<App>) -> Value {
    let s = app.svc.status();
    json!({ "state": state_name(s.state), "connected": s.connected, "pairing_expires_ms": s.pairing_expires_ms, "now_ms": host::now_ms() })
}

/// Link ainda válido: há URI guardado e o host ainda tem a OTK pendente.
fn pending(link: Option<String>, expires_ms: Option<u64>) -> Option<(String, u64)> {
    Some((link?, expires_ms?))
}

/// QR da OTK pendente, sem gerar outra (P13: reabrir/restaurar a janela mostra a mesma OTK).
#[tauri::command]
fn qr(app: State<App>) -> Result<Value, String> {
    let exp = app.svc.status().pairing_expires_ms;
    let (uri, exp) = pending(app.link.lock().unwrap().clone(), exp).ok_or("EXPIRED")?;
    Ok(json!({ "qr": qr_data_url(&uri)?, "expires_ms": exp, "now_ms": host::now_ms() }))
}

/// Nova OTK (invalida a anterior, C2 §1) e o QR dela.
#[tauri::command]
fn pair(app: State<App>) -> Result<Value, String> {
    clip::clear_if_ours();
    *app.link.lock().unwrap() = Some(app.svc.pair()?);
    qr(app)
}

fn cancel(app: &App) {
    app.svc.cancel_pair();
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
    let mut list = app.svc.devices();
    list.sort_by_key(|d| (!d.connected, std::cmp::Reverse(d.paired_at_ms)));
    list.into_iter().map(|d| json!({ "id": URL_SAFE_NO_PAD.encode(d.client_pk), "label": d.label, "paired_at_ms": d.paired_at_ms, "connected": d.connected })).collect()
}

#[tauri::command]
fn revoke(app: State<App>, id: String) -> Result<(), String> {
    let pk: [u8; 32] = URL_SAFE_NO_PAD.decode(id).ok().and_then(|b| b.try_into().ok()).ok_or("INVALID_MESSAGE")?;
    app.svc.revoke(pk)
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
        "scrollback": app.settings.lock().unwrap().scrollback(),
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
    let new = settings::Settings { server, scrollback: s.scrollback };
    settings::save(&app.data_dir, &new)?;
    *app.link.lock().unwrap() = None;
    clip::clear_if_ours();
    // O daemon relê o servidor do disco; as sessões continuam abertas.
    app.svc.restart()?;
    *s = new;
    Ok(())
}

/// Editor de perfis: lista atual (ou `null` sem arquivo) e shells detectados.
#[tauri::command]
fn get_profiles(app: State<App>) -> Value {
    let list = settings::profiles_full(&app.data_dir).ok().flatten();
    let shells: Vec<Value> = settings::detect_shells().into_iter().map(|(l, c)| json!({ "label": l, "command": c })).collect();
    json!({ "profiles": list, "shells": shells })
}

/// Grava `profiles.json` e reinicia o host (no daemon) para relê-lo; sessões abertas continuam (ficam no hub).
/// ponytail: o reinício derruba por instantes os outros PCs (reconectam sozinhos); trocar por recarga só dos perfis no host.
#[tauri::command]
fn save_profiles(app: State<App>, profiles: Vec<settings::Profile>) -> Result<Vec<settings::Profile>, String> {
    let saved = settings::save_profiles(&app.data_dir, profiles)?;
    app.svc.restart()?;
    Ok(saved)
}

/// Linhas de histórico por aba (100–100000); vale para abas novas.
#[tauri::command]
fn set_scrollback(app: State<App>, lines: usize) -> Result<(), String> {
    let mut s = app.settings.lock().unwrap();
    let new = settings::Settings { server: s.server.clone(), scrollback: Some(lines.clamp(100, 100_000)) };
    settings::save(&app.data_dir, &new)?;
    *s = new;
    Ok(())
}

fn apply_opacity(app: &AppHandle, w: &tauri::WebviewWindow) {
    sys::window_opacity(w, settings::appearance(&app.state::<App>().data_dir).opacity);
}

/// Aparência (tema, fonte, vidro, brilho, animações): a mesma que o celular lê e grava por `settings_get`/`settings_set`.
#[tauri::command]
fn get_appearance(app: State<App>) -> Value { settings::appearance(&app.data_dir).json() }

/// Só os campos mandados mudam (lista branca e faixas de `Appearance::patch`); todas as janelas recebem `appearance`.
#[tauri::command]
fn set_appearance(handle: AppHandle, app: State<App>, patch: serde_json::Map<String, Value>) -> Result<Value, String> {
    let a = settings::appearance(&app.data_dir).patch(&patch).map_err(|e| e.code().to_owned())?;
    settings::save_appearance(&app.data_dir, &a)?;
    let _ = handle.emit("appearance", a.json());
    for w in handle.webview_windows().values() { sys::window_opacity(w, a.opacity); }
    Ok(a.json())
}

/// Abre a pasta de dados (perfis, trust store e, no futuro, logs) no Explorer.
#[tauri::command]
fn open_data_dir(app: State<App>) -> Result<(), String> {
    std::process::Command::new("explorer").arg(&app.data_dir).spawn().map(drop).map_err(|_| "IO_ERROR".into())
}

/// Parear, Aparelhos e Configurações são painéis da janela do terminal (V1 Aurora): abre (ou traz
/// para frente) a janela e pede o painel. Janela nova recebe o painel pelo hash da URL.
fn open_window(app: &AppHandle, tab: &str) {
    if let Some(w) = app.get_webview_window("terminal") {
        let _ = w.emit("tab", tab);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    build_terminal(app, &format!("terminal.html#{tab}"));
}

/// Terminal Desktop (design §6): janela própria, redimensionável.
fn open_terminal(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("terminal") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    build_terminal(app, "terminal.html");
}

fn build_terminal(app: &AppHandle, url: &str) {
    let w = WebviewWindowBuilder::new(app, "terminal", WebviewUrl::App(url.into()))
        .title("Terminal Desktop — live-cmd").decorations(false).inner_size(1280.0, 800.0).min_inner_size(560.0, 360.0).center().build();
    if let Ok(w) = w { apply_opacity(app, &w); fit_window(&w, true); }
}

/// Encaixa a janela na área útil (sem a barra de tarefas) do monitor onde está o centro dela:
/// encolhe se não couber e puxa para dentro o que ficou fora da tela. `grow`: ocupa a área útil toda.
fn fit_window(w: &tauri::WebviewWindow, grow: bool) {
    if w.is_maximized().unwrap_or(false) || w.is_minimized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) { return; }
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let (cx, cy) = (pos.x as f64 + size.width as f64 / 2.0, pos.y as f64 + size.height as f64 / 2.0);
    let mon = w.monitor_from_point(cx, cy).ok().flatten().or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(mon) = mon else { return };
    let a = mon.work_area();
    let (ax, ay, aw, ah) = (a.position.x, a.position.y, a.size.width as i32, a.size.height as i32);
    let (mut ww, mut wh) = (size.width as i32, size.height as i32);
    if grow { ww = aw; wh = ah; } else { ww = ww.min(aw); wh = wh.min(ah); }
    let x = pos.x.clamp(ax, ax + aw - ww);
    let y = pos.y.clamp(ay, ay + ah - wh);
    if (ww, wh) != (size.width as i32, size.height as i32) {
        // outer_size inclui a borda; set_size é do conteúdo: desconta a diferença.
        let inner = w.inner_size().unwrap_or(size);
        let (dw, dh) = (size.width as i32 - inner.width as i32, size.height as i32 - inner.height as i32);
        let _ = w.set_size(tauri::PhysicalSize::new((ww - dw).max(1) as u32, (wh - dh).max(1) as u32));
    }
    if (x, y) != (pos.x, pos.y) { let _ = w.set_position(tauri::PhysicalPosition::new(x, y)); }
}

/// `Moved` chega a cada passo do arraste: encaixa só 400 ms depois do último, quando o usuário soltou.
fn fit_later(w: &tauri::Window) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GEN: AtomicU64 = AtomicU64::new(0);
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = w.app_handle().clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        if GEN.load(Ordering::SeqCst) != g { return; }
        // Botão do mouse ainda preso: arraste parado no meio, não solta ainda.
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0 { return fit_later_retry(&app, g); }
        if let Some(w) = app.get_webview_window("terminal") { fit_window(&w, false); }
    });
}
fn fit_later_retry(app: &AppHandle, g: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        while unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(windows::Win32::UI::Input::KeyboardAndMouse::VK_LBUTTON.0 as i32) } as u16 & 0x8000 != 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = g;
        if let Some(w) = app.get_webview_window("terminal") { fit_window(&w, false); }
    });
}

/// Menu ☰ do terminal: abre Parear/Aparelhos/Configurações ou sai.
#[tauri::command]
fn tray_action(app: AppHandle, id: String) {
    match id.as_str() {
        "pair" | "devices" | "settings" => open_window(&app, &id),
        "quit" => quit(&app),
        "stop_terminals" => stop_terminals(&app),
        _ => {}
    }
}

fn ask(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> bool {
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("live-cmd"), flags | MB_SETFOREGROUND) == IDOK }
}

/// "Sair" fecha só o app: terminais, programas e aparelhos seguem no daemon; abrir o app reconecta.
fn quit(app: &AppHandle) {
    cancel(&app.state::<App>());
    term::shutdown(app);
    app.exit(0);
}

/// "Encerrar terminais": para o daemon (sessões, programas e aparelhos caem) e fecha o app.
fn stop_terminals(app: &AppHandle) {
    let a = app.state::<App>();
    let n = a.svc.status().connected;
    // ponytail: MessageBox nativa tem botões OK/Cancelar, não "Encerrar"/"Cancelar"; usar tauri-plugin-dialog se o rótulo importar.
    let who = match n { 0 => String::new(), 1 => " 1 aparelho será desconectado.".into(), n => format!(" {n} aparelhos serão desconectados.") };
    let msg = format!("Encerrar os terminais deste PC?\n\nOs programas em execução nos terminais são encerrados.{who} O live-cmd também fecha. Os terminais abertos serão reabertos na próxima vez.");
    if !ask(&msg, MB_OKCANCEL | MB_ICONQUESTION | MB_DEFBUTTON2) { return; }
    cancel(&a);
    term::shutdown(app);
    if let Err(e) = a.svc.shutdown() { live_cmd_echo::hlog(&format!("app: encerrar terminais falhou ({e})")); }
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
    menu.append(&MenuItem::with_id(app, "terminal", "Abrir Terminal Desktop", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "pair", "Novo Pareamento (Exibir QR Code)", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "devices", "Dispositivos Conectados", true, None::<&str>)?)?;
    menu.append(&sep()?)?;
    menu.append(&CheckMenuItem::with_id(app, "autostart", "Iniciar com o Windows", true, autostart::is_enabled(autostart::RUN_KEY), None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Configurações", true, None::<&str>)?)?;
    menu.append(&sep()?)?;
    menu.append(&MenuItem::with_id(app, "stop_terminals", "Encerrar terminais…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?)?;
    Ok(menu)
}

/// Último (estado, conectados) desenhado na bandeja.
static TRAY_SHOWN: Mutex<Option<(HostState, usize)>> = Mutex::new(None);

/// Atualiza ícone, tooltip e menu quando o estado muda (ou `force`).
fn refresh_tray(app: &AppHandle, force: bool) {
    let s = app.state::<App>().svc.status();
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
        // Clique simples não abre o menu (engolia o duplo clique); menu fica no botão direito.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|t, e| if let tauri::tray::TrayIconEvent::DoubleClick { .. } = e { open_terminal(t.app_handle()) })
        .on_menu_event(|app, e| match e.id().as_ref() {
            "terminal" => open_terminal(app),
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
            "stop_terminals" => stop_terminals(app),
            _ => {}
        })
        .build(app)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| open_window(app, "devices")))
        .invoke_handler(tauri::generate_handler![tray_action, status, qr, pair, cancel_pair, copy_link, devices, revoke, get_settings, set_autostart, set_server, open_data_dir, get_profiles, save_profiles, set_scrollback, check_update, install_update,
            term::term_list, term::term_create, term::term_attach, term::term_detach, term::term_close, term::term_input, term::term_paste,
            term::term_resize, term::term_scroll, term::clip_copy, term::term_connections, term::remote_add, term::remote_connect,
            term::remote_disconnect, term::remote_remove,
            term::term_export, term::term_export_file, term::term_clear, term::term_upload, term::term_conn_stats, term::term_busy, term::term_rename, term::term_dirs, term::term_layout_get, term::term_layout_set, term::remote_rename,
            agents::agent_list, agents::agent_resume, agents::agent_remove, agents::agent_default_get, agents::agent_default_set,
            ssh::ssh_list, ssh::ssh_save, ssh::ssh_import, ssh::ssh_remove, ssh::ssh_probe, ssh::ssh_connect, ssh::ssh_sessions, ssh::ssh_reconnect, ssh::ssh_git, ssh::ssh_stats,
            hist::hist_meta, hist::hist_list, hist::hist_clear, hist::hist_sessions, sys::sys_stats, get_appearance, set_appearance])
        .setup(|a| {
            let dir = settings::data_dir()?;
            let s = settings::load(&dir)?;
            // Host, sessões deste PC e telas do celular (Agentes, SSH, Histórico) moram no daemon, que lê os
            // mesmos arquivos; o histórico das abas de outros PCs é gravado aqui, no mesmo `history.jsonl`.
            let history = hist::History::default();
            let ep = ipc::Endpoint::user()?;
            if let Err(e) = daemon::ensure(&ep) { live_cmd_echo::hlog(&format!("app: daemon indisponível ({e})")); }
            let svc = ServiceClient::new(ep);
            // Celular mudou a aparência ou um agente passou a esperar confirmação: as janelas desktop recebem o evento.
            let h = a.handle().clone();
            svc.watch(move |ev: &str, v: Value| {
                if ev == "appearance" && let Some(o) = v["opacity"].as_u64() { for w in h.webview_windows().values() { sys::window_opacity(w, o as u8); } }
                let _ = h.emit(ev, v);
            });
            a.manage(App { svc, data_dir: dir, settings: Mutex::new(s), link: Mutex::new(None) });
            a.manage(term::Term::default());
            a.manage(history);
            term::start(a.handle());
            tray(a.handle())?;
            refresh_tray(a.handle(), true);
            // Laço de 1 s: bandeja segue o host; link sai do clipboard quando a OTK expira ou é usada.
            // A cada 5 s confere o daemon e sobe outro se ele saiu (travou, foi encerrado por fora).
            let h = a.handle().clone();
            std::thread::spawn(move || for tick in 1u64.. {
                std::thread::sleep(Duration::from_secs(1));
                let app = h.state::<App>();
                if tick % 5 == 0 && matches!(ipc::probe(&app.svc.ep), Err(ipc::Fail::Absent | ipc::Fail::Version(_)))
                    && let Err(e) = daemon::ensure(&app.svc.ep) { live_cmd_echo::hlog(&format!("app: daemon indisponível ({e})")); }
                if app.svc.status().pairing_expires_ms.is_none() && app.link.lock().unwrap().take().is_some() {
                    clip::clear_if_ours();
                }
                refresh_tray(&h, false);
            });
            Ok(())
        })
        .on_window_event(|w, e| {
            // P13: fechar/minimizar não invalida a OTK (vale até 300 s ou o uso). Fechar destrói a
            // janela e o QR com ela; minimizar avisa a UI para tirar o QR da tela até restaurar.
            match (w.label(), e) {
                ("terminal", WindowEvent::Moved(_)) => fit_later(w),
                ("terminal", WindowEvent::ScaleFactorChanged { .. }) => fit_later(w),
                ("terminal", WindowEvent::Resized(_)) => { let _ = w.emit("visible", !w.is_minimized().unwrap_or(false)); }
                ("terminal", WindowEvent::Destroyed) => term::shutdown(w.app_handle()),
                _ => {}
            }
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
    fn qr_so_com_otk_pendente() {
        assert_eq!(pending(Some("u".into()), Some(5)), Some(("u".into(), 5)));
        assert_eq!(pending(Some("u".into()), None), None); // usada ou expirada no host
        assert_eq!(pending(None, Some(5)), None); // host reiniciado sem link nosso
    }
    #[test]
    fn texto_de_aparelhos() {
        assert_eq!(devices_text(0), "Nenhum aparelho conectado");
        assert_eq!(devices_text(1), "1 aparelho conectado");
        assert_eq!(devices_text(2), "2 aparelhos conectados");
    }
}

//! Histórico de comandos: o arquivo e as regras estão em `live_cmd_echo::appdata::hist` (o daemon grava
//! as sessões deste PC no mesmo `history.jsonl`); aqui fica o estado das abas de outros PCs e as telas.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::App;

pub use live_cmd_echo::appdata::hist::*;

// ---------- Estado no app ----------

#[derive(Default)]
struct Meta { kind: String, label: String, pc: String, cwd: Option<String> }

#[derive(Default)]
pub struct History {
    /// Também entregue ao provedor do celular (`live_cmd_echo::appdata::AppProvider`).
    pub store: Shared,
    /// Chave `"<conn>/<session_id>"`.
    meta: Mutex<HashMap<String, Meta>>,
    typed: Mutex<HashMap<String, Typed>>,
    /// Último comando de cada aba, à espera do código de saída (OSC 133).
    pending: Mutex<HashMap<String, u64>>,
}

fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }

fn with_store<R>(app: &AppHandle, f: impl FnOnce(&mut Store) -> R) -> R {
    with_shared(&app.state::<History>().store, &app.state::<App>().data_dir, f)
}

/// `cwd_changed` do host (term.rs): a pasta vai junto dos próximos comandos da aba.
pub fn record_cwd(app: &AppHandle, key: &str, cwd: Option<&str>) {
    let h = app.state::<History>();
    h.meta.lock().unwrap().entry(key.into()).or_insert_with(|| Meta { kind: "outro".into(), label: "Terminal".into(), pc: String::new(), cwd: None }).cwd = cwd.map(str::to_owned);
}

/// Chamado por `term_input` (term.rs) só para abas de outro PC: as deste PC, venham do terminal desktop ou do
/// celular, o host grava no ponto comum da entrada (`appdata::AppProvider`, `Provider::session_input`), uma vez só.
/// `line`: linha do cursor se `data` tem Enter e a tela é a normal.
pub fn record_input(app: &AppHandle, key: &str, conn: &str, data: &str, line: Option<String>) {
    let h = app.state::<History>();
    let cmds = on_input(h.typed.lock().unwrap().entry(key.into()).or_default(), data, line.as_deref());
    if cmds.is_empty() { return; }
    let fallback_pc = || if conn == "local" { "Este PC".to_owned() } else { "Outro PC".to_owned() };
    let (kind, label, pc, cwd) = match h.meta.lock().unwrap().get(key) {
        Some(m) => (m.kind.clone(), m.label.clone(), if m.pc.is_empty() { fallback_pc() } else { m.pc.clone() }, m.cwd.clone()),
        None => ("outro".into(), "Terminal".into(), fallback_pc(), None),
    };
    if kind == "ia" { return; }
    for c in cmds {
        if let Ok(id) = with_store(app, |s| s.add_in(now_ms(), &c, &kind, &label, &pc, cwd.as_deref())) { h.pending.lock().unwrap().insert(key.into(), id); }
    }
}

/// `command_completed` do host: código de saída do último comando da aba.
pub fn record_done(app: &AppHandle, key: &str, code: Option<i64>) {
    let Some(id) = app.state::<History>().pending.lock().unwrap().remove(key) else { return };
    if let Some(c) = code { let _ = with_store(app, |s| s.set_code(id, c)); }
}

/// Rótulo e PC da aba, para gravar junto do comando.
#[tauri::command]
pub fn hist_meta(app: AppHandle, conn: String, session_id: String, label: String, pc: String) {
    let kind = kind_of(&label).to_owned();
    let h = app.state::<History>();
    let mut meta = h.meta.lock().unwrap();
    // A pasta pode ter chegado antes (cwd_changed no attach): fica.
    let cwd = meta.get(&format!("{conn}/{session_id}")).and_then(|m| m.cwd.clone());
    let m = Meta { kind, label: label.chars().take(64).collect(), pc: pc.chars().take(40).collect(), cwd };
    meta.insert(format!("{conn}/{session_id}"), m);
}

#[tauri::command(async)]
pub fn hist_list(app: AppHandle, query: Query) -> Vec<Entry> {
    with_store(&app, |s| s.search(&query, now_ms()))
}

#[tauri::command(async)]
pub fn hist_sessions(app: AppHandle, query: Query) -> Vec<serde_json::Value> {
    with_store(&app, |s| sessions(s, &query, now_ms())).into_iter().map(|g| serde_json::json!({
        "label": g.label, "kind": g.kind, "pc": g.pc, "day": g.day, "count": g.count, "errors": g.errors, "last_ms": g.last_ms,
    })).collect()
}

#[tauri::command(async)]
pub fn hist_clear(app: AppHandle) -> Result<(), String> {
    app.state::<History>().pending.lock().unwrap().clear();
    with_store(&app, |s| s.clear()).map_err(|_| "IO_ERROR".into())
}

//! Terminal Desktop (design §6): sessões deste PC (hub em processo, P5) e de outros PCs
//! (offerer C2/C3, pins em `remotes.json`). A tela de cada aba é um `vt100::Parser` aqui;
//! a WebView só desenha as linhas prontas. Nunca logar link, OTK, nomes nem conteúdo.
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::host::{Client, ClientEvent, RemotePin, parse_pair_link};
use crate::{App, clip, settings};

const LOCAL: &str = "local";
const WIN: &str = "terminal";
const FILE: &str = "remotes.json";
const SCROLLBACK: usize = 1000;

/// PC remoto pareado. `id` = `host_pk` em base64url.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Remote { pub id: String, pub name: String, pub pin: RemotePin }

struct View { vt: vt100::Parser, next: u64 }

#[derive(Default)]
pub struct Term {
    local: Mutex<Option<Arc<Client>>>,
    remotes: Mutex<HashMap<String, Arc<Client>>>,
    /// Pareamentos em curso: o pin só é gravado no primeiro `Ready`.
    pending: Mutex<HashMap<String, Remote>>,
    /// Estado da conexão para a UI: `{state: connecting|ready|closed|off, attempt?, code?}`.
    states: Mutex<HashMap<String, Value>>,
    /// Chave `"<conn>/<session_id>"`.
    views: Mutex<HashMap<String, View>>,
    dirty: Mutex<HashSet<String>>,
}

fn nz(n: u16) -> NonZeroU16 { NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN) }
fn key(conn: &str, sid: &str) -> String { format!("{conn}/{sid}") }
fn parser(rows: u16, cols: u16) -> vt100::Parser { vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK) }

/// Saída a partir de `offset`; o que já foi desenhado (replay repetido) é pulado.
fn feed(v: &mut View, offset: u64, data: &[u8]) {
    let end = offset + data.len() as u64;
    if end <= v.next { return; }
    v.vt.process(&data[v.next.saturating_sub(offset) as usize..]);
    v.next = end;
}

/// Paleta xterm-256; as 16 primeiras são a "Campbell" do Windows Terminal (conteúdo do terminal, não tema).
fn idx_rgb(n: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (12, 12, 12), (197, 15, 31), (19, 161, 14), (193, 156, 0), (0, 55, 218), (136, 23, 152), (58, 150, 221), (204, 204, 204),
        (118, 118, 118), (231, 72, 86), (22, 198, 12), (249, 241, 165), (59, 120, 255), (180, 0, 158), (97, 214, 214), (242, 242, 242),
    ];
    let f = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
    match n {
        0..=15 => BASE[n as usize],
        16..=231 => { let i = n - 16; (f(i / 36), f(i / 6 % 6), f(i % 6)) }
        _ => { let g = 8 + (n - 232) * 10; (g, g, g) }
    }
}

fn color(c: vt100::Color) -> Value {
    let (r, g, b) = match c { vt100::Color::Default => return Value::Null, vt100::Color::Idx(n) => idx_rgb(n), vt100::Color::Rgb(r, g, b) => (r, g, b) };
    format!("#{r:02x}{g:02x}{b:02x}").into()
}

/// Tela pronta: linhas de trechos `[texto, fg, bg, flags]` (1 negrito, 2 itálico, 4 sublinhado, 8 inverso, 16 cursor).
fn render(vt: &vt100::Parser) -> Value {
    let s = vt.screen();
    let (rows, cols) = s.size();
    let cursor = (!s.hide_cursor() && s.scrollback() == 0).then(|| s.cursor_position());
    let lines: Vec<Value> = (0..rows.get()).map(|r| {
        let mut runs: Vec<(String, Value, Value, u8)> = vec![];
        for c in 0..cols.get() {
            let Some(cell) = s.cell(r, c) else { break };
            if cell.is_wide_continuation() { continue; }
            let (fg, bg) = (color(cell.fgcolor()), color(cell.bgcolor()));
            let fl = cell.bold() as u8 | (cell.italic() as u8) << 1 | (cell.underline() as u8) << 2 | (cell.inverse() as u8) << 3 | ((cursor == Some((r, c))) as u8) << 4;
            let text = if cell.has_contents() { cell.contents() } else { " " };
            match runs.last_mut() {
                Some(l) if l.1 == fg && l.2 == bg && l.3 == fl => l.0.push_str(text),
                _ => runs.push((text.into(), fg, bg, fl)),
            }
        }
        // Espaços finais sem fundo nem inverso não desenham nada.
        if let Some(l) = runs.last_mut() && l.2.is_null() && l.3 & 28 == 0 { l.0.truncate(l.0.trim_end_matches(' ').len()); }
        if runs.last().is_some_and(|l| l.0.is_empty()) { runs.pop(); }
        Value::Array(runs.into_iter().map(|(t, f, b, fl)| json!([t, f, b, fl])).collect())
    }).collect();
    json!({ "rows": lines, "app_cursor": s.application_cursor() })
}

fn load(dir: &Path) -> Result<Vec<Remote>, String> {
    match std::fs::read(dir.join(FILE)) {
        Ok(b) => serde_json::from_slice(&b).map_err(|_| "IO_ERROR".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err("IO_ERROR".into()),
    }
}

fn store(dir: &Path, list: &[Remote]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "IO_ERROR")?;
    let tmp = dir.join("remotes.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(list).unwrap()).map_err(|_| "IO_ERROR")?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|_| "IO_ERROR".into())
}

fn upsert(dir: &Path, r: Remote) -> Result<(), String> {
    let mut l = load(dir)?;
    match l.iter_mut().find(|x| x.id == r.id) { Some(x) => *x = r, None => l.push(r) }
    store(dir, &l)
}

/// Servidor aceito no link: o configurado neste app e a sinalização local, como `host[:porta]` (C2 §1).
/// ponytail: allowlist = servidores conhecidos deste app; trocar pela lista embutida de C2 quando houver deploy.
fn allowlist(server: &str) -> Vec<String> {
    [server, settings::DEFAULT_SERVER].iter()
        .map(|s| s.split_once("://").map_or(*s, |(_, r)| r).split(['/', '?', '#']).next().unwrap_or("").to_owned())
        .collect()
}

fn default_name(list: &[Remote]) -> String {
    (1..).map(|n| if n == 1 { "Outro PC".to_string() } else { format!("Outro PC {n}") })
        .find(|n| !list.iter().any(|r| &r.name == n)).unwrap()
}

fn set_state(app: &AppHandle, conn: &str, v: Value) {
    app.state::<Term>().states.lock().unwrap().insert(conn.into(), v.clone());
    let mut p = v;
    p["conn"] = conn.into();
    let _ = app.emit_to(WIN, "term-conn", p);
}

fn on_event(app: &AppHandle, conn: &str, e: ClientEvent) {
    let t = app.state::<Term>();
    match e {
        ClientEvent::Output { session_id, offset, data } => {
            let k = key(conn, &session_id);
            if let Some(v) = t.views.lock().unwrap().get_mut(&k) { feed(v, offset, &data); t.dirty.lock().unwrap().insert(k); }
        }
        ClientEvent::Control(c) if c["type"] == "screen_snapshot" => {
            let k = key(conn, c["session_id"].as_str().unwrap_or_default());
            let data = c["data"].as_str().and_then(|d| URL_SAFE_NO_PAD.decode(d).ok());
            let offset = c["offset"].as_str().and_then(|o| o.parse().ok());
            if let (Some(v), Some(data), Some(offset)) = (t.views.lock().unwrap().get_mut(&k), data, offset) {
                let (rows, cols) = v.vt.screen().size();
                v.vt = parser(rows.get(), cols.get());
                v.vt.process(&data);
                v.next = offset;
                t.dirty.lock().unwrap().insert(k);
            }
        }
        // F4: pasta atual (OSC 7 no host) para "Nova sessão aqui".
        ClientEvent::Control(c) if c["type"] == "cwd_changed" => {
            let _ = app.emit_to(WIN, "term-cwd", json!({ "key": key(conn, c["session_id"].as_str().unwrap_or_default()), "cwd": c["cwd"] }));
        }
        ClientEvent::Control(_) => {}
        ClientEvent::Exited { session_id, code } => { let _ = app.emit_to(WIN, "term-exited", json!({ "key": key(conn, &session_id), "code": code })); }
        ClientEvent::Connecting { attempt } => set_state(app, conn, json!({ "state": "connecting", "attempt": attempt })),
        ClientEvent::Ready => {
            let saved = t.pending.lock().unwrap().remove(conn).map(|r| upsert(&app.state::<App>().data_dir, r));
            match saved {
                Some(Err(e)) => set_state(app, conn, json!({ "state": "closed", "code": e })),
                _ => set_state(app, conn, json!({ "state": "ready" })),
            }
        }
        ClientEvent::Closed { code } => {
            t.pending.lock().unwrap().remove(conn);
            t.remotes.lock().unwrap().remove(conn);
            set_state(app, conn, json!({ "state": "closed", "code": code }));
        }
    }
}

/// Desenha as abas com saída nova, no máximo a cada 33 ms.
pub fn start(app: &AppHandle) {
    let h = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(33));
        let t = h.state::<Term>();
        let keys: Vec<String> = t.dirty.lock().unwrap().drain().collect();
        for k in keys {
            let Some(mut s) = t.views.lock().unwrap().get(&k).map(|v| render(&v.vt)) else { continue };
            s["key"] = k.into();
            let _ = h.emit_to(WIN, "term-screen", s);
        }
    });
}

/// Janela fechada: larga clientes e telas (desanexa tudo; as sessões continuam no host).
pub fn shutdown(app: &AppHandle) {
    let t = app.state::<Term>();
    *t.local.lock().unwrap() = None;
    t.remotes.lock().unwrap().clear();
    t.pending.lock().unwrap().clear();
    t.states.lock().unwrap().clear();
    t.views.lock().unwrap().clear();
}

fn client(app: &AppHandle, conn: &str) -> Result<Arc<Client>, String> {
    let t = app.state::<Term>();
    if conn == LOCAL {
        let mut l = t.local.lock().unwrap();
        if l.is_none() {
            let h = app.clone();
            *l = Some(Arc::new(Client::local(&app.state::<App>().svc.lock().unwrap(), move |e| on_event(&h, LOCAL, e))));
        }
        return Ok(l.clone().unwrap());
    }
    t.remotes.lock().unwrap().get(conn).cloned().ok_or_else(|| "NOT_CONNECTED".into())
}

fn connect(app: &AppHandle, id: &str, pin: RemotePin, otk: Option<[u8; 32]>) -> Result<(), String> {
    let (h, cid) = (app.clone(), id.to_owned());
    // Trava durante a criação: um `Closed` rápido só remove depois da inserção.
    let t = app.state::<Term>();
    let mut m = t.remotes.lock().unwrap();
    m.insert(id.into(), Arc::new(Client::remote(pin, otk, move |e| on_event(&h, &cid, e))?));
    Ok(())
}

fn with_view(app: &AppHandle, conn: &str, sid: &str, f: impl FnOnce(&mut View)) {
    let t = app.state::<Term>();
    let k = key(conn, sid);
    if let Some(v) = t.views.lock().unwrap().get_mut(&k) { f(v); t.dirty.lock().unwrap().insert(k); }
}

#[tauri::command(async)]
pub fn term_list(app: AppHandle, conn: String) -> Result<Value, String> {
    client(&app, &conn)?.request(json!({ "type": "session_list" }))
}

#[tauri::command(async)]
pub fn term_create(app: AppHandle, conn: String, profile_id: String, label: String, cols: u16, rows: u16, cwd: Option<String>) -> Result<String, String> {
    let label: String = label.chars().take(64).collect();
    let mut req = json!({ "type": "session_create", "profile_id": profile_id, "label": label, "cols": cols, "rows": rows });
    if let Some(d) = cwd { req["cwd"] = d.into(); }
    let v = client(&app, &conn)?.request(req)?;
    Ok(v["session_id"].as_str().unwrap_or_default().to_owned())
}

/// Anexa do começo do buffer numa tela nova (também ao reconectar).
#[tauri::command(async)]
pub fn term_attach(app: AppHandle, conn: String, session_id: String, cols: u16, rows: u16) -> Result<Value, String> {
    let c = client(&app, &conn)?;
    let t = app.state::<Term>();
    let (k, views) = (key(&conn, &session_id), &t.views);
    views.lock().unwrap().insert(k.clone(), View { vt: parser(rows, cols), next: 0 });
    let r = c.request(json!({ "type": "session_attach", "session_id": session_id, "from_offset": null, "cols": cols, "rows": rows }));
    if r.is_err() { views.lock().unwrap().remove(&k); }
    r
}

#[tauri::command(async)]
pub fn term_detach(app: AppHandle, conn: String, session_id: String) -> Result<(), String> {
    app.state::<Term>().views.lock().unwrap().remove(&key(&conn, &session_id));
    client(&app, &conn)?.request(json!({ "type": "session_detach", "session_id": session_id })).map(drop)
}

#[tauri::command(async)]
pub fn term_close(app: AppHandle, conn: String, session_id: String) -> Result<(), String> {
    client(&app, &conn)?.request(json!({ "type": "session_close", "session_id": session_id })).map(drop)
}

#[tauri::command(async)]
pub fn term_input(app: AppHandle, conn: String, session_id: String, data: String) -> Result<(), String> {
    with_view(&app, &conn, &session_id, |v| v.vt.screen_mut().set_scrollback(0));
    client(&app, &conn)?.input(&session_id, data.as_bytes())
}

/// `Ctrl+Shift+V`: texto do clipboard, com bracketed paste se o programa pediu.
#[tauri::command(async)]
pub fn term_paste(app: AppHandle, conn: String, session_id: String) -> Result<(), String> {
    let text = clip::read_text()?.replace("\r\n", "\r").replace('\n', "\r");
    let mut bracketed = false;
    with_view(&app, &conn, &session_id, |v| { bracketed = v.vt.screen().bracketed_paste(); v.vt.screen_mut().set_scrollback(0); });
    let data = if bracketed { format!("\x1b[200~{text}\x1b[201~") } else { text };
    client(&app, &conn)?.input(&session_id, data.as_bytes())
}

#[tauri::command(async)]
pub fn term_resize(app: AppHandle, conn: String, session_id: String, cols: u16, rows: u16) -> Result<(), String> {
    with_view(&app, &conn, &session_id, |v| v.vt.screen_mut().set_size(nz(rows), nz(cols)));
    client(&app, &conn)?.resize(&session_id, cols, rows)
}

/// Roda do mouse: `lines > 0` sobe no histórico.
#[tauri::command]
pub fn term_scroll(app: AppHandle, conn: String, session_id: String, lines: i32) {
    with_view(&app, &conn, &session_id, |v| { let n = v.vt.screen().scrollback() as i64 + lines as i64; v.vt.screen_mut().set_scrollback(n.max(0) as usize); });
}

#[tauri::command]
pub fn clip_copy(text: String) -> Result<(), String> { clip::write_text(&text) }

#[tauri::command]
pub fn term_connections(app: AppHandle) -> Result<Value, String> {
    let list = load(&app.state::<App>().data_dir)?;
    let t = app.state::<Term>();
    let states = t.states.lock().unwrap();
    let remotes: Vec<Value> = list.into_iter().map(|r| json!({
        "id": r.id, "name": r.name, "status": states.get(&r.id).cloned().unwrap_or(json!({ "state": "off" })),
    })).collect();
    Ok(json!({ "remotes": remotes }))
}

/// "Adicionar PC": valida o link, tira do clipboard se ainda estiver lá e pareia.
/// O pin só é gravado quando o pareamento conclui (evento `term-conn` `ready`).
#[tauri::command(async)]
pub fn remote_add(app: AppHandle, link: String, name: String) -> Result<Value, String> {
    let allow = allowlist(&app.state::<App>().settings.lock().unwrap().server);
    let parsed = parse_pair_link(&link, &allow.iter().map(String::as_str).collect::<Vec<_>>());
    clip::clear_if_text(link.trim());
    drop(link);
    let (pin, otk) = parsed?;
    let id = URL_SAFE_NO_PAD.encode(pin.host_pk);
    let name = match name.trim() { "" => default_name(&load(&app.state::<App>().data_dir)?), n => n.chars().take(40).collect() };
    let t = app.state::<Term>();
    t.remotes.lock().unwrap().remove(&id);
    t.pending.lock().unwrap().insert(id.clone(), Remote { id: id.clone(), name: name.clone(), pin: pin.clone() });
    set_state(&app, &id, json!({ "state": "connecting", "attempt": 0 }));
    if let Err(e) = connect(&app, &id, pin, Some(otk)) { t.pending.lock().unwrap().remove(&id); return Err(e); }
    Ok(json!({ "id": id, "name": name }))
}

#[tauri::command(async)]
pub fn remote_connect(app: AppHandle, id: String) -> Result<(), String> {
    if app.state::<Term>().remotes.lock().unwrap().contains_key(&id) { return Ok(()); }
    let r = load(&app.state::<App>().data_dir)?.into_iter().find(|r| r.id == id).ok_or("NOT_FOUND")?;
    connect(&app, &id, r.pin, None)
}

#[tauri::command]
pub fn remote_disconnect(app: AppHandle, id: String) {
    let t = app.state::<Term>();
    t.remotes.lock().unwrap().remove(&id);
    t.pending.lock().unwrap().remove(&id);
    set_state(&app, &id, json!({ "state": "off" }));
}

/// "Remover este PC": apaga só o pin local; o outro PC continua listando este até revogar lá.
#[tauri::command]
pub fn remote_remove(app: AppHandle, id: String) -> Result<(), String> {
    remote_disconnect(app.clone(), id.clone());
    let t = app.state::<Term>();
    t.states.lock().unwrap().remove(&id);
    t.views.lock().unwrap().retain(|k, _| !k.starts_with(&format!("{id}/")));
    let dir = &app.state::<App>().data_dir;
    let mut l = load(dir)?;
    l.retain(|r| r.id != id);
    store(dir, &l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tela_em_trechos_com_cor_e_cursor() {
        let mut vt = parser(2, 10);
        vt.process(b"\x1b[31mab\x1b[0m c\x1b[1;7mX");
        let s = render(&vt);
        assert_eq!(s["rows"][0], json!([["ab", "#c50f1f", null, 0], [" c", null, null, 0], ["X", null, null, 9], [" ", null, null, 16]]));
        assert_eq!(s["rows"][1], json!([]));
        vt.process(b"\x1b[?25l\x1b[?1h");
        let s = render(&vt);
        assert_eq!(s["rows"][0].as_array().unwrap().len(), 3);
        assert!(s["app_cursor"] == true);
        assert_eq!(color(vt100::Color::Idx(196)), "#ff0000");
        assert_eq!(color(vt100::Color::Idx(232)), "#080808");
    }

    #[test]
    fn replay_repetido_nao_desenha_duas_vezes() {
        let mut v = View { vt: parser(2, 20), next: 0 };
        feed(&mut v, 0, b"abc");
        feed(&mut v, 0, b"abc");
        feed(&mut v, 2, b"cde");
        assert_eq!(v.vt.screen().contents(), "abcde");
    }

    #[test]
    fn pins_gravam_trocam_e_removem() {
        let dir = std::env::temp_dir().join(format!("live-cmd-term-{}", std::process::id()));
        let pin = RemotePin { host_pk: [1; 32], room: "r".into(), server: "127.0.0.1:8787".into() };
        let r = Remote { id: "a".into(), name: "Notebook".into(), pin };
        assert_eq!(load(&dir).unwrap(), vec![]);
        upsert(&dir, r.clone()).unwrap();
        upsert(&dir, Remote { name: "Sala".into(), ..r.clone() }).unwrap();
        let l = load(&dir).unwrap();
        assert_eq!((l.len(), l[0].name.as_str()), (1, "Sala"));
        assert_eq!(default_name(&l), "Outro PC");
        assert_eq!(default_name(&[Remote { name: "Outro PC".into(), ..r }]), "Outro PC 2");
        store(&dir, &[]).unwrap();
        assert_eq!(load(&dir).unwrap(), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allowlist_sem_esquema() {
        assert_eq!(allowlist("wss://sig.example.com/x"), vec!["sig.example.com", "live-cmd-signaling.yago-riccomi-s.workers.dev"]);
    }
}

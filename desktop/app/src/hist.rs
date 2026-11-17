//! Histórico de comandos (tela Histórico): grava localmente em `history.jsonl`, nunca envia para fora.
//! A linha do comando sai da tela (vt100) no Enter; prompts de senha e telas sem eco são ignorados.
//! Formato: uma `Entry` por linha; o código de saída chega depois como `{"id":N,"code":C}` (remendo).
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::App;

pub const FILE: &str = "history.jsonl";
/// Entradas guardadas; as mais antigas saem primeiro.
pub const MAX: usize = 5000;
const MAX_CMD: usize = 2000;
/// Palavras que marcam prompt de senha na linha do cursor (minúsculas).
const SECRET: [&str; 5] = ["password", "senha", "passphrase", "palavra-passe", "contraseña"];
/// Fim de prompt comum: PowerShell/cmd `>`, bash `$`, root `#`, zsh `%`, temas `❯ λ »`.
const PROMPT_END: [&str; 7] = ["> ", "$ ", "# ", "% ", "❯ ", "λ ", "» "];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: u64,
    /// Hora em ms desde 1970 (UTC); a UI formata no fuso local.
    pub ts: u64,
    pub cmd: String,
    /// `pwsh`, `cmd`, `wsl`, `ssh` ou `outro`.
    pub kind: String,
    /// Rótulo da aba (ex.: "PowerShell", "ssh deploy@srv").
    pub label: String,
    /// PC da sessão ("Este PC" ou o nome do PC remoto).
    pub pc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    /// Pasta da aba no Enter, quando o shell anuncia (OSC 7 / 9;9 lidos pelo host → `cwd_changed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// Store único do processo: a tela desktop e o celular (`mobile.rs`) leem, gravam e limpam o mesmo,
/// para "Limpar" de um lado não ser desfeito pela cópia em memória do outro.
pub type Shared = std::sync::Arc<Mutex<Option<Store>>>;

/// Abre `dir/history.jsonl` na primeira vez e aplica `f` com o lock tomado.
pub fn with_shared<R>(s: &Shared, dir: &std::path::Path, f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = s.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| Store::open(dir.join(FILE), MAX)))
}

#[derive(Deserialize)]
struct Patch { id: u64, code: Option<i64> }

#[derive(Deserialize, Default)]
pub struct Query {
    #[serde(default)]
    pub q: String,
    /// Só os últimos N dias.
    pub days: Option<u32>,
    pub kind: Option<String>,
    /// Só código de saída diferente de 0.
    #[serde(default)]
    pub errors: bool,
    pub limit: Option<usize>,
}

pub struct Store { path: PathBuf, entries: Vec<Entry>, lines: usize, max: usize }

impl Store {
    /// Lê o arquivo (ausente ou ilegível = vazio; linha corrompida é pulada).
    pub fn open(path: PathBuf, max: usize) -> Store {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut entries: Vec<Entry> = vec![];
        let mut lines = 0;
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            lines += 1;
            if let Ok(e) = serde_json::from_str::<Entry>(l) { entries.push(e); }
            else if let Ok(p) = serde_json::from_str::<Patch>(l) && let Some(e) = entries.iter_mut().rev().find(|e| e.id == p.id) { e.code = p.code; }
        }
        let cut = entries.len().saturating_sub(max);
        entries.drain(..cut);
        Store { path, entries, lines, max }
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[Entry] { &self.entries }

    pub fn get(&self, id: u64) -> Option<&Entry> { self.entries.iter().rev().find(|e| e.id == id) }

    fn append(&mut self, line: &str) -> std::io::Result<()> {
        if let Some(d) = self.path.parent() { std::fs::create_dir_all(d)?; }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        f.write_all(format!("{line}\n").as_bytes())?;
        self.lines += 1;
        // Arquivo só cresce por append; regrava quando passa de 1,5× o limite.
        if self.lines > self.max + self.max / 2 { self.compact()?; }
        Ok(())
    }

    fn compact(&mut self) -> std::io::Result<()> {
        let tmp = self.path.with_extension("jsonl.tmp");
        let body: String = self.entries.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect();
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)?;
        self.lines = self.entries.len();
        Ok(())
    }

    /// Grava um comando sem pasta (atalho dos testes); devolve o id.
    #[cfg(test)]
    pub fn add(&mut self, ts: u64, cmd: &str, kind: &str, label: &str, pc: &str) -> std::io::Result<u64> { self.add_in(ts, cmd, kind, label, pc, None) }

    /// Como `add`, com a pasta da aba (`None` se o shell não anunciou).
    pub fn add_in(&mut self, ts: u64, cmd: &str, kind: &str, label: &str, pc: &str, cwd: Option<&str>) -> std::io::Result<u64> {
        let id = self.entries.last().map_or(ts, |l| ts.max(l.id + 1));
        let cwd = cwd.map(|c| c.chars().take(1024).collect());
        let e = Entry { id, ts, cmd: cmd.chars().take(MAX_CMD).collect(), kind: kind.into(), label: label.into(), pc: pc.into(), code: None, cwd };
        let line = serde_json::to_string(&e).unwrap();
        self.entries.push(e);
        let cut = self.entries.len().saturating_sub(self.max);
        self.entries.drain(..cut);
        self.append(&line)?;
        Ok(id)
    }

    pub fn set_code(&mut self, id: u64, code: i64) -> std::io::Result<()> {
        let Some(e) = self.entries.iter_mut().rev().find(|e| e.id == id) else { return Ok(()) };
        e.code = Some(code);
        self.append(&format!("{{\"id\":{id},\"code\":{code}}}"))
    }

    pub fn clear(&mut self) -> std::io::Result<()> {
        self.entries.clear();
        self.lines = 0;
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Mais recentes primeiro. Busca sem diferenciar maiúsculas no comando, rótulo e PC.
    pub fn search(&self, q: &Query, now: u64) -> Vec<Entry> {
        let needle = q.q.trim().to_lowercase();
        let since = q.days.map(|d| now.saturating_sub(d as u64 * 86_400_000));
        self.entries.iter().rev()
            .filter(|e| since.is_none_or(|s| e.ts >= s))
            .filter(|e| q.kind.as_deref().is_none_or(|k| k.is_empty() || e.kind == k))
            .filter(|e| !q.errors || e.code.is_some_and(|c| c != 0))
            .filter(|e| needle.is_empty() || [&e.cmd, &e.label, &e.pc].iter().any(|s| s.to_lowercase().contains(&needle)))
            .take(q.limit.unwrap_or(1000).min(MAX))
            .cloned().collect()
    }
}

/// Tipo de sessão pelo rótulo da aba. `ia` (agente) não é gravado: lá se digita prompt, não comando.
pub fn kind_of(label: &str) -> &'static str {
    let l = label.to_lowercase();
    let has = |w: &str| l.split(|c: char| !c.is_alphanumeric() && c != '-').any(|t| t == w);
    if has("ssh") { "ssh" }
    else if [" · claude", " · openclaude", " · codex"].iter().any(|s| l.ends_with(s)) { "ia" }
    else if has("pwsh") || l.contains("powershell") { "pwsh" }
    else if has("cmd") || l.contains("prompt de comando") { "cmd" }
    else if has("wsl") || has("bash") || has("ubuntu") { "wsl" }
    else { "outro" }
}

/// Linha lógica onde está o cursor (junta as linhas quebradas pela largura).
pub fn cursor_line(vt: &vt100::Parser) -> String {
    let s = vt.screen();
    let cols = s.size().1.get();
    let mut r = s.cursor_position().0;
    let mut text = s.rows(0, cols).nth(r as usize).unwrap_or_default();
    while r > 0 && s.row_wrapped(r - 1) {
        r -= 1;
        text = s.rows(0, cols).nth(r as usize).unwrap_or_default() + &text;
    }
    text.trim_end().to_owned()
}

fn is_secret(line: &str) -> bool {
    let l = line.to_lowercase();
    SECRET.iter().any(|w| l.contains(w))
}

/// Tira o prompt: tudo até o primeiro fim de prompt conhecido (a linha da tela vem sem o espaço final).
fn strip_prompt(line: &str) -> String {
    let l = format!("{line} ");
    PROMPT_END.iter().filter_map(|p| l.find(p).map(|i| i + p.len())).min().map_or(&l[..], |i| &l[i..]).trim().to_owned()
}

/// Digitação desde o último Enter: `dirty` quando houve seta, Tab ou outro controle (texto não confere mais).
#[derive(Default, Debug)]
pub struct Typed { text: String, dirty: bool }

/// Comando enviado por um `term_input`. `line` = linha do cursor antes do envio; `None` se tela alternativa (vim, less…).
/// Devolve os comandos a gravar (vazio se nada, prompt de senha ou sem eco).
pub fn on_input(t: &mut Typed, data: &str, line: Option<&str>) -> Vec<String> {
    let mut out = vec![];
    let mut now = String::new();
    let mut first = true;
    let mut esc = false;
    for c in data.chars() {
        match c {
            '\r' => {
                let cmd = if first { line.and_then(|l| from_line(t, l, &now)) } else { (!now.trim().is_empty()).then(|| now.trim().to_owned()) };
                out.extend(cmd);
                first = false;
                *t = Typed::default();
                now.clear();
            }
            '\x7f' | '\x08' => { if now.pop().is_none() { t.text.pop(); } }
            '\x03' | '\x15' => { *t = Typed::default(); now.clear(); }
            '\x1b' => { t.dirty = true; esc = true; }
            c if (c as u32) < 0x20 => t.dirty = true,
            // Corpo de sequência de escape (ex.: `[A`) não é texto.
            c if esc => { if c.is_ascii_alphabetic() || c == '~' { esc = false; } }
            c => now.push(c),
        }
    }
    t.text.push_str(&now);
    out
}

fn from_line(t: &Typed, line: &str, now: &str) -> Option<String> {
    if is_secret(line) { return None; }
    let typed = t.text.as_str();
    // Digitou e nada apareceu na linha: terminal sem eco (senha).
    if !t.dirty && !typed.trim().is_empty() && !line.contains(typed.trim()) { return None; }
    let cmd = if !t.dirty && !typed.trim().is_empty() && line.ends_with(typed.trim_end()) { format!("{typed}{now}") } else { format!("{}{now}", strip_prompt(line)) };
    let cmd = cmd.trim();
    (!cmd.is_empty()).then(|| cmd.to_owned())
}

// ---------- Estado no app ----------

#[derive(Default)]
struct Meta { kind: String, label: String, pc: String, cwd: Option<String> }

#[derive(Default)]
pub struct History {
    /// Também entregue ao provedor do celular (`mobile.rs`).
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

/// Chamado por `term_input` (term.rs). `line`: linha do cursor se `data` tem Enter e a tela é a normal.
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
pub fn hist_clear(app: AppHandle) -> Result<(), String> {
    app.state::<History>().pending.lock().unwrap().clear();
    with_store(&app, |s| s.clear()).map_err(|_| "IO_ERROR".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("live-cmd-hist-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join(FILE)
    }

    #[test]
    fn grava_le_de_novo_e_aplica_codigo() {
        let p = tmp("grava");
        let mut s = Store::open(p.clone(), 10);
        let a = s.add(1000, "git status", "pwsh", "PowerShell", "Este PC").unwrap();
        let b = s.add(1000, "ls", "ssh", "ssh srv", "Este PC").unwrap();
        assert!(b > a, "id único mesmo na mesma hora");
        s.set_code(b, 2).unwrap();
        let s = Store::open(p.clone(), 10);
        assert_eq!(s.entries().iter().map(|e| (e.cmd.as_str(), e.code)).collect::<Vec<_>>(), vec![("git status", None), ("ls", Some(2))]);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn grava_pasta_e_store_compartilhado() {
        let p = tmp("pasta");
        let shared = Shared::default();
        let dir = p.parent().unwrap().to_path_buf();
        with_shared(&shared, &dir, |s| s.add_in(1, "ls", "pwsh", "PowerShell", "Este PC", Some(r"C:\proj"))).unwrap();
        assert_eq!(Store::open(p.clone(), 10).entries()[0].cwd.as_deref(), Some(r"C:\proj"), "pasta volta do disco");
        // Quem limpa pelo store compartilhado não deixa cópia velha em memória do outro lado.
        with_shared(&shared, &dir, |s| s.clear()).unwrap();
        assert!(with_shared(&shared, &dir, |s| s.search(&Query::default(), 10)).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn limite_tira_os_mais_antigos_e_compacta_o_arquivo() {
        let p = tmp("limite");
        let mut s = Store::open(p.clone(), 4);
        for i in 0..20 { s.add(i, &format!("c{i}"), "pwsh", "PowerShell", "Este PC").unwrap(); }
        assert_eq!(s.entries().len(), 4);
        assert_eq!(s.entries()[0].cmd, "c16");
        let lines = std::fs::read_to_string(&p).unwrap().lines().count();
        assert!(lines <= 6, "arquivo compactado: {lines} linhas");
        let s = Store::open(p.clone(), 4);
        assert_eq!(s.entries().iter().map(|e| e.cmd.as_str()).collect::<Vec<_>>(), vec!["c16", "c17", "c18", "c19"]);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn busca_e_filtros() {
        let p = tmp("busca");
        let day = 86_400_000;
        let now = 30 * day;
        let mut s = Store::open(p.clone(), 100);
        s.add(now - 10 * day, "npm run build", "pwsh", "PowerShell", "Este PC").unwrap();
        let x = s.add(now - day, "Git Push", "pwsh", "PowerShell", "Este PC").unwrap();
        s.add(now - 1000, "uptime", "ssh", "ssh deploy@srv", "Notebook").unwrap();
        s.set_code(x, 1).unwrap();
        let q = |q: &str, days, kind: Option<&str>, errors| s.search(&Query { q: q.into(), days, kind: kind.map(Into::into), errors, limit: None }, now)
            .into_iter().map(|e| e.cmd).collect::<Vec<_>>();
        assert_eq!(q("", None, None, false), vec!["uptime", "Git Push", "npm run build"], "mais recente primeiro");
        assert_eq!(q("git", None, None, false), vec!["Git Push"], "sem diferenciar maiúsculas");
        assert_eq!(q("notebook", None, None, false), vec!["uptime"], "busca no PC também");
        assert_eq!(q("", Some(7), None, false), vec!["uptime", "Git Push"]);
        assert_eq!(q("", None, Some("ssh"), false), vec!["uptime"]);
        assert_eq!(q("", None, None, true), vec!["Git Push"]);
        let mut s = s;
        s.clear().unwrap();
        assert!(s.entries().is_empty() && !p.exists());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn linha_corrompida_e_pulada() {
        let p = tmp("corrompida");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{\"id\":1,\"ts\":1,\"cmd\":\"a\",\"kind\":\"pwsh\",\"label\":\"P\",\"pc\":\"E\"}\nlixo\n").unwrap();
        assert_eq!(Store::open(p.clone(), 10).entries().len(), 1);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    fn typed(keys: &[&str], line: Option<&str>) -> Vec<String> {
        let mut t = Typed::default();
        let (last, rest) = keys.split_last().unwrap();
        for k in rest { assert!(on_input(&mut t, k, None).is_empty()); }
        on_input(&mut t, last, line)
    }

    #[test]
    fn captura_comando_da_linha() {
        assert_eq!(typed(&["g", "i", "t", " ", "s", "\r"], Some("PS C:\\x> git s")), vec!["git s"]);
        // Seta para cima (histórico do shell): texto vem da tela, sem o prompt.
        assert_eq!(typed(&["\x1b[A", "\r"], Some("PS C:\\x> echo a > b")), vec!["echo a > b"]);
        assert_eq!(typed(&["\r"], Some("yago@srv:~$ ls -la")), vec!["ls -la"]);
        // Reexecutar: comando inteiro no mesmo envio.
        assert_eq!(typed(&["dir\r"], Some("C:\\Users>")), vec!["dir"]);
        assert_eq!(typed(&["a", "b", "\x7f", "c", "\r"], Some("$ ac")), vec!["ac"]);
        assert!(typed(&["\r"], Some("PS C:\\x>")).is_empty(), "Enter vazio não grava");
        assert!(typed(&["\r"], Some("yago@srv:~$")).is_empty());
        assert!(typed(&["v", "i", "\r"], None).is_empty(), "tela alternativa");
    }

    #[test]
    fn nao_grava_senha() {
        assert!(typed(&["s", "3", "c", "\r"], Some("yago@srv's password:")).is_empty());
        assert!(typed(&["x", "\r"], Some("Digite a senha:")).is_empty());
        assert!(typed(&["x", "\r"], Some("Enter passphrase for key 'id':")).is_empty());
        // Sem eco: o que foi digitado não aparece na linha.
        assert!(typed(&["h", "u", "n", "t", "e", "r", "\r"], Some("Token:")).is_empty());
    }

    #[test]
    fn tipo_pelo_rotulo() {
        assert_eq!(kind_of("PowerShell"), "pwsh");
        assert_eq!(kind_of("ssh deploy@srv"), "ssh");
        assert_eq!(kind_of("SSH · prod"), "ssh");
        assert_eq!(kind_of("Prompt de Comando"), "cmd");
        assert_eq!(kind_of("Ubuntu"), "wsl");
        assert_eq!(kind_of("app · claude"), "ia");
        assert_eq!(kind_of("build"), "outro");
    }

    #[test]
    fn linha_do_cursor_junta_quebra() {
        let mut vt = vt100::Parser::new(std::num::NonZeroU16::new(3).unwrap(), std::num::NonZeroU16::new(10).unwrap(), 0);
        vt.process(b"out\r\n$ echo 12345678");
        assert_eq!(cursor_line(&vt), "$ echo 12345678");
    }
}

//! D3d: cópia da tela no host (`vt100`) para `screen_snapshot` (C3 §5, D9) e
//! integração de shell OSC 133 → `command_completed`, OSC 7 → `cwd_changed` (§9 item 6).
//! O OSC é lido pelo parser VT (estado entre pedaços), nunca por regex na saída.
#![allow(dead_code)] // ligado ao pty-channel em D3c
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde_json::{json, Value};
use std::num::NonZeroU16;

/// Prompt do perfil pwsh: emite fim do comando anterior (a partir do 2º prompt) e a pasta atual.
pub const PWSH_PROMPT: &str = r#"function prompt { $c = if ($?) { 0 } elseif ($LASTEXITCODE) { $LASTEXITCODE } else { 1 }; $o = ''; if ($global:__lc) { $o = "`e]133;D;$c`a" }; $global:__lc = 1; "$o`e]7;$(([uri]$PWD.ProviderPath).AbsoluteUri)`aPS $($PWD.ProviderPath)> " }"#;
/// `PROMPT_COMMAND` do perfil WSL (bash). `$PWD` vai percent-encoded byte a byte (subshell com `LC_ALL=C`).
pub const BASH_PROMPT_COMMAND: &str = r#"__lc=$?; [ -n "$__lc_s" ] && printf '\e]133;D;%s\a' "$__lc"; __lc_s=1; (LC_ALL=C; s=$PWD o=; for ((i=0;i<${#s};i++)); do c=${s:i:1}; case $c in [a-zA-Z0-9/._~-]) o+=$c;; *) printf -v c '%%%02X' "'$c"; o+=$c;; esac; done; printf '\e]7;file://%s%s\a' "$HOSTNAME" "$o")"#;
/// OSC maior que isto é descartado sem chegar ao parser (o `vte` com `std` acumula sem limite).
pub const OSC_MAX: usize = 4096;
/// Teto dos bytes do snapshot: base64 (×4/3) + envelope cabe em 65536 B (C3 §2).
pub const SNAP_MAX: usize = 48_000;

#[derive(Debug, PartialEq)]
pub enum Event { CommandCompleted(Option<i32>), Cwd(String) }

struct Osc { on: bool, skip: bool, ev: Vec<Event> }

impl vt100::Callbacks for Osc {
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, p: &[&[u8]]) {
        if !self.on || self.skip { return; }
        match p {
            [b"133", b"D"] => self.ev.push(Event::CommandCompleted(None)),
            [b"133", b"D", c, ..] => if let Some(c) = std::str::from_utf8(c).ok().and_then(|c| c.parse().ok()) {
                self.ev.push(Event::CommandCompleted(Some(c)))
            },
            [b"7", rest @ ..] if !rest.is_empty() => if let Some(c) = cwd(&rest.join(&b';')) { self.ev.push(Event::Cwd(c)) },
            _ => {}
        }
    }
}

/// `file://host/path` → caminho. `/C:/x` vira `C:\x`. Percent-decode; UTF-8 inválido ou sem `file://` → ignora.
fn cwd(u: &[u8]) -> Option<String> {
    let rest = u.strip_prefix(b"file://")?;
    let path = &rest[rest.iter().position(|&b| b == b'/')?..];
    let mut out = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        let h = |b: u8| (b as char).to_digit(16);
        match (path[i], path.get(i + 1).and_then(|&b| h(b)), path.get(i + 2).and_then(|&b| h(b))) {
            (b'%', Some(a), Some(b)) => { out.push((a * 16 + b) as u8); i += 3; }
            (b, ..) => { out.push(b); i += 1; }
        }
    }
    let s = String::from_utf8(out).ok()?;
    let b = s.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' { Some(s[1..].replace('/', "\\")) } else { Some(s) }
}

/// Tela de uma sessão. `osc = false` (perfil `cmd`): sem eventos; o host anuncia cwd `null`.
/// `esc`/`osc`: pré-filtro que corta OSC > `OSC_MAX` (`Some(n)` = n bytes lidos; `OSC_MAX + 1` = descartando).
pub struct Screen { vt: vt100::Parser<Osc>, esc: bool, osc: Option<usize> }

/// Estado de um grid que o `vt100` não expõe: cursor salvo (DECSC), região de rolagem (DECSTBM), DECOM.
#[derive(Debug, PartialEq, Clone, Copy)]
struct G { saved: (u16, u16), top: u16, bottom: u16, origin: bool, saved_origin: bool }

impl G {
    fn new(rows: u16) -> Self { G { saved: (0, 0), top: 0, bottom: rows - 1, origin: false, saved_origin: false } }
    fn parse(t: &str) -> Option<Self> {
        let num = |t: &str, k: &str| -> Option<u16> { let r = &t[t.find(k)? + k.len()..]; r[..r.find(|c: char| !c.is_ascii_digit())?].parse().ok() };
        let flag = |k: &str| t.find(k).map(|i| t[i + k.len()..].starts_with("true"));
        let sp = &t[t.find(" saved_pos: ")?..];
        Some(G { saved: (num(sp, "row: ")?, num(sp, "col: ")?), top: num(t, "scroll_top: ")?, bottom: num(t, "scroll_bottom: ")?,
                 origin: flag(" origin_mode: ")?, saved_origin: flag("saved_origin_mode: ")? })
    }
    /// Região, depois cursor salvo (com o DECOM salvo) já posicionado; quem chama emite DECSC ou `?1049h`.
    fn prep(&self, o: &mut String, s: &vt100::Screen, txt: bool) {
        let rows = s.size().0.get();
        if (self.top, self.bottom) != (0, rows - 1) { o.push_str(&format!("\x1b[{};{}r", self.top + 1, self.bottom + 1)); }
        if self.saved_origin { o.push_str("\x1b[?6h"); }
        at(o, s, txt, self.saved);
    }
}

/// Cursor em (linha, coluna) absolutas: VPA+CHA ignoram DECOM e região. Coluna == `cols` (quebra pendente) só se
/// alcança imprimindo a última célula: reimprime a de `s` se `txt`; senão (principal sob a alternativa, texto já perdido) espaço.
/// ponytail: última célula vazia com quebra pendente (impressa e depois apagada) fica em `cols - 1`.
fn at(o: &mut String, s: &vt100::Screen, txt: bool, (r, c): (u16, u16)) {
    let cols = s.size().1.get();
    if c < cols { return o.push_str(&format!("\x1b[{}d\x1b[{}G", r + 1, c + 1)); }
    let st = if txt && s.cell(r, cols - 1).is_some_and(|x| x.is_wide_continuation()) { cols - 2 } else { cols - 1 };
    let cell = if txt { s.rows_formatted(st, cols - st).nth(r.into()).unwrap_or_default() } else { " ".into() };
    o.push_str(&format!("\x1b[{}d\x1b[{}G\x1b[m{cell}\x1b[m", r + 1, st + 1));
}

/// (grid principal, grid alternativo) lidos do `Debug` do `vt100`, pulando as linhas (só números, sem texto do usuário).
/// ponytail: depende do layout do `Debug` do vt100 0.18 (o teste `same` quebra se mudar); custa ~O(células) por snapshot,
/// que só ocorre após `replay_gap`. Upgrade: getters no vt100 (PR/fork).
fn grids(s: &vt100::Screen) -> (G, G) {
    struct Sink { out: String, depth: u32 }
    impl std::fmt::Write for Sink {
        fn write_str(&mut self, t: &str) -> std::fmt::Result {
            for c in t.chars() {
                if self.depth > 0 { match c { '[' => self.depth += 1, ']' => self.depth -= 1, _ => {} } }
                else if c == '[' && self.out.ends_with("rows: ") { self.depth = 1 }
                else { self.out.push(c) }
            }
            Ok(())
        }
    }
    let mut k = Sink { out: String::new(), depth: 0 };
    let _ = std::fmt::Write::write_fmt(&mut k, format_args!("{s:?}"));
    let d = G::new(s.size().0.get());
    let (a, b) = k.out.split_once("alternate_grid:").unwrap_or((&k.out, ""));
    (G::parse(a).unwrap_or(d), G::parse(b).unwrap_or(d))
}

impl Screen {
    pub fn new(cols: u16, rows: u16, osc: bool) -> Self {
        Screen { vt: vt100::Parser::new_with_callbacks(nz(rows), nz(cols), 0, Osc { on: osc, skip: false, ev: vec![] }), esc: false, osc: None }
    }
    /// Alimenta a saída do PTY; devolve os eventos completados neste pedaço.
    pub fn feed(&mut self, b: &[u8]) -> Vec<Event> {
        let mut from = 0;
        for (i, &c) in b.iter().enumerate() {
            match self.osc {
                None => if self.esc && c == b']' { self.osc = Some(0) },
                Some(n) if matches!(c, 0x07 | 0x18 | 0x1A | 0x1B) => {
                    if n > OSC_MAX { from = if c == 0x1B { i } else { i + 1 } }
                    self.osc = None;
                }
                Some(n) if n > OSC_MAX => from = i + 1,
                Some(n) if n == OSC_MAX => {
                    // Corta o OSC com CAN (o vte despacha o pedaço sob `skip`) e descarta o resto até o terminador.
                    self.vt.process(&b[from..i]);
                    self.vt.callbacks_mut().skip = true;
                    self.vt.process(b"\x18");
                    self.vt.callbacks_mut().skip = false;
                    from = i + 1;
                    self.osc = Some(n + 1);
                }
                Some(n) => self.osc = Some(n + 1),
            }
            self.esc = c == 0x1B;
        }
        self.vt.process(&b[from..]);
        std::mem::take(&mut self.vt.callbacks_mut().ev)
    }
    pub fn resize(&mut self, cols: u16, rows: u16) { self.vt.set_size(nz(rows), nz(cols)); }
    /// Bytes ANSI (≤ `SNAP_MAX`) que reproduzem a tela atual num emulador limpo do mesmo tamanho, com região de rolagem,
    /// DECOM e cursor salvo. Se a tela formatada não couber, vai só o texto das linhas de baixo que couberem.
    /// ponytail: na tela alternativa (vim) só ela é reproduzida; a tela principal fica vazia ao sair. Ver E7.
    /// ponytail: atributos salvos pelo DECSC (cor ao restaurar com ESC 8) não são reproduzidos.
    pub fn snapshot(&self) -> Vec<u8> {
        let s = self.vt.screen();
        let rows = s.size().0.get();
        let (main, alt) = grids(s);
        let mut pre = String::new();
        if s.alternate_screen() {
            if main != G::new(rows) { main.prep(&mut pre, s, false); }
            pre.push_str("\x1b[?1049h");
        }
        let g = if s.alternate_screen() { alt } else { main };
        let mut post = String::new();
        // Sempre: o `cursor_state_formatted` do vt100 usa ESC 7 na quebra pendente e estragaria o cursor salvo.
        g.prep(&mut post, s, true);
        post.push_str("\x1b7");
        if g.origin != g.saved_origin { post.push_str(if g.origin { "\x1b[?6h" } else { "\x1b[?6l" }); }
        at(&mut post, s, true, s.cursor_position());
        post.push_str(&s.attributes_formatted());
        let budget = SNAP_MAX.saturating_sub(pre.len() + post.len());
        let full = s.state_formatted();
        let body = if full.len() <= budget { full } else {
            let tail = format!("{}{}{}", s.input_mode_formatted(), s.cursor_state_formatted(), s.attributes_formatted());
            let mut left = budget.saturating_sub(tail.len() + 6);
            let mut lines = vec![];
            for (i, r) in s.rows(0, s.size().1.get()).enumerate().collect::<Vec<_>>().into_iter().rev() {
                if r.is_empty() { continue; }
                let l = format!("\x1b[{}H{r}", i + 1);
                if l.len() > left { break; }
                left -= l.len();
                lines.push(l);
            }
            lines.reverse();
            format!("\x1b[m\x1b[H\x1b[J{}{tail}", lines.concat())
        };
        format!("{pre}{body}{post}").into_bytes()
    }
    /// Corpo do controle `screen_snapshot` (para `proto::encode_control`); `offset` = `available_from` do replay.
    pub fn snapshot_body(&self, session_id: &[u8; 16], offset: u64) -> Value {
        let (rows, cols) = self.vt.screen().size();
        json!({"type":"screen_snapshot","session_id":crate::proto::uuid_str(session_id),"offset":crate::proto::u64s(offset),
               "cols":cols.get(),"rows":rows.get(),"data":B64.encode(self.snapshot())})
    }
}

fn nz(n: u16) -> NonZeroU16 { NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN) }

pub fn event_body(session_id: &[u8; 16], e: &Event) -> Value {
    let sid = crate::proto::uuid_str(session_id);
    match e {
        Event::CommandCompleted(Some(c)) => json!({"type":"command_completed","session_id":sid,"exit_code":c}),
        Event::CommandCompleted(None) => json!({"type":"command_completed","session_id":sid}),
        Event::Cwd(c) => json!({"type":"cwd_changed","session_id":sid,"cwd":c}),
    }
}
/// Perfil sem integração (`cmd`): anunciado uma vez no attach.
pub fn cwd_unknown(session_id: &[u8; 16]) -> Value {
    json!({"type":"cwd_changed","session_id":crate::proto::uuid_str(session_id),"cwd":null})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &Screen, b: &vt100::Parser) {
        let (x, y) = (a.vt.screen(), b.screen());
        assert_eq!(x.contents_formatted(), y.contents_formatted());
        assert_eq!(x.cursor_position(), y.cursor_position());
        assert_eq!(x.alternate_screen(), y.alternate_screen());
        assert_eq!(x.hide_cursor(), y.hide_cursor());
        assert_eq!(x.input_mode_formatted(), y.input_mode_formatted());
        assert_eq!(x.attributes_formatted(), y.attributes_formatted());
        assert_eq!(grids(x), grids(y), "região/DECOM/cursor salvo");
    }
    fn snap(s: &Screen) -> vt100::Parser {
        let (rows, cols) = s.vt.screen().size();
        let body = s.snapshot_body(&[7; 16], 42);
        assert!(crate::proto::encode_control(&"c".repeat(36), None, body.clone()).len() <= 65536);
        let mut c = vt100::Parser::new(rows, cols, 0);
        c.process(&B64.decode(body["data"].as_str().unwrap()).unwrap());
        c
    }
    fn round_trip(input: &[u8], cols: u16, rows: u16) -> Screen {
        let mut s = Screen::new(cols, rows, true);
        s.feed(input);
        let body = s.snapshot_body(&[7; 16], 42);
        assert_eq!((body["cols"].as_u64(), body["rows"].as_u64(), body["offset"].as_str()), (Some(cols as u64), Some(rows as u64), Some("42")));
        same(&s, &snap(&s));
        s
    }

    #[test]
    fn grids_le_estado_interno() {
        let mut s = Screen::new(80, 24, true);
        assert_eq!(grids(s.vt.screen()), (G::new(24), G::new(24)));
        s.feed(b"\x1b[3;10r\x1b[?6h\x1b[2;5H\x1b7\x1b[?6l\x1b[20;1H");
        let g = grids(s.vt.screen()).0;
        assert_eq!(g, G { saved: (3, 4), top: 2, bottom: 9, origin: false, saved_origin: true });
        // texto do usuário com os nomes dos campos não engana o parser
        s.feed(b"scroll_top: 99 saved_pos: Pos { row: 7, col: 7 } origin_mode: true alternate_grid:");
        assert_eq!(grids(s.vt.screen()).0, g);
    }

    #[test]
    fn snapshot_restaura_regiao_decom_e_cursor_salvo() {
        // região + cursor salvo + DECOM ligado, na tela principal
        let s = round_trip(b"abc\x1b[5;15r\x1b[3;7H\x1b7\x1b[?6h\x1b[2;3Hx", 40, 20);
        let mut c = snap(&s);
        let mut s2 = Screen::new(40, 20, true);
        s2.feed(b"abc\x1b[5;15r\x1b[3;7H\x1b7\x1b[?6h\x1b[2;3Hx");
        for p in [&b"\x1b8Y"[..], b"\x1b[30;1H\n\n\nZ", b"\x1b[HW"] { c.process(p); s2.feed(p); }
        same(&s2, &c);
        // vim: região na tela alternativa, cursor salvo pelo ?1049h na principal
        let s = round_trip(b"$ ls\r\n\x1b[2;9r\x1b[?1049h\x1b[3;20r\x1b[10;4Hv\x1b[?6h\x1b[1;1Hq", 40, 20);
        let mut c = snap(&s);
        let mut s2 = Screen::new(40, 20, true);
        s2.feed(b"$ ls\r\n\x1b[2;9r\x1b[?1049h\x1b[3;20r\x1b[10;4Hv\x1b[?6h\x1b[1;1Hq");
        // ponytail conhecido: o texto da tela principal não volta; região, cursor salvo e DECOM voltam
        for p in [&b"\x1b[?1049l"[..], b"\x1b[40;1H\nL\x1b[1;1H\nM"] {
            c.process(p); s2.feed(p);
            assert_eq!(grids(c.screen()), grids(s2.vt.screen()));
            assert_eq!(c.screen().cursor_position(), s2.vt.screen().cursor_position());
        }
    }

    #[test]
    fn snapshot_quebra_pendente_e_decom_fora_da_regiao() {
        // cursor e cursor salvo na quebra pendente (última coluna impressa), inclusive com caractere largo
        round_trip(b"\x1b[3;5r\x1b[1;9Habcd\x1b7\x1b[8;1H", 12, 8);
        round_trip(b"\x1b[3;5r\x1b[1;11H\xe6\x97\xa5\x1b7\x1b[2;1H\x1b[31mxyzxyzxyzxyz", 12, 8);
        // DECOM ligado com cursor e cursor salvo fora da região
        round_trip(b"\x1b[3;5r\x1b[8;4H\x1b[?6h\x1b7\x1b[1;1H", 12, 8);
        // cursor salvo padrão + quebra pendente sobre célula apagada: o ESC 7 interno do vt100 não pode vazar
        let mut s = Screen::new(5, 3, true);
        s.feed(b"\x1b7abcde\x1b[1K");
        assert_eq!(grids(snap(&s).screen()), grids(s.vt.screen()));
        // round-trip aleatório (LCG fixo) misturando região, DECOM, DECSC/DECRC, cursor e texto
        let ops: [&[u8]; 16] = [b"\x1b[3;6r", b"\x1b[2;4r", b"\x1b[r", b"\x1b[?6h", b"\x1b[?6l", b"\x1b7", b"\x1b8", b"\x1b[1;12H",
                                b"\x1b[8;1H", b"\x1b[2;5H", b"ab", b"\xe6\x97\xa5", b"\r\n", b"\x1b[?1049h", b"\x1b[1K", b"\x1b[K"];
        let mut x = 1u64;
        for _ in 0..3000 {
            let mut seq = vec![];
            for _ in 0..12 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                seq.extend_from_slice(ops[(x >> 33) as usize % ops.len()]);
            }
            let mut s = Screen::new(12, 8, true);
            s.feed(&seq);
            let c = snap(&s);
            if s.vt.screen().alternate_screen() {
                // ponytail conhecido: texto da principal se perde; o resto do grid ativo tem de bater
                assert_eq!(grids(c.screen()).1, grids(s.vt.screen()).1, "{seq:?}");
                assert_eq!(c.screen().cursor_position(), s.vt.screen().cursor_position(), "{seq:?}");
            } else if { let v = s.vt.screen(); let (r, col) = v.cursor_position(); col == 12 && !v.cell(r, 11).is_some_and(|x| x.has_contents()) } {
                // ponytail conhecido (`at`): quebra pendente sobre célula vazia; o resto tem de bater
                assert_eq!(grids(c.screen()), grids(s.vt.screen()), "{seq:?}");
                assert_eq!(c.screen().contents(), s.vt.screen().contents(), "{seq:?}");
            } else {
                same(&s, &c);
            }
        }
    }

    #[test]
    fn snapshot_cabe_na_mensagem() {
        // 200×60 com cor por célula: não cabe formatado; vai o texto inteiro
        let cor: Vec<u8> = (0..60u32).flat_map(|r| (0..200u32).flat_map(move |c| format!("\x1b[38;2;{};{};{}m\x1b[48;5;{}m{}", c, r, c ^ r, (c + r) % 256, (b'a' + ((c + r) % 26) as u8) as char).into_bytes())).collect();
        let mut s = Screen::new(200, 60, true);
        s.feed(&[&cor[..], b"\x1b[?1h\x1b[3;3H"].concat());
        assert!(s.vt.screen().state_formatted().len() > SNAP_MAX);
        let c = snap(&s);
        let rows = |p: &vt100::Screen| p.rows(0, 200).collect::<Vec<_>>();
        assert_eq!(rows(c.screen()), rows(s.vt.screen()));
        assert_eq!((c.screen().cursor_position(), c.screen().application_cursor()), ((2, 2), true));
        // 1000×1000 cheio de largos: só as linhas de baixo, ainda dentro do limite
        let mut s = Screen::new(1000, 1000, true);
        s.feed(&"\x1b[1;32m日".repeat(500_000).into_bytes());
        s.feed(b"\x1b[1000;1H\x1b[2Kfim");
        let c = snap(&s);
        assert!(c.screen().contents().ends_with("\nfim"));
        assert_eq!(c.screen().cursor_position(), s.vt.screen().cursor_position());
    }

    #[test]
    fn osc_gigante_descartado() {
        let mut s = Screen::new(80, 24, true);
        let big = [&b"\x1b]7;file:///"[..], &vec![b'a'; 50 << 20], b"\x07ok\x1b]133;D;3\x07"].concat();
        let mut ev = vec![];
        for p in big.chunks(65536) { ev.extend(s.feed(p)); }
        assert_eq!(ev, vec![Event::CommandCompleted(Some(3))]);
        assert_eq!(s.vt.screen().contents(), "ok");
        // exatamente no limite ainda vale; terminado por ESC \ também
        let p = format!("/{}", "b".repeat(OSC_MAX - "7;file://".len() - 1));
        assert_eq!(s.feed(format!("\x1b]7;file://{p}\x1b\\").as_bytes()), vec![Event::Cwd(p.clone())]);
        assert!(s.feed(format!("\x1b]7;file://{p}c\x1b\\z").as_bytes()).is_empty());
        assert_eq!(s.vt.screen().contents(), "okz");
    }

    #[test]
    #[ignore = "precisa de bash (LIVE_CMD_BASH=caminho)"]
    fn bash_codifica_pwd() {
        let d = std::env::temp_dir().join("lc João a%b;c'd");
        std::fs::create_dir_all(&d).unwrap();
        let o = std::process::Command::new(std::env::var("LIVE_CMD_BASH").unwrap_or("bash".into())).current_dir(&d).env("HOSTNAME", "h").env("LANG", "C.UTF-8")
            .args(["-c", BASH_PROMPT_COMMAND]).output().unwrap();
        let mut s = Screen::new(80, 24, true);
        let ev = s.feed(&o.stdout);
        assert!(String::from_utf8_lossy(&o.stdout).contains("Jo%C3%A3o%20a%25b%3Bc%27d"), "{:?}", String::from_utf8_lossy(&o.stdout));
        assert!(matches!(&ev[..], [Event::Cwd(c)] if c.ends_with("lc João a%b;c'd")), "{ev:?}");
    }

    #[test]
    fn snapshot_reproduz_tela() {
        round_trip(b"PS C:\\> dir\r\n\x1b[1;31merro\x1b[0m \x1b[38;5;208mlaranja\x1b[48;2;1;2;3m rgb\x1b[0m\r\n\xc3\xa7\xe6\x97\xa5\xf0\x9f\x98\x80", 40, 10);
        // tipo vim: tela alternativa, região de rolagem, cursor escondido, teclado de aplicação, cursor no meio
        round_trip(b"shell$ \x1b[?1049h\x1b[?1h\x1b=\x1b[?25l\x1b[2J\x1b[1;20r\x1b[H~\r\n~\r\n\x1b[7m arquivo.txt \x1b[0m\x1b[5;3HX\x1b[?2004h", 80, 24);
        // rolagem além da altura
        round_trip(&(0..50).flat_map(|i| format!("linha {i}\r\n").into_bytes()).collect::<Vec<_>>(), 20, 5);
        round_trip(b"", 1, 1);
    }

    #[test]
    fn resize_muda_tamanho_do_snapshot() {
        let mut s = Screen::new(80, 24, true);
        s.feed(b"abc");
        s.resize(100, 30);
        let b = s.snapshot_body(&[0; 16], 0);
        assert_eq!((b["cols"].as_u64(), b["rows"].as_u64()), (Some(100), Some(30)));
        round_trip(b"abc", 100, 30);
    }

    #[test]
    fn osc_133_e_7() {
        let mut s = Screen::new(80, 24, true);
        assert_eq!(s.feed(b"\x1b]133;D;0\x07x\x1b]133;D;-1\x1b\\\x1b]133;D\x07\x1b]133;A\x07"),
            vec![Event::CommandCompleted(Some(0)), Event::CommandCompleted(Some(-1)), Event::CommandCompleted(None)]);
        assert_eq!(s.feed(b"\x1b]7;file:///C:/Users/Jo%C3%A3o/a%20b\x07"), vec![Event::Cwd("C:\\Users\\João\\a b".into())]);
        assert_eq!(s.feed(b"\x1b]7;file://PC/home/u/x;y z\x07"), vec![Event::Cwd("/home/u/x;y z".into())]);
        assert_eq!(s.vt.screen().contents(), "x", "OSC não aparece na tela");
    }

    #[test]
    fn osc_partido_entre_pedacos() {
        let mut s = Screen::new(80, 24, true);
        let msg = b"a\x1b]133;D;127\x07b\x1b]7;file:///tmp/%41\x1b\\c";
        for i in 0..msg.len() {
            let (mut s2, mut ev) = (Screen::new(80, 24, true), vec![]);
            ev.extend(s2.feed(&msg[..i]));
            ev.extend(s2.feed(&msg[i..]));
            assert_eq!(ev, vec![Event::CommandCompleted(Some(127)), Event::Cwd("/tmp/A".into())], "corte em {i}");
        }
        for b in msg { s.feed(&[*b]); }
        assert_eq!(s.vt.screen().contents(), "abc");
    }

    #[test]
    fn osc_malformado_ignorado() {
        let mut s = Screen::new(80, 24, true);
        for m in [&b"\x1b]133;D;abc\x07"[..], b"\x1b]133;D;99999999999\x07", b"\x1b]7;http://x/y\x07", b"\x1b]7;file://semcaminho\x07",
                  b"\x1b]7;file:///%FF\x07", b"\x1b]7\x07", b"\x1b]133\x07", b"\x1b]1337;x\x07", b"\x1b]0;titulo\x07", b"\x1b]7;file:///a%2\x07"] {
            let ev = s.feed(m);
            if m == b"\x1b]7;file:///a%2\x07" { assert_eq!(ev, vec![Event::Cwd("/a%2".into())]); } else { assert!(ev.is_empty(), "{m:?}"); }
        }
    }

    #[test]
    fn cmd_sem_eventos() {
        let mut s = Screen::new(80, 24, false);
        assert!(s.feed(b"\x1b]133;D;0\x07\x1b]7;file:///C:/x\x07C:\\>").is_empty());
        assert_eq!(cwd_unknown(&[0; 16])["cwd"], Value::Null);
    }

    #[test]
    fn corpos_dos_eventos() {
        let sid = [0x11; 16];
        assert_eq!(event_body(&sid, &Event::CommandCompleted(None)), json!({"type":"command_completed","session_id":"11111111-1111-1111-1111-111111111111"}));
        assert_eq!(event_body(&sid, &Event::CommandCompleted(Some(2)))["exit_code"], 2);
        assert_eq!(event_body(&sid, &Event::Cwd("C:\\".into()))["cwd"], "C:\\");
        assert!(PWSH_PROMPT.contains("133;D") && BASH_PROMPT_COMMAND.contains("]7;file://"));
    }
}

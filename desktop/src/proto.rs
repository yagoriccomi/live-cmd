//! C3: frames binários (§3) e controles JSON (§2.1–2.3) do `pty-channel`.
#![allow(dead_code)]
use crate::auth::{uuid_v4_lower, Error::{self, *}};
use serde_json::{Map, Value};

type R<T> = Result<T, Error>;
pub const HEADER: usize = 30;
pub const MAX_IO: usize = 16384;
pub const MAX_FRAME: usize = HEADER + MAX_IO;
const MAX_MSG: usize = 65536;

#[derive(Debug, Clone, PartialEq)]
pub enum Op { Input(Vec<u8>), Output(Vec<u8>), Resize { cols: u16, rows: u16 }, Heartbeat, Exited(i32) }
#[derive(Debug, Clone, PartialEq)]
pub struct Frame { pub session_id: [u8; 16], pub seq: u64, pub op: Op }

fn dim(n: u16) -> R<u16> { if (1..=1000).contains(&n) { Ok(n) } else { Err(InvalidMessage) } }

pub fn parse_frame(b: &[u8]) -> R<Frame> {
    if b.len() < HEADER || b.len() > MAX_FRAME { return Err(InvalidMessage); }
    if b[0] != 1 { return Err(VersionUnsupported); }
    let session_id: [u8; 16] = b[2..18].try_into().unwrap();
    let seq = u64::from_be_bytes(b[18..26].try_into().unwrap());
    let len = u32::from_be_bytes(b[26..30].try_into().unwrap()) as usize;
    let p = &b[HEADER..];
    if len != p.len() { return Err(InvalidMessage); }
    let zero = session_id == [0; 16];
    let op = match (b[1], p.len()) {
        (1, 1..=MAX_IO) => Op::Input(p.to_vec()),
        (2, 1..=MAX_IO) => Op::Output(p.to_vec()),
        (3, 4) if seq == 0 => Op::Resize { cols: dim(u16::from_be_bytes([p[0], p[1]]))?, rows: dim(u16::from_be_bytes([p[2], p[3]]))? },
        (4, 0) if zero => return Ok(Frame { session_id, seq, op: Op::Heartbeat }),
        (5, 4) => Op::Exited(i32::from_be_bytes(p.try_into().unwrap())),
        _ => return Err(InvalidMessage),
    };
    if zero { return Err(InvalidMessage); }
    Ok(Frame { session_id, seq, op })
}

pub fn encode_frame(f: &Frame) -> Vec<u8> {
    let (code, p): (u8, Vec<u8>) = match &f.op {
        Op::Input(d) => (1, d.clone()),
        Op::Output(d) => (2, d.clone()),
        Op::Resize { cols, rows } => (3, [cols.to_be_bytes(), rows.to_be_bytes()].concat()),
        Op::Heartbeat => (4, vec![]),
        Op::Exited(c) => (5, c.to_be_bytes().to_vec()),
    };
    let mut b = Vec::with_capacity(HEADER + p.len());
    b.extend([1, code]);
    b.extend(f.session_id);
    b.extend(f.seq.to_be_bytes());
    b.extend((p.len() as u32).to_be_bytes());
    b.extend(p);
    b
}

/// u64 como string decimal (§2.1); número JSON é rejeitado (N10).
pub fn parse_u64(v: &Value) -> R<u64> {
    let s = v.as_str().ok_or(InvalidMessage)?;
    let canon = !s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if canon { s.parse().map_err(|_| InvalidMessage) } else { Err(InvalidMessage) }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus { Foreground, Background }
#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    Create { profile_id: String, cols: u16, rows: u16, label: Option<String> },
    List,
    Attach { session_id: [u8; 16], from_offset: Option<u64>, size: Option<(u16, u16)> },
    Detach([u8; 16]),
    Close([u8; 16]),
    Focus(Focus),
}
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Request { request_id: String, req: Req },
    /// `error` do cliente: com `request_id` não é fatal (§2.2).
    Error { request_id: Option<String>, code: Error },
}

/// Controles cliente→host. Campos desconhecidos, tipos errados ou faixas inválidas: `INVALID_MESSAGE`.
pub fn parse_control(text: &str, connection_id: &str) -> R<Control> {
    if text.len() > MAX_MSG { return Err(InvalidMessage); }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(text) else { return Err(InvalidMessage) };
    match o.get("v") { Some(v) if v.as_u64() == Some(1) => {} Some(Value::Number(_)) => return Err(VersionUnsupported), _ => return Err(InvalidMessage) }
    if o.get("connection_id").and_then(Value::as_str) != Some(connection_id) { return Err(InvalidMessage); }
    let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or(InvalidMessage);
    let fields = |req: &[&str], opt: &[&str]| -> R<()> {
        let ok = req.iter().all(|k| o.contains_key(*k))
            && o.keys().all(|k| ["v", "type", "connection_id", "request_id"].contains(&k.as_str()) || req.contains(&k.as_str()) || opt.contains(&k.as_str()));
        if ok { Ok(()) } else { Err(InvalidMessage) }
    };
    let n = |k: &str| o.get(k).and_then(Value::as_u64).filter(|n| *n <= 1000).ok_or(InvalidMessage).and_then(|n| dim(n as u16));
    let sid = |k: &str| uuid_v4_lower(s(k)?).ok_or(InvalidMessage);
    let rid = o.get("request_id").map(|v| v.as_str().filter(|r| uuid_v4_lower(r).is_some()).map(str::to_owned).ok_or(InvalidMessage)).transpose()?;
    let ty = s("type")?;
    if ty == "error" {
        fields(&["code"], &[])?;
        return Ok(Control::Error { request_id: rid, code: Error::from_code(s("code")?).ok_or(InvalidMessage)? });
    }
    let req = match ty {
        "session_create" => {
            fields(&["profile_id", "cols", "rows"], &["label"])?;
            let label = o.get("label").map(|_| s("label").map(str::to_owned)).transpose()?;
            if label.as_ref().is_some_and(|l| l.chars().count() > 64) { return Err(InvalidMessage); }
            Req::Create { profile_id: s("profile_id")?.to_owned(), cols: n("cols")?, rows: n("rows")?, label }
        }
        "session_list" => { fields(&[], &[])?; Req::List }
        "session_attach" => {
            fields(&["session_id", "from_offset"], &["cols", "rows"])?;
            let from_offset = match &o["from_offset"] { Value::Null => None, v => Some(parse_u64(v)?) };
            // ponytail: cols/rows só juntos; um sem o outro é INVALID_MESSAGE (leitura estrita).
            let size = match (o.contains_key("cols"), o.contains_key("rows")) { (false, false) => None, (true, true) => Some((n("cols")?, n("rows")?)), _ => return Err(InvalidMessage) };
            Req::Attach { session_id: sid("session_id")?, from_offset, size }
        }
        "session_detach" => { fields(&["session_id"], &[])?; Req::Detach(sid("session_id")?) }
        "session_close" => { fields(&["session_id"], &[])?; Req::Close(sid("session_id")?) }
        "client_focus" => {
            fields(&["state"], &[])?;
            Req::Focus(match s("state")? { "foreground" => Focus::Foreground, "background" => Focus::Background, _ => return Err(InvalidMessage) })
        }
        _ => return Err(InvalidMessage),
    };
    Ok(Control::Request { request_id: rid.ok_or(InvalidMessage)?, req })
}

pub fn uuid_str(b: &[u8; 16]) -> String {
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// Host→cliente: `body` é o objeto com `type` e campos (u64 já como string, via `u64s`); adiciona `v`, `connection_id`, `request_id?`.
pub fn encode_control(connection_id: &str, request_id: Option<&str>, body: Value) -> String {
    let mut m: Map<String, Value> = match body { Value::Object(m) => m, _ => Map::new() };
    m.insert("v".into(), 1.into());
    m.insert("connection_id".into(), connection_id.into());
    if let Some(r) = request_id { m.insert("request_id".into(), r.into()); }
    Value::Object(m).to_string()
}
pub fn u64s(n: u64) -> Value { n.to_string().into() }
pub fn error(code: Error) -> Value { serde_json::json!({"type":"error","code":code.code()}) }

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CID: &str = "9b2f6c1e-3d4a-4f5b-8c6d-7e8f9a0b1c2d";
    const RID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
    const SID: &str = "0f1e2d3c-4b5a-4968-8778-695a4b3c2d1e";

    fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
    fn sid() -> [u8; 16] { uuid_v4_lower(SID).unwrap() }
    fn raw(ver: u8, op: u8, s: [u8; 16], seq: u64, len: u32, p: &[u8]) -> Vec<u8> {
        [&[ver, op][..], &s, &seq.to_be_bytes(), &len.to_be_bytes(), p].concat()
    }
    fn res<T>(r: R<T>) -> Value { match r { Ok(_) => json!("ok"), Err(e) => json!(e.code()) } }

    fn frame_cases() -> Vec<(&'static str, Vec<u8>)> {
        let s = sid();
        vec![
            ("input", encode_frame(&Frame { session_id: s, seq: 7, op: Op::Input(b"ls\r".to_vec()) })),
            ("output_max", encode_frame(&Frame { session_id: s, seq: u64::MAX, op: Op::Output(vec![b'x'; MAX_IO]) })),
            ("resize", encode_frame(&Frame { session_id: s, seq: 0, op: Op::Resize { cols: 1000, rows: 1 } })),
            ("heartbeat", encode_frame(&Frame { session_id: [0; 16], seq: 0, op: Op::Heartbeat })),
            ("heartbeat_ping_id", encode_frame(&Frame { session_id: [0; 16], seq: 7, op: Op::Heartbeat })),
            ("exited", encode_frame(&Frame { session_id: s, seq: 42, op: Op::Exited(-1) })),
            ("N3_len_maior", raw(1, 1, s, 1, 4, b"abc")),
            ("N3_len_menor", raw(1, 1, s, 1, 2, b"abc")),
            ("N3_opcode_desconhecido", raw(1, 6, s, 1, 1, b"a")),
            ("N3_opcode_zero", raw(1, 0, s, 1, 1, b"a")),
            ("N3_resize_cols_0", raw(1, 3, s, 0, 4, &[0, 0, 0, 24])),
            ("N3_resize_rows_1001", raw(1, 3, s, 0, 4, &[0, 80, 0x03, 0xe9])),
            ("resize_seq_nao_zero", raw(1, 3, s, 1, 4, &[0, 80, 0, 24])),
            ("input_vazio", raw(1, 1, s, 1, 0, b"")),
            ("output_16385", raw(1, 2, s, 1, 16385, &[b'x'; 16385])),
            ("heartbeat_com_sessao", raw(1, 4, s, 0, 0, b"")),
            ("heartbeat_com_payload", raw(1, 4, [0; 16], 0, 1, b"a")),
            ("input_sessao_zero", raw(1, 1, [0; 16], 1, 1, b"a")),
            ("exited_3_bytes", raw(1, 5, s, 1, 3, b"abc")),
            ("curto_29", vec![1; 29]),
            ("versao_2", raw(2, 1, s, 1, 1, b"a")),
        ]
    }

    fn control_cases() -> Vec<(&'static str, String)> {
        let c = |mut v: Value| { v["v"] = 1.into(); v["connection_id"] = CID.into(); v.to_string() };
        let r = |v: Value| { let mut v = v; v["request_id"] = RID.into(); c(v) };
        vec![
            ("create", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":24,"label":"build"}))),
            ("list", r(json!({"type":"session_list"}))),
            ("attach_null", r(json!({"type":"session_attach","session_id":SID,"from_offset":null}))),
            ("attach_max", r(json!({"type":"session_attach","session_id":SID,"from_offset":"18446744073709551615","cols":120,"rows":40}))),
            ("detach", r(json!({"type":"session_detach","session_id":SID}))),
            ("close", r(json!({"type":"session_close","session_id":SID}))),
            ("focus", r(json!({"type":"client_focus","state":"background"}))),
            ("error_com_request_id", r(json!({"type":"error","code":"NOT_FOUND"}))),
            ("error_sem_request_id", c(json!({"type":"error","code":"LIMIT_EXCEEDED"}))),
            ("N10_offset_numero", r(json!({"type":"session_attach","session_id":SID,"from_offset":5}))),
            ("offset_zero_esquerda", r(json!({"type":"session_attach","session_id":SID,"from_offset":"05"}))),
            ("offset_2_64", r(json!({"type":"session_attach","session_id":SID,"from_offset":"18446744073709551616"}))),
            ("offset_negativo", r(json!({"type":"session_attach","session_id":SID,"from_offset":"-1"}))),
            ("offset_ausente", r(json!({"type":"session_attach","session_id":SID}))),
            ("attach_so_cols", r(json!({"type":"session_attach","session_id":SID,"from_offset":null,"cols":80}))),
            ("campo_desconhecido", r(json!({"type":"session_list","x":1}))),
            ("sem_request_id", c(json!({"type":"session_list"}))),
            ("request_id_maiusculo", { let mut v = json!({"type":"session_list","v":1,"connection_id":CID}); v["request_id"] = RID.to_uppercase().into(); v.to_string() }),
            ("session_id_maiusculo", r(json!({"type":"session_detach","session_id":SID.to_uppercase()}))),
            ("cols_0", r(json!({"type":"session_create","profile_id":"pwsh","cols":0,"rows":24}))),
            ("rows_1001", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":1001}))),
            ("label_65", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":24,"label":"é".repeat(65)}))),
            ("label_64", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":24,"label":"é".repeat(64)}))),
            ("focus_invalido", r(json!({"type":"client_focus","state":"idle"}))),
            ("tipo_desconhecido", r(json!({"type":"session_kill","session_id":SID}))),
            ("codigo_desconhecido", r(json!({"type":"error","code":"NOPE"}))),
            ("connection_id_errado", { let mut v = json!({"type":"session_list","v":1,"connection_id":RID}); v["request_id"] = RID.into(); v.to_string() }),
            ("v_2", { let mut v = json!({"type":"session_list","connection_id":CID,"request_id":RID}); v["v"] = 2.into(); v.to_string() }),
            ("v_string", json!({"type":"session_list","v":"1","connection_id":CID,"request_id":RID}).to_string()),
            ("acima_65536", r(json!({"type":"session_create","profile_id":"p".repeat(65536),"cols":80,"rows":24}))),
            ("label_64_emoji", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":24,"label":"😀".repeat(64)}))),
            ("label_65_emoji", r(json!({"type":"session_create","profile_id":"pwsh","cols":80,"rows":24,"label":"😀".repeat(65)}))),
            ("exato_65536", { let base = r(json!({"type":"session_create","profile_id":"","cols":80,"rows":24})).len();
                r(json!({"type":"session_create","profile_id":"p".repeat(MAX_MSG - base),"cols":80,"rows":24})) }),
        ]
    }

    fn fixture(name: &str, v: Value) {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        if std::env::var_os("LIVE_CMD_WRITE_FIXTURE").is_some() {
            std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
        }
        let f: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(f, v, "{name} desatualizada: rode com LIVE_CMD_WRITE_FIXTURE=1");
    }

    #[test]
    fn frames() {
        let v: Vec<Value> = frame_cases().into_iter().map(|(name, b)| {
            let r = parse_frame(&b);
            if let Ok(f) = &r { assert_eq!(encode_frame(f), b, "{name}"); }
            json!({"name":name,"hex":hex(&b),"result":res(r)})
        }).collect();
        let exp = |n: &str| v.iter().find(|c| c["name"] == n).unwrap()["result"].clone();
        for n in ["input", "output_max", "resize", "heartbeat", "heartbeat_ping_id", "exited"] { assert_eq!(exp(n), "ok", "{n}"); }
        assert_eq!(exp("versao_2"), "VERSION_UNSUPPORTED");
        assert!(v.iter().filter(|c| c["name"].as_str().unwrap().starts_with("N3")).all(|c| c["result"] == "INVALID_MESSAGE"));
        fixture("c3-frames.json", json!({"cases": v}));
    }

    #[test]
    fn controls() {
        let v: Vec<Value> = control_cases().into_iter().map(|(name, t)| {
            let r = parse_control(&t, CID);
            let text = if t.len() > MAX_MSG { Value::Null } else { t.into() };
            json!({"name":name,"text":text,"result":res(r)})
        }).collect();
        let ok = ["create", "list", "attach_null", "attach_max", "detach", "close", "focus", "error_com_request_id", "error_sem_request_id", "label_64", "label_64_emoji", "exato_65536"];
        assert_eq!(control_cases().iter().find(|c| c.0 == "exato_65536").unwrap().1.len(), MAX_MSG);
        for c in &v {
            let want = match c["name"].as_str().unwrap() { n if ok.contains(&n) => "ok", "v_2" => "VERSION_UNSUPPORTED", _ => "INVALID_MESSAGE" };
            assert_eq!(c["result"], want, "{}", c["name"]);
        }
        fixture("c3-controls.json", json!({"connection_id": CID, "cases": v}));
    }

    #[test]
    fn values() {
        let Ok(Control::Request { req: Req::Attach { from_offset, size, .. }, request_id }) =
            parse_control(&control_cases()[3].1, CID) else { panic!() };
        assert_eq!((from_offset, size, request_id.as_str()), (Some(u64::MAX), Some((120, 40)), RID));
        assert!(matches!(parse_control(&control_cases()[8].1, CID), Ok(Control::Error { request_id: None, code: LimitExceeded })));
        assert_eq!(uuid_str(&sid()), SID);
        let m: Value = serde_json::from_str(&encode_control(CID, Some(RID), json!({"type":"replay_end","session_id":SID,"offset":u64s(u64::MAX)}))).unwrap();
        assert_eq!(m, json!({"v":1,"connection_id":CID,"request_id":RID,"type":"replay_end","session_id":SID,"offset":"18446744073709551615"}));
        assert_eq!(error(NotFound), json!({"type":"error","code":"NOT_FOUND"}));
    }
}

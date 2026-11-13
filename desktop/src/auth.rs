//! C2 identidade (docs/planos/SPEC-C2-identidade.md): transcript, provas, QR, mensagens e handshake.
//! Puro: sem rede. Relógio injetado por parâmetro (`now: Instant`).
// ponytail: dead_code cobre a API do lado cliente (QR/Offerer), usada só em testes no desktop.
#![allow(dead_code)]

use std::{collections::{HashMap, HashSet, VecDeque}, io, path::PathBuf, sync::{Arc, Mutex}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

pub const OTK_TTL: Duration = Duration::from_secs(300);
pub const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PAIRS: usize = 4;
const NONCE_CACHE: usize = 4096;
const MAX_MSG: usize = 65536;
const MAX_URI: usize = 512;

/// Lista única de erros (C2 §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // nomes vêm dos códigos da spec
pub enum Error { InvalidMessage, VersionUnsupported, Unauthorized, Revoked, Expired, LimitExceeded, NotFound, AlreadyExists, HashMismatch, IoError, ServerNotAllowed }
use Error::*;
const CODES: [(Error, &str); 11] = [
    (InvalidMessage, "INVALID_MESSAGE"), (VersionUnsupported, "VERSION_UNSUPPORTED"), (Unauthorized, "UNAUTHORIZED"),
    (Revoked, "REVOKED"), (Expired, "EXPIRED"), (LimitExceeded, "LIMIT_EXCEEDED"), (NotFound, "NOT_FOUND"),
    (AlreadyExists, "ALREADY_EXISTS"), (HashMismatch, "HASH_MISMATCH"), (IoError, "IO_ERROR"), (ServerNotAllowed, "SERVER_NOT_ALLOWED"),
];
impl Error {
    pub fn code(self) -> &'static str { CODES.iter().find(|c| c.0 == self).map(|c| c.1).unwrap_or("INVALID_MESSAGE") }
    pub(crate) fn from_code(s: &str) -> Option<Self> { CODES.iter().find(|c| c.1 == s).map(|c| c.0) }
}
type R<T> = Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Pair, Reconnect }
impl Mode { fn as_str(self) -> &'static str { match self { Mode::Pair => "pair", Mode::Reconnect => "reconnect" } } }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role { Offerer, Answerer }
impl Role { fn as_str(self) -> &'static str { match self { Role::Offerer => "offerer", Role::Answerer => "answerer" } } }

/// O que cada lado observou no próprio transporte. Fingerprints por papel, nunca local/remoto (§3).
#[derive(Clone)]
pub struct Binding { pub connection_id: [u8; 16], pub room_id: [u8; 16], pub fp_offerer: [u8; 32], pub fp_answerer: [u8; 32] }

fn lp(out: &mut Vec<u8>, x: &[u8]) { out.extend_from_slice(&(x.len() as u32).to_be_bytes()); out.extend_from_slice(x); }

pub fn transcript(mode: Mode, b: &Binding, host_pk: &[u8; 32], client_pk: &[u8; 32], nonce_o: &[u8; 32], nonce_a: &[u8; 32]) -> Vec<u8> {
    let mut t = Vec::with_capacity(312);
    for x in [b"live-cmd/auth/v1".as_slice(), &[1], mode.as_str().as_bytes(), b"offerer", b"answerer", &b.connection_id, &b.room_id,
              host_pk, client_pk, nonce_o, nonce_a, &b.fp_offerer, &b.fp_answerer] { lp(&mut t, x); }
    t
}
fn labeled(kind: &str, role: Role, t: &[u8]) -> Vec<u8> { let mut m = Vec::with_capacity(t.len() + 20); lp(&mut m, format!("{kind}/{}", role.as_str()).as_bytes()); m.extend_from_slice(t); m }
pub fn sign(sk: &SigningKey, role: Role, t: &[u8]) -> [u8; 64] { sk.sign(&labeled("sig", role, t)).to_bytes() }
pub fn verify(pk: &[u8; 32], role: Role, t: &[u8], sig: &[u8; 64]) -> R<()> {
    VerifyingKey::from_bytes(pk).map_err(|_| Unauthorized)?.verify_strict(&labeled("sig", role, t), &Signature::from_bytes(sig)).map_err(|_| Unauthorized)
}
fn hmac(otk: &[u8; 32], role: Role, t: &[u8]) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(otk).expect("HMAC aceita qualquer chave"); m.update(&labeled("mac", role, t)); m
}
pub fn mac(otk: &[u8; 32], role: Role, t: &[u8]) -> [u8; 32] { hmac(otk, role, t).finalize().into_bytes().into() }
/// Tempo constante (`verify_slice`).
pub fn verify_mac(otk: &[u8; 32], role: Role, t: &[u8], tag: &[u8; 32]) -> R<()> { hmac(otk, role, t).verify_slice(tag).map_err(|_| Unauthorized) }

fn random<const N: usize>() -> R<[u8; N]> { let mut b = [0u8; N]; getrandom::getrandom(&mut b).map_err(|_| IoError)?; Ok(b) }
fn b64<const N: usize>(s: &str) -> R<[u8; N]> { B64.decode(s).ok().and_then(|v| v.try_into().ok()).ok_or(InvalidMessage) }

/// UUID textual (qualquer caixa) → 16 bytes.
pub fn uuid_bytes(s: &str) -> Option<[u8; 16]> {
    let b = s.as_bytes();
    if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') { return None; }
    let hex: Vec<u8> = b.iter().copied().filter(|&c| c != b'-').collect();
    if hex.len() != 32 || !hex.iter().all(u8::is_ascii_hexdigit) { return None; }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() { *o = u8::from_str_radix(std::str::from_utf8(&hex[2 * i..2 * i + 2]).ok()?, 16).ok()?; }
    Some(out)
}
pub(crate) fn uuid_v4_lower(s: &str) -> Option<[u8; 16]> {
    let b = s.as_bytes();
    (b.len() == 36 && !s.bytes().any(|c| c.is_ascii_uppercase()) && b[14] == b'4' && matches!(b[19], b'8' | b'9' | b'a' | b'b')).then(|| uuid_bytes(s)).flatten()
}

/// Conteúdo do QR (§1). Nunca logar.
pub struct Pairing { pub session: String, pub room_id: [u8; 16], pub otk: [u8; 32], pub host_pk: [u8; 32], pub server: String }

pub fn parse_qr(uri: &str) -> R<Pairing> {
    if uri.len() > MAX_URI { return Err(InvalidMessage); }
    let query = uri.strip_prefix("remoteterm://pair?").ok_or(InvalidMessage)?;
    let mut f = HashMap::new();
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').ok_or(InvalidMessage)?;
        if f.insert(k, v).is_some() { return Err(InvalidMessage); }
    }
    if f.get("v").is_some_and(|v| *v != "1") { return Err(VersionUnsupported); }
    if f.len() != 5 || ["v", "session", "otk", "host_pk", "server"].iter().any(|k| !f.contains_key(k)) { return Err(InvalidMessage); }
    if !valid_server(f["server"]) { return Err(InvalidMessage); }
    Ok(Pairing {
        session: f["session"].to_owned(),
        room_id: uuid_v4_lower(f["session"]).ok_or(InvalidMessage)?,
        otk: b64(f["otk"])?,
        host_pk: b64(f["host_pk"])?,
        server: f["server"].to_owned(),
    })
}
fn valid_server(s: &str) -> bool {
    let (host, port) = match s.split_once(':') { Some((h, p)) => (h, Some(p)), None => (s, None) };
    !host.is_empty() && host.len() <= 253 && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        && port.is_none_or(|p| !p.starts_with('0') && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n)))
}
/// Allowlist fixa embutida no cliente (§1).
pub fn check_server(server: &str, allowlist: &[&str]) -> R<()> { if allowlist.contains(&server) { Ok(()) } else { Err(ServerNotAllowed) } }

/// Mensagens de controle C2 (§2).
/// Debug manual: só o tipo, nunca nonces/assinaturas/MACs (§1).
impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Msg::Hello { .. } => "Hello", Msg::Challenge { .. } => "Challenge", Msg::Proof { .. } => "Proof", Msg::AuthOk => "AuthOk", _ => "Error" })
    }
}
#[derive(Clone, PartialEq)]
pub enum Msg {
    Hello { mode: Mode, client_pk: [u8; 32], nonce_o: [u8; 32] },
    Challenge { host_pk: [u8; 32], nonce_a: [u8; 32], sig_a: [u8; 64], mac_a: Option<[u8; 32]> },
    Proof { sig_o: [u8; 64], mac_o: Option<[u8; 32]> },
    AuthOk,
    Fail(Error),
}
impl Msg {
    pub fn to_json(&self, connection_id: &str) -> String {
        let e = |b: &[u8]| Value::from(B64.encode(b));
        let mut m = match self {
            Msg::Hello { mode, client_pk, nonce_o } => json!({"type":"auth_hello","mode":mode.as_str(),"role":"offerer","client_pk":e(client_pk),"nonce_o":e(nonce_o)}),
            Msg::Challenge { host_pk, nonce_a, sig_a, mac_a } => {
                let mut v = json!({"type":"auth_challenge","role":"answerer","host_pk":e(host_pk),"nonce_a":e(nonce_a),"sig_a":e(sig_a)});
                if let Some(m) = mac_a { v["mac_a"] = e(m); }
                v
            }
            Msg::Proof { sig_o, mac_o } => {
                let mut v = json!({"type":"auth_proof","sig_o":e(sig_o)});
                if let Some(m) = mac_o { v["mac_o"] = e(m); }
                v
            }
            Msg::AuthOk => json!({"type":"auth_ok"}),
            Msg::Fail(c) => json!({"type":"error","code":c.code()}),
        };
        m["v"] = 1.into();
        m["connection_id"] = connection_id.into();
        m.to_string()
    }
}

pub fn parse_msg(text: &str, connection_id: &str) -> R<Msg> {
    if text.len() > MAX_MSG { return Err(InvalidMessage); }
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(text) else { return Err(InvalidMessage) };
    match o.get("v") { Some(v) if v.as_u64() == Some(1) => {} Some(Value::Number(_)) => return Err(VersionUnsupported), _ => return Err(InvalidMessage) }
    if o.get("connection_id").and_then(Value::as_str) != Some(connection_id) { return Err(InvalidMessage); }
    let s = |k: &str| o.get(k).and_then(Value::as_str).ok_or(InvalidMessage);
    let fields = |req: &[&str], opt: &str| -> R<()> {
        let ok = req.iter().all(|k| o.contains_key(*k))
            && o.keys().all(|k| ["v", "type", "connection_id", opt].contains(&k.as_str()) || req.contains(&k.as_str()));
        if ok { Ok(()) } else { Err(InvalidMessage) }
    };
    let role = |want: &str| if s("role")? == want { Ok(()) } else { Err(InvalidMessage) };
    let opt_mac = |k: &str| o.get(k).map(|_| s(k).and_then(b64)).transpose();
    Ok(match s("type")? {
        "auth_hello" => {
            fields(&["mode", "role", "client_pk", "nonce_o"], "")?;
            role("offerer")?;
            let mode = match s("mode")? { "pair" => Mode::Pair, "reconnect" => Mode::Reconnect, _ => return Err(InvalidMessage) };
            Msg::Hello { mode, client_pk: b64(s("client_pk")?)?, nonce_o: b64(s("nonce_o")?)? }
        }
        "auth_challenge" => {
            fields(&["role", "host_pk", "nonce_a", "sig_a"], "mac_a")?;
            role("answerer")?;
            Msg::Challenge { host_pk: b64(s("host_pk")?)?, nonce_a: b64(s("nonce_a")?)?, sig_a: b64(s("sig_a")?)?, mac_a: opt_mac("mac_a")? }
        }
        "auth_proof" => { fields(&["sig_o"], "mac_o")?; Msg::Proof { sig_o: b64(s("sig_o")?)?, mac_o: opt_mac("mac_o")? } }
        "auth_ok" => { fields(&[], "")?; Msg::AuthOk }
        "error" => { fields(&["code"], "")?; Msg::Fail(Error::from_code(s("code")?).ok_or(InvalidMessage)?) }
        _ => return Err(InvalidMessage),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State { New, Challenged, Authenticated, Closed }

/// Entrada da trust store (C2 §6). Revogado = tombstone (`revoked_at`).
struct Entry { label: String, paired_at: u64, revoked_at: Option<u64> }
fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }
struct Otk { key: [u8; 32], issued: Instant, consumed: bool }

/// `nonce_o` vistos na vida do processo; evicção FIFO (repetição é rejeitada, então não há acesso a renovar).
#[derive(Default)]
struct Nonces { order: VecDeque<[u8; 32]>, seen: HashSet<[u8; 32]> }
impl Nonces {
    fn insert(&mut self, n: [u8; 32]) -> bool {
        if !self.seen.insert(n) { return false; }
        self.order.push_back(n);
        if self.order.len() > NONCE_CACHE && let Some(old) = self.order.pop_front() { self.seen.remove(&old); }
        true
    }
}

/// Estado do host. Reinício do processo = novo `Host`: OTK, nonces e handshakes pendentes somem (§2.1).
pub struct Host {
    sk: SigningKey,
    otk: Mutex<Option<Otk>>,
    nonces: Mutex<Nonces>,
    trust: Mutex<HashMap<[u8; 32], Entry>>,
    pairs: Mutex<usize>,
    store: Option<PathBuf>,
}
impl Host {
    pub fn new(sk: SigningKey) -> Arc<Self> {
        Arc::new(Self::bare(sk, None))
    }
    fn bare(sk: SigningKey, store: Option<PathBuf>) -> Self {
        Host { sk, otk: Mutex::new(None), nonces: Mutex::default(), trust: Mutex::default(), pairs: Mutex::new(0), store }
    }
    /// Trust store em arquivo (`schema_version`, escrita atômica temp + rename).
    pub fn with_store(sk: SigningKey, path: PathBuf) -> io::Result<Arc<Self>> {
        let h = Self::bare(sk, Some(path));
        h.reload()?;
        Ok(Arc::new(h))
    }
    /// Relê o arquivo: revogação feita por outro processo (`--revoke`) chega por aqui.
    pub fn reload(&self) -> io::Result<()> {
        let Some(path) = &self.store else { return Ok(()) };
        let text = match std::fs::read_to_string(path) { Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()), r => r? };
        let bad = || io::Error::other("trust store inválida");
        let v: Value = serde_json::from_str(&text).map_err(|_| bad())?;
        if v["schema_version"] != 1 { return Err(bad()); }
        let num = |c: &Value, k: &str| c[k].as_str().and_then(|s| s.parse::<u64>().ok());
        let mut map = HashMap::new();
        for c in v["clients"].as_array().ok_or_else(bad)? {
            let pk = c["client_pk"].as_str().and_then(|s| b64::<32>(s).ok()).ok_or_else(bad)?;
            map.insert(pk, Entry { label: c["label"].as_str().unwrap_or_default().to_owned(), paired_at: num(c, "paired_at").ok_or_else(bad)?, revoked_at: num(c, "revoked_at") });
        }
        *self.trust.lock().unwrap() = map;
        Ok(())
    }
    // ponytail: dois processos gravando ao mesmo tempo podem perder uma atualização; trocar por lock de arquivo se houver host multiprocesso.
    fn save(&self, trust: &HashMap<[u8; 32], Entry>) -> io::Result<()> {
        let Some(path) = &self.store else { return Ok(()) };
        let clients: Vec<Value> = trust.iter().map(|(pk, e)| {
            let mut c = json!({"client_pk": B64.encode(pk), "label": e.label, "paired_at": e.paired_at.to_string()});
            if let Some(r) = e.revoked_at { c["revoked_at"] = r.to_string().into(); }
            c
        }).collect();
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&json!({"schema_version": 1, "clients": clients}))?)?;
        std::fs::rename(&tmp, path)
    }
    pub fn pk(&self) -> [u8; 32] { self.sk.verifying_key().to_bytes() }
    /// Nova OTK invalida a anterior (no máximo uma ativa).
    pub fn issue_otk(&self, now: Instant) -> R<[u8; 32]> {
        let key = random()?;
        *self.otk.lock().unwrap() = Some(Otk { key, issued: now, consumed: false });
        Ok(key)
    }
    /// Marca tombstone e persiste. Conexões ativas do `client_pk` fecham ao reler a store (main.rs).
    pub fn revoke(&self, client_pk: [u8; 32]) -> R<()> {
        let mut t = self.trust.lock().unwrap();
        t.entry(client_pk).or_insert_with(|| Entry { label: String::new(), paired_at: now_ms(), revoked_at: None }).revoked_at.get_or_insert_with(now_ms);
        self.save(&t).map_err(|_| IoError)
    }
    /// Aparelhos pareados não revogados: `(client_pk, label, paired_at_ms)`.
    pub fn clients(&self) -> Vec<([u8; 32], String, u64)> {
        self.trust.lock().unwrap().iter().filter(|(_, e)| e.revoked_at.is_none()).map(|(pk, e)| (*pk, e.label.clone(), e.paired_at)).collect()
    }
    /// Tempo restante da OTK ativa (não consumida e não expirada).
    pub fn otk_remaining(&self, now: Instant) -> Option<Duration> {
        self.otk.lock().unwrap().as_ref().filter(|o| !o.consumed).and_then(|o| OTK_TTL.checked_sub(now.duration_since(o.issued))).filter(|d| !d.is_zero())
    }
    /// Descarta a OTK pendente (janela do QR fechada, P13).
    pub fn cancel_otk(&self) { *self.otk.lock().unwrap() = None; }
    pub fn check_trusted(&self, client_pk: &[u8; 32]) -> R<()> {
        match self.trust.lock().unwrap().get(client_pk) { Some(e) if e.revoked_at.is_some() => Err(Revoked), Some(_) => Ok(()), None => Err(Unauthorized) }
    }
    fn active_otk(&self, now: Instant) -> R<[u8; 32]> {
        match &*self.otk.lock().unwrap() {
            Some(o) if !o.consumed => if now.duration_since(o.issued) >= OTK_TTL { Err(Expired) } else { Ok(o.key) },
            _ => Err(Unauthorized),
        }
    }
    /// CAS active → consumed. Perdeu (consumida/substituída) → UNAUTHORIZED.
    fn consume_otk(&self, key: &[u8; 32], now: Instant) -> R<()> {
        match self.otk.lock().unwrap().as_mut() {
            Some(o) if o.key == *key && !o.consumed => {
                if now.duration_since(o.issued) >= OTK_TTL { return Err(Expired); }
                o.consumed = true;
                Ok(())
            }
            _ => Err(Unauthorized),
        }
    }
}

struct Pending { mode: Mode, client_pk: [u8; 32], otk: Option<[u8; 32]>, t: Vec<u8> }

/// Handshake do lado answerer, um por PeerConnection, criado no DTLS `connected`.
/// `Err(e)` de qualquer método: enviar `Msg::Fail(e)` e fechar a PeerConnection inteira.
pub struct HostHandshake { host: Arc<Host>, binding: Binding, started: Instant, state: State, slot: bool, pending: Option<Pending> }
impl HostHandshake {
    pub fn new(host: Arc<Host>, binding: Binding, connected_at: Instant) -> Self {
        HostHandshake { host, binding, started: connected_at, state: State::New, slot: false, pending: None }
    }
    pub fn state(&self) -> State { self.state }
    pub fn client_pk(&self) -> Option<[u8; 32]> { (self.state == State::Authenticated).then(|| self.pending.as_ref().map(|p| p.client_pk)).flatten() }
    pub fn deadline(&self) -> Instant { self.started + AUTH_TIMEOUT }
    /// Chamar no `deadline()`: fecha com EXPIRED se ainda não autenticou.
    pub fn tick(&mut self, now: Instant) -> R<()> {
        if self.state != State::Authenticated && now >= self.deadline() { self.close(); return Err(Expired); }
        Ok(())
    }
    pub fn handle(&mut self, msg: Msg, now: Instant) -> R<Msg> {
        let r = self.tick(now).and_then(|_| self.step(msg, now));
        match r { Err(_) => self.close(), Ok(Msg::AuthOk) => self.release(), _ => {} }
        r
    }
    fn step(&mut self, msg: Msg, now: Instant) -> R<Msg> {
        match (self.state, msg) {
            (State::New, Msg::Hello { mode, client_pk, nonce_o }) => {
                let otk = match mode {
                    Mode::Reconnect => { self.host.check_trusted(&client_pk)?; None }
                    Mode::Pair => { let k = self.host.active_otk(now)?; self.acquire()?; Some(k) }
                };
                if !self.host.nonces.lock().unwrap().insert(nonce_o) { return Err(Unauthorized); }
                let (host_pk, nonce_a) = (self.host.pk(), random()?);
                let t = transcript(mode, &self.binding, &host_pk, &client_pk, &nonce_o, &nonce_a);
                let sig_a = sign(&self.host.sk, Role::Answerer, &t);
                let mac_a = otk.map(|k| mac(&k, Role::Answerer, &t));
                self.pending = Some(Pending { mode, client_pk, otk, t });
                self.state = State::Challenged;
                Ok(Msg::Challenge { host_pk, nonce_a, sig_a, mac_a })
            }
            (State::Challenged, Msg::Proof { sig_o, mac_o }) => {
                let p = self.pending.as_ref().ok_or(InvalidMessage)?;
                match (p.otk, mac_o) {
                    (Some(k), Some(m)) => {
                        verify(&p.client_pk, Role::Offerer, &p.t, &sig_o)?;
                        verify_mac(&k, Role::Offerer, &p.t, &m)?;
                        self.host.consume_otk(&k, now)?;
                        // Falha ao gravar após o CAS → IO_ERROR; OTK continua consumida (§5).
                        let mut t = self.host.trust.lock().unwrap();
                        let prev = t.insert(p.client_pk, Entry { label: B64.encode(&p.client_pk[..6]), paired_at: now_ms(), revoked_at: None });
                        if self.host.save(&t).is_err() {
                            match prev { Some(e) => { t.insert(p.client_pk, e); } None => { t.remove(&p.client_pk); } }
                            return Err(IoError);
                        }
                    }
                    (None, None) => {
                        verify(&p.client_pk, Role::Offerer, &p.t, &sig_o)?;
                        self.host.check_trusted(&p.client_pk)?;
                    }
                    _ => return Err(InvalidMessage),
                }
                debug_assert!(matches!(p.mode, Mode::Pair) == p.otk.is_some());
                self.state = State::Authenticated;
                Ok(Msg::AuthOk)
            }
            _ => Err(InvalidMessage),
        }
    }
    fn acquire(&mut self) -> R<()> {
        let mut n = self.host.pairs.lock().unwrap();
        if *n >= MAX_PAIRS { return Err(LimitExceeded); }
        *n += 1;
        self.slot = true;
        Ok(())
    }
    fn release(&mut self) { if std::mem::take(&mut self.slot) { *self.host.pairs.lock().unwrap() -= 1; } }
    fn close(&mut self) { self.state = State::Closed; self.pending = None; self.release(); }
}
impl Drop for HostHandshake { fn drop(&mut self) { self.release(); } }

/// Handshake do lado offerer (mobile ou peer Rust de teste). `otk = Some` → pair; `None` → reconnect.
pub struct Offerer { sk: SigningKey, pin: [u8; 32], otk: Option<[u8; 32]>, binding: Binding, nonce_o: [u8; 32], started: Instant, state: State }
impl Offerer {
    /// Devolve o `auth_hello` a enviar.
    pub fn new(otk: Option<[u8; 32]>, sk: SigningKey, pin: [u8; 32], binding: Binding, connected_at: Instant) -> R<(Self, Msg)> {
        let o = Offerer { sk, pin, otk, binding, nonce_o: random()?, started: connected_at, state: State::New };
        let hello = Msg::Hello { mode: o.mode(), client_pk: o.sk.verifying_key().to_bytes(), nonce_o: o.nonce_o };
        Ok((o, hello))
    }
    fn mode(&self) -> Mode { if self.otk.is_some() { Mode::Pair } else { Mode::Reconnect } }
    pub fn state(&self) -> State { self.state }
    /// `Ok(Some(proof))` após o desafio; `Ok(None)` ao receber `auth_ok`.
    pub fn handle(&mut self, msg: Msg, now: Instant) -> R<Option<Msg>> {
        let r = if now >= self.started + AUTH_TIMEOUT { Err(Expired) } else { self.step(msg) };
        if r.is_err() { self.state = State::Closed; }
        r
    }
    fn step(&mut self, msg: Msg) -> R<Option<Msg>> {
        match (self.state, msg) {
            (State::New, Msg::Challenge { host_pk, nonce_a, sig_a, mac_a }) => {
                if host_pk != self.pin { return Err(Unauthorized); }
                let t = transcript(self.mode(), &self.binding, &host_pk, &self.sk.verifying_key().to_bytes(), &self.nonce_o, &nonce_a);
                verify(&host_pk, Role::Answerer, &t, &sig_a)?;
                let mac_o = match (self.otk, mac_a) {
                    (Some(k), Some(m)) => { verify_mac(&k, Role::Answerer, &t, &m)?; Some(mac(&k, Role::Offerer, &t)) }
                    (None, None) => None,
                    _ => return Err(InvalidMessage),
                };
                self.state = State::Challenged;
                Ok(Some(Msg::Proof { sig_o: sign(&self.sk, Role::Offerer, &t), mac_o }))
            }
            (State::Challenged, Msg::AuthOk) => { self.state = State::Authenticated; Ok(None) }
            _ => Err(InvalidMessage),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const CID: &str = "00010203-0405-0607-0809-0a0b0c0d0e0f"; // vetor 3.1: bytes 00..0f

    fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
    fn unhex(s: &str) -> Vec<u8> { (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect() }
    fn binding() -> Binding { Binding { connection_id: uuid_bytes(CID).unwrap(), room_id: std::array::from_fn(|i| 0x10 + i as u8), fp_offerer: [0x40; 32], fp_answerer: [0x41; 32] } }
    /// Passa pelo JSON, como no fio.
    fn wire(m: Msg) -> Msg { parse_msg(&m.to_json(CID), CID).unwrap() }
    fn host() -> Arc<Host> { Host::new(SigningKey::from_bytes(&[0x60; 32])) }
    fn client(n: u8) -> SigningKey { SigningKey::from_bytes(&[n; 32]) }

    /// Pareamento completo; devolve os dois estados finais.
    fn pair(host: &Arc<Host>, otk: [u8; 32], c: SigningKey, now: Instant) -> R<(HostHandshake, Offerer)> {
        let (mut off, hello) = Offerer::new(Some(otk), c, host.pk(), binding(), now)?;
        let mut h = HostHandshake::new(host.clone(), binding(), now);
        let proof = off.handle(wire(h.handle(wire(hello), now)?), now)?.unwrap();
        assert_eq!(off.handle(wire(h.handle(wire(proof), now)?), now)?, None);
        Ok((h, off))
    }
    fn reconnect(host: &Arc<Host>, c: SigningKey, now: Instant) -> R<()> {
        let (mut off, hello) = Offerer::new(None, c, host.pk(), binding(), now)?;
        let mut h = HostHandshake::new(host.clone(), binding(), now);
        let proof = off.handle(h.handle(hello, now)?, now)?.unwrap();
        off.handle(h.handle(proof, now)?, now)?;
        assert_eq!(h.state(), State::Authenticated);
        Ok(())
    }

    fn vector() -> Value {
        let t = transcript(Mode::Pair, &binding(), &[0x20; 32], &[0x21; 32], &[0x30; 32], &[0x31; 32]);
        let (h, c, otk) = (client(0x60), client(0x61), [0x50u8; 32]);
        json!({
            "note": "C2 §3.1; T usa host_pk=20x32/client_pk=21x32 estruturais; assinaturas verificam com host_vk/client_vk (sementes 60x32/61x32); OTK=50x32",
            "host_vk": hex(h.verifying_key().as_bytes()), "client_vk": hex(c.verifying_key().as_bytes()),
            "T": hex(&t),
            "sig_a": hex(&sign(&h, Role::Answerer, &t)), "sig_o": hex(&sign(&c, Role::Offerer, &t)),
            "mac_a": hex(&mac(&otk, Role::Answerer, &t)), "mac_o": hex(&mac(&otk, Role::Offerer, &t)),
        })
    }

    #[test]
    fn a3_transcript_bytes_match_spec_table() {
        let t = transcript(Mode::Pair, &binding(), &[0x20; 32], &[0x21; 32], &[0x30; 32], &[0x31; 32]);
        let mut e = unhex("000000106c6976652d636d642f617574682f7631000000010100000004706169720000000\
76f66666572657200000008616e737765726572");
        e.extend([0, 0, 0, 16]); e.extend(0..16u8);
        e.extend([0, 0, 0, 16]); e.extend(16..32u8);
        for x in [0x20, 0x21, 0x30, 0x31, 0x40, 0x41] { e.extend([0, 0, 0, 32]); e.extend([x; 32]); }
        assert_eq!(t.len(), 312);
        assert_eq!(t, e);
    }

    #[test]
    fn a3_fixture_matches_and_verifies() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/c2-vector-3.1.json");
        let v = vector();
        if std::env::var_os("LIVE_CMD_WRITE_FIXTURE").is_some() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
        }
        let f: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(f, v);
        let g = |k: &str| unhex(f[k].as_str().unwrap());
        let (t, hvk, cvk): (Vec<u8>, [u8; 32], [u8; 32]) = (g("T"), g("host_vk").try_into().unwrap(), g("client_vk").try_into().unwrap());
        verify(&hvk, Role::Answerer, &t, &g("sig_a").try_into().unwrap()).unwrap();
        verify(&cvk, Role::Offerer, &t, &g("sig_o").try_into().unwrap()).unwrap();
        verify_mac(&[0x50; 32], Role::Offerer, &t, &g("mac_o").try_into().unwrap()).unwrap();
    }

    #[test]
    fn vector_negatives_swap_fps_labels_and_mode() {
        let mut b = binding();
        let t = transcript(Mode::Pair, &b, &[0x20; 32], &[0x21; 32], &[0x30; 32], &[0x31; 32]);
        std::mem::swap(&mut b.fp_offerer, &mut b.fp_answerer);
        let swapped = transcript(Mode::Pair, &b, &[0x20; 32], &[0x21; 32], &[0x30; 32], &[0x31; 32]);
        let reconn = transcript(Mode::Reconnect, &binding(), &[0x20; 32], &[0x21; 32], &[0x30; 32], &[0x31; 32]);
        let h = client(0x60);
        let pk = h.verifying_key().to_bytes();
        let sig = sign(&h, Role::Answerer, &t);
        assert_eq!(verify(&pk, Role::Answerer, &swapped, &sig), Err(Unauthorized));
        assert_eq!(verify(&pk, Role::Offerer, &t, &sig), Err(Unauthorized));
        assert_eq!(verify(&pk, Role::Answerer, &reconn, &sig), Err(Unauthorized));
        assert_eq!(verify_mac(&[0x50; 32], Role::Offerer, &t, &mac(&[0x50; 32], Role::Answerer, &t)), Err(Unauthorized));
    }

    #[test]
    fn a1_pair_then_a2_reconnect_then_n10_revoked() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        let (hs, off) = pair(&h, otk, client(1), now).unwrap();
        assert_eq!((hs.state(), off.state()), (State::Authenticated, State::Authenticated));
        assert_eq!(hs.client_pk(), Some(client(1).verifying_key().to_bytes()));
        reconnect(&h, client(1), now).unwrap();
        assert_eq!(reconnect(&h, client(2), now), Err(Unauthorized));
        h.revoke(client(1).verifying_key().to_bytes()).unwrap();
        assert_eq!(reconnect(&h, client(1), now), Err(Revoked));
    }

    #[test]
    fn n4_otk_expired_at_hello_and_at_proof() {
        let (h, t0) = (host(), Instant::now());
        let otk = h.issue_otk(t0).unwrap();
        assert_eq!(pair(&h, otk, client(1), t0 + OTK_TTL).err(), Some(Expired));
        let at = t0 + OTK_TTL - Duration::from_secs(1);
        let (mut off, hello) = Offerer::new(Some(otk), client(1), h.pk(), binding(), at).unwrap();
        let mut hs = HostHandshake::new(h.clone(), binding(), at);
        let proof = off.handle(hs.handle(hello, at).unwrap(), at).unwrap().unwrap();
        assert_eq!(hs.handle(proof, at + Duration::from_secs(2)), Err(Expired));
        assert_eq!(hs.state(), State::Closed);
    }

    #[test]
    fn n5_otk_reuse_and_replacement_rejected() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        pair(&h, otk, client(1), now).unwrap();
        assert_eq!(pair(&h, otk, client(2), now).err(), Some(Unauthorized));
        let old = h.issue_otk(now).unwrap();
        h.issue_otk(now).unwrap();
        let (mut off, hello) = Offerer::new(Some(old), client(3), h.pk(), binding(), now).unwrap();
        let mut hs = HostHandshake::new(h.clone(), binding(), now);
        // OTK nova ativa: o host desafia com ela; o MAC do cliente (OTK antiga) não confere.
        assert_eq!(off.handle(hs.handle(hello, now).unwrap(), now), Err(Unauthorized));
    }

    #[test]
    fn n6_repeated_nonce_and_limit_exceeded() {
        let (h, now) = (host(), Instant::now());
        h.issue_otk(now).unwrap();
        let hello = |n: u8| Msg::Hello { mode: Mode::Pair, client_pk: client(1).verifying_key().to_bytes(), nonce_o: [n; 32] };
        let mut a = HostHandshake::new(h.clone(), binding(), now);
        a.handle(hello(7), now).unwrap();
        let mut b = HostHandshake::new(h.clone(), binding(), now);
        assert_eq!(b.handle(hello(7), now), Err(Unauthorized));
        let mut open: Vec<_> = (8..11).map(|n| { let mut x = HostHandshake::new(h.clone(), binding(), now); x.handle(hello(n), now).unwrap(); x }).collect();
        let mut fifth = HostHandshake::new(h.clone(), binding(), now);
        assert_eq!(fifth.handle(hello(11), now), Err(LimitExceeded));
        open.pop();
        let mut again = HostHandshake::new(h.clone(), binding(), now);
        again.handle(hello(12), now).unwrap();
    }

    #[test]
    fn n7_n7b_invalid_role_mode_version_and_fields() {
        let ok = Msg::Hello { mode: Mode::Pair, client_pk: [1; 32], nonce_o: [2; 32] }.to_json(CID);
        assert!(parse_msg(&ok, CID).is_ok());
        let edit = |f: &dyn Fn(&mut Value)| { let mut v: Value = serde_json::from_str(&ok).unwrap(); f(&mut v); parse_msg(&v.to_string(), CID) };
        assert_eq!(edit(&|v| v["role"] = "answerer".into()), Err(InvalidMessage));
        assert_eq!(edit(&|v| v["mode"] = "other".into()), Err(InvalidMessage));
        assert_eq!(edit(&|v| v["extra"] = 1.into()), Err(InvalidMessage));
        assert_eq!(edit(&|v| v["nonce_o"] = "AAAA".into()), Err(InvalidMessage));
        assert_eq!(edit(&|v| v["v"] = 2.into()), Err(VersionUnsupported));
        assert_eq!(edit(&|v| v["v"] = "1".into()), Err(InvalidMessage));
        assert_eq!(parse_msg(&ok, "ffffffff-ffff-4fff-bfff-ffffffffffff"), Err(InvalidMessage));
        assert_eq!(parse_msg(&" ".repeat(MAX_MSG + 1), CID), Err(InvalidMessage));
        assert_eq!(wire(Msg::Fail(Revoked)), Msg::Fail(Revoked));
        let (h, now) = (host(), Instant::now());
        let mut hs = HostHandshake::new(h, binding(), now);
        assert_eq!(hs.handle(Msg::Proof { sig_o: [0; 64], mac_o: None }, now), Err(InvalidMessage));
        assert_eq!(hs.state(), State::Closed);
    }

    #[test]
    fn n8_host_pk_differs_from_pin() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        let (mut off, hello) = Offerer::new(Some(otk), client(1), client(9).verifying_key().to_bytes(), binding(), now).unwrap();
        let mut hs = HostHandshake::new(h, binding(), now);
        assert_eq!(off.handle(hs.handle(hello, now).unwrap(), now), Err(Unauthorized));
        assert_eq!(off.state(), State::Closed);
    }

    #[test]
    fn n9_two_clients_same_qr_exactly_one_wins() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        let mut runs: Vec<_> = (1..=2).map(|n| {
            let (mut off, hello) = Offerer::new(Some(otk), client(n), h.pk(), binding(), now).unwrap();
            let mut hs = HostHandshake::new(h.clone(), binding(), now);
            let proof = off.handle(hs.handle(hello, now).unwrap(), now).unwrap().unwrap();
            (hs, proof)
        }).collect();
        let wins = std::thread::scope(|s| {
            let hs: Vec<_> = runs.iter_mut().map(|(hs, p)| { let p = p.clone(); s.spawn(move || hs.handle(p, now)) }).collect();
            hs.into_iter().map(|j| j.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(wins.iter().filter(|r| **r == Ok(Msg::AuthOk)).count(), 1);
        assert!(wins.contains(&Err(Unauthorized)));
    }

    #[test]
    fn n12_timeout_without_proof() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        let (mut off, hello) = Offerer::new(Some(otk), client(1), h.pk(), binding(), now).unwrap();
        let mut hs = HostHandshake::new(h.clone(), binding(), now);
        let proof = off.handle(hs.handle(hello, now).unwrap(), now).unwrap().unwrap();
        assert_eq!(hs.tick(now + AUTH_TIMEOUT - Duration::from_millis(1)), Ok(()));
        assert_eq!(hs.tick(hs.deadline()), Err(Expired));
        assert_eq!(hs.state(), State::Closed);
        assert_eq!(hs.handle(proof, now), Err(InvalidMessage), "fechado não aceita nada");
        assert_eq!(*h.pairs.lock().unwrap(), 0);
    }

    #[test]
    fn n13_reflection_rejected() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        // client_pk = host_pk: sem rótulos por papel, sig_a/mac_a refletidos verificariam.
        let hello = Msg::Hello { mode: Mode::Pair, client_pk: h.pk(), nonce_o: [7; 32] };
        let mut hs = HostHandshake::new(h.clone(), binding(), now);
        let Msg::Challenge { sig_a, mac_a, .. } = hs.handle(hello, now).unwrap() else { panic!() };
        assert_eq!(hs.handle(Msg::Proof { sig_o: sig_a, mac_o: mac_a }, now), Err(Unauthorized));
        assert_eq!(h.active_otk(now), Ok(otk), "prova inválida não consome a OTK");
    }

    #[test]
    fn n14_host_restart_drops_pending() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        let (mut off, hello) = Offerer::new(Some(otk), client(1), h.pk(), binding(), now).unwrap();
        let mut hs = HostHandshake::new(h, binding(), now);
        let proof = off.handle(hs.handle(hello.clone(), now).unwrap(), now).unwrap().unwrap();
        let restarted = host();
        let mut fresh = HostHandshake::new(restarted.clone(), binding(), now);
        assert_eq!(fresh.handle(proof, now), Err(InvalidMessage));
        assert_eq!(HostHandshake::new(restarted, binding(), now).handle(hello, now), Err(Unauthorized));
    }

    #[test]
    fn reconnect_mode_rejects_mac() {
        let (h, now) = (host(), Instant::now());
        let otk = h.issue_otk(now).unwrap();
        pair(&h, otk, client(1), now).unwrap();
        let (mut off, hello) = Offerer::new(None, client(1), h.pk(), binding(), now).unwrap();
        let mut hs = HostHandshake::new(h.clone(), binding(), now);
        let Msg::Challenge { host_pk, nonce_a, sig_a, .. } = hs.handle(hello, now).unwrap() else { panic!() };
        assert_eq!(off.handle(Msg::Challenge { host_pk, nonce_a, sig_a, mac_a: Some([0; 32]) }, now), Err(InvalidMessage));
        assert_eq!(hs.handle(Msg::Proof { sig_o: [0; 64], mac_o: Some([0; 32]) }, now), Err(InvalidMessage));
    }

    #[test]
    fn n16_qr_parse_and_allowlist() {
        let otk = B64.encode([5u8; 32]);
        let pk = B64.encode([6u8; 32]);
        let good = format!("remoteterm://pair?v=1&session=0f0e0d0c-0b0a-4908-8706-050403020100&otk={otk}&host_pk={pk}&server=sig.example.com:443");
        let p = parse_qr(&good).unwrap();
        assert_eq!((p.otk, p.host_pk, p.room_id[0], p.server.as_str()), ([5; 32], [6; 32], 0x0f, "sig.example.com:443"));
        assert_eq!(check_server(&p.server, &["sig.example.com:443"]), Ok(()));
        assert_eq!(check_server(&p.server, &["other.example.com"]), Err(ServerNotAllowed));
        assert_eq!(parse_qr(&good.replace("v=1", "v=2")).err(), Some(VersionUnsupported));
        for bad in [
            good.replace("remoteterm://pair?", "remoteterm://other?"),
            format!("{good}&x=1"), format!("{good}&v=1"), good.replace("&server=sig.example.com:443", ""),
            good.replace("0f0e0d0c", "0F0E0D0C"), good.replace("-4908-", "-1908-"),
            good.replace(&otk, &format!("{otk}=")), good.replace(&otk, &otk[..42]), good.replace(&pk, &pk.replace('G', "+")),
            good.replace(":443", ":0"), good.replace(":443", ":70000"), good.replace("sig.", "SIG."),
            format!("{good}&pad={}", "a".repeat(600)),
        ] { assert!(parse_qr(&bad).is_err(), "{}", bad.len()); }
        assert!(good.len() <= MAX_URI);
    }

    #[test]
    fn nonce_cache_is_bounded() {
        let mut n = Nonces::default();
        for i in 0..=NONCE_CACHE as u32 { let mut k = [0u8; 32]; k[..4].copy_from_slice(&i.to_be_bytes()); assert!(n.insert(k)); }
        assert_eq!(n.seen.len(), NONCE_CACHE);
        assert!(n.insert([0; 32]), "mais antigo foi descartado");
        assert!(!n.insert([0; 32]));
    }

    #[test]
    fn trust_store_persists_revocation_and_io_error_keeps_otk_consumed() {
        let dir = std::env::temp_dir().join(format!("live-cmd-auth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trust.json");
        let _ = std::fs::remove_file(&path);
        let now = Instant::now();
        let h = Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap();
        pair(&h, h.issue_otk(now).unwrap(), client(1), now).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert!(!path.with_extension("tmp").exists(), "rename atômico");
        // Outro processo (`--revoke`) grava; este relê.
        let other = Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap();
        other.revoke(client(1).verifying_key().to_bytes()).unwrap();
        reconnect(&h, client(1), now).unwrap();
        h.reload().unwrap();
        assert_eq!(reconnect(&h, client(1), now), Err(Revoked));
        // Gravação impossível (destino é diretório) após o CAS → IO_ERROR, OTK consumida, cliente não confiável.
        let bad = Arc::new(Host::bare(SigningKey::from_bytes(&[0x60; 32]), Some(dir.clone())));
        let otk = bad.issue_otk(now).unwrap();
        assert_eq!(pair(&bad, otk, client(2), now).err(), Some(IoError));
        assert_eq!(bad.active_otk(now), Err(Unauthorized), "OTK consumida");
        assert_eq!(bad.check_trusted(&client(2).verifying_key().to_bytes()), Err(Unauthorized));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

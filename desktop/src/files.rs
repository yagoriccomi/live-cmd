//! C4 §2–§8 (arquivo único): frame de chunk, controles, caminhos, staging, finalização, expurgo e revogação.
//! Lógica pura com disco real: relógio (`now` em ms em cada chamada), `data_dir` e espaço livre/gravação injetáveis (`Env`).
//! Nunca registrar caminhos, nomes ou conteúdo (§6).
#![allow(dead_code)]
use crate::auth::{uuid_v4_lower, Error::{self, *}};
use crate::proto::{parse_u64, u64s, uuid_str};
use serde_json::{json, Map, Value};

type R<T> = Result<T, Error>;
pub type Fid = [u8; 16];
pub const FHDR: usize = 65;
pub const FRAG: usize = 16384;
pub const CHUNK: u64 = 65536;
const MAX_MSG: usize = 65536;

/// Um fragmento (§3). `data.len() == payload_len`.
#[derive(Debug, Clone, PartialEq)]
pub struct Frag { pub file_id: Fid, pub index: u32, pub offset: u32, pub chunk_len: u32, pub sha: [u8; 32], pub data: Vec<u8> }

/// Regras estruturais do frame; as que dependem da transferência ficam em `Files::on_frame`.
pub fn parse_frag(b: &[u8]) -> R<Frag> {
    if b.is_empty() { return Err(InvalidMessage); }
    if b[0] != 1 { return Err(VersionUnsupported); }
    if b.len() < FHDR + 1 || b.len() > FHDR + FRAG { return Err(InvalidMessage); }
    let u = |i: usize| u32::from_be_bytes(b[i..i + 4].try_into().unwrap());
    let (index, offset, chunk_len, plen) = (u(17), u(21), u(25), u(61) as usize);
    let ok = (1..=CHUNK as u32).contains(&chunk_len) && offset % FRAG as u32 == 0 && offset < chunk_len
        && plen == (chunk_len - offset).min(FRAG as u32) as usize && plen == b.len() - FHDR;
    if !ok { return Err(InvalidMessage); }
    Ok(Frag { file_id: b[1..17].try_into().unwrap(), index, offset, chunk_len, sha: b[29..61].try_into().unwrap(), data: b[FHDR..].to_vec() })
}

pub fn encode_frag(f: &Frag) -> Vec<u8> {
    let mut b = Vec::with_capacity(FHDR + f.data.len());
    b.push(1);
    b.extend(f.file_id);
    for n in [f.index, f.offset, f.chunk_len] { b.extend(n.to_be_bytes()); }
    b.extend(f.sha);
    b.extend((f.data.len() as u32).to_be_bytes());
    b.extend(&f.data);
    b
}

/// Fragmenta um chunk inteiro (remetente no download).
pub fn frags(file_id: Fid, index: u32, chunk: &[u8]) -> Vec<Vec<u8>> {
    let sha = sha(chunk);
    chunk.chunks(FRAG).enumerate().map(|(i, d)| encode_frag(&Frag { file_id, index, offset: (i * FRAG) as u32, chunk_len: chunk.len() as u32, sha, data: d.to_vec() })).collect()
}

pub fn sha(b: &[u8]) -> [u8; 32] { use sha2::Digest; sha2::Sha256::digest(b).into() }
pub fn total_chunks(size: u64) -> u64 { size.div_ceil(CHUNK) }
/// Tamanho exato do chunk `i` (§3).
pub fn chunk_len(size: u64, i: u32) -> u32 { (size - i as u64 * CHUNK).min(CHUNK) as u32 }
pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != 2 * N || !s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) { return None; }
    let mut o = [0; N];
    for (i, x) in o.iter_mut().enumerate() { *x = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?; }
    Some(o)
}

/// §6 `filename`: um componente; nunca corrigido.
pub fn valid_filename(s: &str) -> bool {
    const RES: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    let stem = s.split('.').next().unwrap_or("").to_uppercase();
    let numbered = ["COM", "LPT"].iter().any(|p| stem.strip_prefix(p).is_some_and(|d| matches!(d, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")));
    !matches!(s, "" | "." | "..") && s.encode_utf16().count() <= 255 && !s.ends_with(['.', ' '])
        && !s.chars().any(|c| c < ' ' || "/\\:<>\"|?*".contains(c))
        && !RES.contains(&stem.as_str()) && !numbered
}

/// §6 `target_dir`/`path`: absoluto (letra de unidade ou UNC), ≤ 4096 bytes, sem `.`/`..`, sem `\\.\` e `\\?\`.
pub fn valid_path(s: &str) -> bool {
    let p = s.replace('/', "\\");
    if s.len() > 4096 || s.contains('\0') || p.starts_with("\\\\.\\") || p.starts_with("\\\\?\\") { return false; }
    let b = p.as_bytes();
    let rest = if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' { &p[3..] }
        else if let Some(unc) = p.strip_prefix("\\\\") {
            // ponytail: UNC só validado na forma (servidor e compartilhamento não vazios); regras extras da §8B ficam para D4b.
            let mut it = unc.splitn(3, '\\');
            let (srv, share) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            if srv.is_empty() || share.is_empty() { return false; }
            it.next().unwrap_or("")
        } else { return false };
    !rest.split('\\').any(|c| c == "." || c == "..") && !rest.contains(':')
}

#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    FsList { path: Option<String>, session_id: Option<Fid>, cursor: Option<String> },
    UploadInit { file_id: Fid, session_id: Option<Fid>, filename: String, target_dir: Option<String>, size: u64, overwrite: bool, sha256: [u8; 32] },
    DownloadInit { file_id: Fid, path: String },
    Get { file_id: Fid, ranges: Vec<(u32, u32)> },
    Resume { file_id: Fid, cursor: Option<String> },
    Cancel(Fid),
}
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Request { request_id: String, req: Req },
    /// Espontâneos do cliente (destinatário no download).
    Ack { file_id: Fid, ranges: Vec<(u32, u32)> },
    Complete(Fid),
    /// Não fatal com `request_id` ou `file_id` (§1, D3).
    Error { request_id: Option<String>, file_id: Option<Fid>, code: Error },
}

fn ranges(v: Option<&Value>) -> R<Vec<(u32, u32)>> {
    let a = v.and_then(Value::as_array).filter(|a| a.len() <= 64).ok_or(InvalidMessage)?;
    let n = |x: &Value| x.as_u64().filter(|n| *n <= u32::MAX as u64).map(|n| n as u32).ok_or(InvalidMessage);
    a.iter().map(|r| match r.as_array().map(Vec::as_slice) { Some([s, e]) if n(s)? < n(e)? => Ok((n(s)?, n(e)?)), _ => Err(InvalidMessage) }).collect()
}

/// Controles C4 cliente→host (formato C3 §2). Qualquer violação: `INVALID_MESSAGE` (fatal).
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
    let id = |k: &str| uuid_v4_lower(s(k)?).ok_or(InvalidMessage);
    let opt_id = |k: &str| match o.get(k) { None | Some(Value::Null) => Ok(None), Some(_) => id(k).map(Some) };
    let opt_s = |k: &str| match o.get(k) { None | Some(Value::Null) => Ok(None), Some(_) => s(k).map(|x| Some(x.to_owned())) };
    let cursor = || opt_s("cursor").and_then(|c| if c.as_ref().is_some_and(|c| c.is_empty() || c.len() > 64) { Err(InvalidMessage) } else { Ok(c) });
    let rid = o.get("request_id").map(|v| v.as_str().filter(|r| uuid_v4_lower(r).is_some()).map(str::to_owned).ok_or(InvalidMessage)).transpose()?;
    let req = match s("type")? {
        "error" => {
            fields(&["code"], &["file_id"])?;
            return Ok(Control::Error { request_id: rid, file_id: opt_id("file_id")?, code: Error::from_code(s("code")?).ok_or(InvalidMessage)? });
        }
        "file_ack" if rid.is_none() => { fields(&["file_id", "ranges"], &[])?; return Ok(Control::Ack { file_id: id("file_id")?, ranges: ranges(o.get("ranges"))? }); }
        "file_complete" if rid.is_none() => { fields(&["file_id"], &[])?; return Ok(Control::Complete(id("file_id")?)); }
        "fs_list" => {
            fields(&["path"], &["cursor", "session_id"])?;
            let path = opt_s("path")?;
            if path.as_deref().is_some_and(|p| !valid_path(p)) { return Err(InvalidMessage); }
            Req::FsList { path, session_id: opt_id("session_id")?, cursor: cursor()? }
        }
        "file_upload_init" => {
            fields(&["file_id", "session_id", "filename", "target_dir", "size_bytes", "chunk_size", "total_chunks", "sha256", "overwrite"], &[])?;
            let size = parse_u64(&o["size_bytes"])?;
            let filename = s("filename")?.to_owned();
            let target_dir = opt_s("target_dir")?;
            let ok = o["chunk_size"].as_u64() == Some(CHUNK) && o["total_chunks"].as_u64() == Some(total_chunks(size)) && total_chunks(size) <= u32::MAX as u64
                && valid_filename(&filename) && target_dir.as_deref().is_none_or(valid_path);
            if !ok { return Err(InvalidMessage); }
            Req::UploadInit { file_id: id("file_id")?, session_id: opt_id("session_id")?, filename, target_dir, size,
                overwrite: o["overwrite"].as_bool().ok_or(InvalidMessage)?, sha256: unhex(s("sha256")?).ok_or(InvalidMessage)? }
        }
        "file_download_init" => {
            fields(&["file_id", "path"], &[])?;
            let path = s("path")?.to_owned();
            if !valid_path(&path) { return Err(InvalidMessage); }
            Req::DownloadInit { file_id: id("file_id")?, path }
        }
        "file_get" => { fields(&["file_id", "ranges"], &[])?; Req::Get { file_id: id("file_id")?, ranges: ranges(o.get("ranges"))? } }
        "file_resume" => { fields(&["file_id"], &["cursor"])?; Req::Resume { file_id: id("file_id")?, cursor: cursor()? } }
        "file_cancel" => { fields(&["file_id"], &[])?; Req::Cancel(id("file_id")?) }
        _ => return Err(InvalidMessage),
    };
    Ok(Control::Request { request_id: rid.ok_or(InvalidMessage)?, req })
}

/// `error` não fatal de uma transferência (D3).
pub fn file_error(code: Error, fid: &Fid) -> Value { serde_json::json!({"type":"error","code":code.code(),"file_id":crate::proto::uuid_str(fid)}) }
fn obj(v: Value) -> Map<String, Value> { match v { Value::Object(m) => m, _ => Map::new() } }

// ---- Windows ----
use std::{fs, io::{self, Read, Seek, SeekFrom, Write}, os::windows::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}, io::AsRawHandle}, path::{Path, PathBuf}, collections::{BTreeSet, HashMap}};
use windows::{core::PCWSTR, Win32::{Foundation::HANDLE, Storage::FileSystem::*}};

fn wide(p: &Path) -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() }
/// Volume serial + file index, seguindo links (ancestrais escolhidos pelo usuário são seguidos, §6).
pub fn identity(p: &Path) -> Option<(u32, u64)> {
    let f = fs::OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES.0).custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0).open(p).ok()?;
    let mut i = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut i) }.ok()?;
    Some((i.dwVolumeSerialNumber, ((i.nFileIndexHigh as u64) << 32) | i.nFileIndexLow as u64))
}
pub fn disk_free(p: &Path) -> Option<u64> {
    let mut a = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide(p).as_ptr()), Some(&mut a), None, None) }.ok()?;
    Some(a)
}
fn same_volume(a: &Path, b: &Path) -> bool { matches!((identity(a), identity(b)), (Some(x), Some(y)) if x.0 == y.0) }
/// Rename sem cópia entre volumes; sem `replace` falha se o destino existir (inclusive link).
fn move_file(a: &Path, b: &Path, replace: bool) -> io::Result<()> {
    let f = if replace { MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH } else { MOVEFILE_WRITE_THROUGH };
    unsafe { MoveFileExW(PCWSTR(wide(a).as_ptr()), PCWSTR(wide(b).as_ptr()), f) }.map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xffff))
}
/// Criação exclusiva sem seguir reparse point no nome final (§6).
fn create_new(p: &Path) -> io::Result<fs::File> { fs::OpenOptions::new().read(true).write(true).create_new(true).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0).open(p) }
fn is_link(m: &fs::Metadata) -> bool { m.file_type().is_symlink() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 }
/// Remove sem seguir links nem junctions: um link é removido como link.
fn rm(p: &Path) {
    let Ok(m) = fs::symlink_metadata(p) else { return };
    let _ = if is_link(&m) { fs::remove_dir(p).or_else(|_| fs::remove_file(p)) } else if m.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
}
/// Temporário + fsync + rename (§5).
fn write_atomic(p: &Path, data: &[u8]) -> io::Result<()> {
    let t = p.with_extension("tmp");
    let mut f = fs::File::create(&t)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    move_file(&t, p, true)
}
fn hash_file(f: &mut fs::File) -> io::Result<[u8; 32]> {
    use sha2::Digest;
    f.seek(SeekFrom::Start(0))?;
    let (mut h, mut b) = (sha2::Sha256::new(), vec![0; 1 << 20]);
    loop { let n = f.read(&mut b)?; if n == 0 { break; } h.update(&b[..n]); }
    Ok(h.finalize().into())
}
fn read_at(f: &mut fs::File, off: u64, n: usize) -> io::Result<Vec<u8>> { let mut b = vec![0; n]; f.seek(SeekFrom::Start(off))?; f.read_exact(&mut b)?; Ok(b) }
fn mtime_ms(m: &fs::Metadata) -> u64 { m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64) }
fn io_code(e: &io::Error) -> Error { match e.kind() { io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => NotFound, io::ErrorKind::PermissionDenied => Unauthorized, _ => IoError } }

const GIB: u64 = 1 << 30;
/// §7. ponytail: valores fixos no código; tornar configuráveis quando o host tiver arquivo de configuração.
#[derive(Debug, Clone)]
pub struct Limits { pub max_file: u64, pub per_conn: usize, pub per_host: usize, pub staging_total: u64, pub dest_margin: u64, pub staging_floor: u64, pub window: usize, pub get_pending: usize, pub write_queue: usize, pub commit_chunks: usize, pub commit_ms: u64, pub ttl_ms: u64, pub cursor_ms: u64, pub cancel_grace_ms: u64 }
impl Default for Limits {
    fn default() -> Self {
        Limits { max_file: 64 * GIB, per_conn: 4, per_host: 8, staging_total: 100 * GIB, dest_margin: GIB, staging_floor: 5 * GIB, window: 32, get_pending: 32, write_queue: 8,
            commit_chunks: 16, commit_ms: 100, ttl_ms: 24 * 3600 * 1000, cursor_ms: 60_000, cancel_grace_ms: 5000 }
    }
}
/// Dependências do disco injetáveis nos testes (N13, A5).
pub struct Env { pub free: fn(&Path) -> Option<u64>, pub same_volume: fn(&Path, &Path) -> bool, pub fail_io: bool }
impl Default for Env { fn default() -> Self { Env { free: disk_free, same_volume, fail_io: false } } }

pub type Pk = [u8; 32];
/// Identifica uma PeerConnection autenticada (atribuído por quem chama).
pub type ConnId = u64;
pub const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Transferência. Upload: `dir` = `target_dir` canônico, `file` = `data.part`. Download: `dir` = origem canônica, `file` = handle do init.
#[derive(Default)]
struct T {
    owner: Pk, conn: Option<ConnId>, up: bool,
    filename: String, dir: PathBuf, dir_id: (u32, u64), overwrite: bool, mtime: u64,
    size: u64, total: u32, sha: [u8; 32], created: u64, last: u64,
    file: Option<fs::File>, bitmap: Vec<u8>, done: u32,
    /// Gravados e ainda sem `fsync`/bitmap (group commit).
    pending: Vec<u32>, since: u64,
    fails: HashMap<u32, u8>,
    reasm: Option<(u32, [u8; 32], Vec<u8>)>,
    /// Download: pedidos por `file_get` e enviados sem ACK.
    queue: BTreeSet<u32>, sent: BTreeSet<u32>,
}
impl T {
    fn has(&self, i: u32) -> bool { self.bitmap[i as usize / 8] >> (i % 8) & 1 == 1 }
    fn manifest(&self, fid: &Fid) -> Value {
        json!({"schema_version":1,"file_id":uuid_str(fid),"owner_client_pk":hex(&self.owner),"direction":if self.up {"upload"} else {"download"},
            "filename":self.filename,"path":self.dir.to_str(),"overwrite":self.overwrite,"size_bytes":u64s(self.size),"total_chunks":self.total,
            "sha256":hex(&self.sha),"created_at":u64s(self.created),"last_valid_chunk_at":u64s(self.last),
            "dir_volume":self.dir_id.0,"dir_index":u64s(self.dir_id.1),"mtime_ms":u64s(self.mtime)})
    }
    /// Download: a versão iniciada mudou (tamanho, mtime ou identidade; §4).
    fn changed(&self) -> bool {
        let m = fs::metadata(&self.dir).ok();
        !(m.is_some_and(|m| m.len() == self.size && mtime_ms(&m) == self.mtime) && identity(&self.dir) == Some(self.dir_id))
    }
}
fn load(p: &Path) -> Option<(Fid, T)> {
    if fs::symlink_metadata(p).ok().is_none_or(|m| is_link(&m) || !m.is_dir()) { return None; }
    let Ok(Value::Object(m)) = serde_json::from_slice(&fs::read(p.join("manifest.json")).ok()?) else { return None };
    let s = |k: &str| m.get(k).and_then(Value::as_str);
    let u = |k: &str| m.get(k).and_then(|v| parse_u64(v).ok());
    let fid = uuid_v4_lower(s("file_id")?)?;
    if m.get("schema_version")?.as_u64()? != 1 || p.file_name()?.to_str()? != uuid_str(&fid) { return None; }
    let up = match s("direction")? { "upload" => true, "download" => false, _ => return None };
    let size = u("size_bytes")?;
    let mut t = T { owner: unhex(s("owner_client_pk")?)?, up, filename: s("filename")?.into(), dir: s("path")?.into(),
        dir_id: (m.get("dir_volume")?.as_u64()? as u32, u("dir_index")?), overwrite: m.get("overwrite")?.as_bool()?, mtime: u("mtime_ms")?,
        size, total: total_chunks(size) as u32, sha: unhex(s("sha256")?)?, created: u("created_at")?, last: u("last_valid_chunk_at")?, ..T::default() };
    if up {
        t.bitmap = fs::read(p.join("bitmap")).ok().filter(|b| b.len() == (t.total as usize).div_ceil(8))?;
        t.done = (0..t.total).filter(|i| t.has(*i)).count() as u32;
        t.file = fs::OpenOptions::new().read(true).write(true).open(p.join("data.part")).ok().filter(|f| f.metadata().is_ok_and(|m| m.len() == size));
    } else { t.file = fs::File::open(&t.dir).ok(); }
    t.file.is_some().then_some((fid, t))
}
fn tmp_name(name: &str, fid: &Fid) -> String { format!(".{name}.livecmd-{}.part", uuid_str(fid)) }

/// `missing` (§2.4): até 1024 intervalos a partir de `from`; `next_cursor` = índice onde continuar.
pub fn missing(bitmap: &[u8], total: u32, from: u32) -> Value {
    let has = |i: u32| bitmap[i as usize / 8] >> (i % 8) & 1 == 1;
    let (mut r, mut i) = (vec![], from);
    while i < total {
        if has(i) { i += 1; continue; }
        if r.len() == 1024 { return json!({"ranges":r,"next_cursor":i.to_string()}); }
        let s = i;
        while i < total && !has(i) { i += 1; }
        r.push(json!([s, i]));
    }
    json!({"ranges":r,"next_cursor":null})
}
fn ack(fid: &Fid, mut idx: Vec<u32>) -> Value {
    idx.sort_unstable();
    let mut r: Vec<[u32; 2]> = vec![];
    for i in idx { match r.last_mut() { Some(l) if l[1] == i => l[1] += 1, _ => r.push([i, i + 1]) } }
    json!({"type":"file_ack","file_id":uuid_str(fid),"ranges":r})
}

/// O que o host envia no download, um chunk por vez.
#[derive(Debug, PartialEq)]
pub enum Pull { Frames(Fid, Vec<Vec<u8>>), Ctl(Value) }

pub struct Files {
    data_dir: PathBuf, pub lim: Limits, pub env: Env,
    t: HashMap<Fid, T>,
    /// Cursor de `fs_list` → (pasta, posição, expira_em).
    cursors: HashMap<String, (Option<PathBuf>, usize, u64)>,
    /// Cancelados/pausados: (conexão, último tráfego); frames em trânsito ignorados por `cancel_grace_ms` (§8).
    gone: HashMap<Fid, (ConnId, u64)>,
}
impl Files {
    /// Varre o staging (§5): manifesto ilegível ou versão desconhecida → pasta descartada; temporários órfãos listados removidos.
    pub fn open(data_dir: &Path, lim: Limits, env: Env, now: u64) -> Files {
        let mut f = Files { data_dir: data_dir.into(), lim, env, t: HashMap::new(), cursors: HashMap::new(), gone: HashMap::new() };
        let root = data_dir.join("staging");
        let _ = fs::create_dir_all(&root);
        for e in fs::read_dir(&root).into_iter().flatten().flatten() {
            match load(&e.path()) {
                Some((fid, t)) => { if t.up { rm(&t.dir.join(tmp_name(&t.filename, &fid))); } f.t.insert(fid, t); }
                None => rm(&e.path()),
            }
        }
        f.tick(now);
        f
    }
    fn staging(&self, fid: &Fid) -> PathBuf { self.data_dir.join("staging").join(uuid_str(fid)) }
    fn slots(&self, conn: ConnId) -> R<()> {
        let act = || self.t.values().filter(|t| t.conn.is_some());
        if act().filter(|t| t.conn == Some(conn)).count() >= self.lim.per_conn || act().count() >= self.lim.per_host { Err(LimitExceeded) } else { Ok(()) }
    }
    /// Pausa: fica retomável; frames em trânsito ignorados por `cancel_grace_ms`.
    fn detach(&mut self, fid: &Fid, now: u64) {
        let Some(t) = self.t.get_mut(fid) else { return };
        if let Some(c) = t.conn.take() { self.gone.insert(*fid, (c, now)); }
        (t.reasm, t.queue, t.sent) = (None, BTreeSet::new(), BTreeSet::new());
        t.pending.clear();
        t.fails.clear();
    }
    /// Cancela e apaga staging/manifesto.
    fn delete(&mut self, fid: &Fid, now: u64) {
        self.detach(fid, now);
        self.t.remove(fid);
        rm(&self.staging(fid));
    }
    fn persist(&self, fid: &Fid, t: &T) -> io::Result<()> {
        let st = self.staging(fid);
        fs::create_dir_all(&st)?;
        if t.up { write_atomic(&st.join("bitmap"), &t.bitmap)?; }
        write_atomic(&st.join("manifest.json"), t.manifest(fid).to_string().as_bytes())
    }
    /// Transferências conhecidas (testes e diagnóstico; sem nomes).
    pub fn len(&self) -> usize { self.t.len() }

    /// Pedido com `request_id`. `cwd`: cwd conhecido da `session_id` do pedido (resolvido por quem chama).
    /// Devolve a resposta (`Null` = sem resposta, `file_get` OK) e os espontâneos seguintes. `Err` = fatal.
    pub fn request(&mut self, conn: ConnId, pk: &Pk, req: Req, cwd: Option<&Path>, now: u64) -> R<(Value, Vec<Value>)> {
        let r = match req {
            Req::FsList { path, cursor, .. } => self.fs_list(path, cwd, cursor, now).map(|v| (v, vec![])),
            Req::UploadInit { file_id, filename, target_dir, size, overwrite, sha256, .. } =>
                self.upload_init(conn, pk, file_id, filename, target_dir.map(PathBuf::from).or(cwd.map(Into::into)), size, overwrite, sha256, now),
            Req::DownloadInit { file_id, path } => self.download_init(conn, pk, file_id, &path, now).map(|v| (v, vec![])),
            Req::Get { file_id, ranges } => self.get(conn, pk, file_id, &ranges).map(|_| (Value::Null, vec![])),
            Req::Resume { file_id, cursor } => self.resume(conn, pk, file_id, cursor, now).map(|v| (v, vec![])),
            Req::Cancel(fid) => match self.t.get(&fid) {
                Some(t) if t.owner == *pk => { self.delete(&fid, now); Ok((json!({"type":"file_cancel_ok","file_id":uuid_str(&fid)}), vec![])) }
                _ => Err(NotFound),
            },
        };
        match r { Err(e @ (InvalidMessage | VersionUnsupported)) => Err(e), Err(e) => Ok((crate::proto::error(e), vec![])), ok => ok }
    }

    fn fs_list(&mut self, path: Option<String>, cwd: Option<&Path>, cursor: Option<String>, now: u64) -> R<Value> {
        let (dir, skip) = match cursor {
            // ponytail: o cursor carrega a pasta; `path` do pedido com cursor é ignorado.
            Some(c) => { let (d, n, _) = self.cursors.remove(&c).filter(|x| x.2 > now).ok_or(NotFound)?; (d, n) }
            None => (path.map(PathBuf::from).or(cwd.map(Into::into)), 0),
        };
        let all: Vec<Value> = match &dir {
            None => {
                let mask = unsafe { GetLogicalDrives() };
                (0..26u8).filter(|i| mask >> i & 1 == 1).map(|i| json!({"name":format!("{}:\\", (b'A' + i) as char),"kind":"dir","mtime_ms":0,"hidden":false})).collect()
            }
            Some(d) => {
                // ponytail: cada página relê a pasta (ordenada) e pula `skip`; mudanças entre páginas podem deslocar a lista.
                let mut es: Vec<_> = fs::read_dir(d).map_err(|e| io_code(&e))?.flatten().collect();
                es.sort_by_key(|e| e.file_name());
                es.iter().skip(skip).filter_map(|e| {
                    let m = e.metadata().ok()?;
                    let kind = if is_link(&m) { "link" } else if m.is_dir() { "dir" } else if m.is_file() { "file" } else { "other" };
                    let mut v = json!({"name":e.file_name().to_str()?,"kind":kind,"mtime_ms":mtime_ms(&m),"hidden":m.file_attributes() & FILE_ATTRIBUTE_HIDDEN.0 != 0});
                    if kind == "file" { v["size_bytes"] = u64s(m.len()); }
                    Some(v)
                }).take(501).collect()
            }
        };
        let (mut page, mut bytes) = (vec![], 0);
        for v in &all {
            let n = v.to_string().len() + 1;
            if page.len() == 500 || bytes + n > 48 * 1024 { break; }
            bytes += n;
            page.push(v.clone());
        }
        let next = if page.len() < all.len() {
            let mut b = [0u8; 16];
            getrandom::getrandom(&mut b).map_err(|_| IoError)?;
            self.cursors.insert(hex(&b), (dir.clone(), skip + page.len(), now + self.lim.cursor_ms));
            Value::from(hex(&b))
        } else { Value::Null };
        Ok(json!({"type":"fs_list_ok","path":dir.as_ref().and_then(|d| d.to_str()),"entries":page,"next_cursor":next}))
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_init(&mut self, conn: ConnId, pk: &Pk, fid: Fid, filename: String, dir: Option<PathBuf>, size: u64, overwrite: bool, sha: [u8; 32], now: u64) -> R<(Value, Vec<Value>)> {
        if self.t.contains_key(&fid) || fs::symlink_metadata(self.staging(&fid)).is_ok() { return Err(InvalidMessage); }
        if size == 0 && hex(&sha) != EMPTY_SHA { return Err(InvalidMessage); }
        if size > self.lim.max_file { return Err(LimitExceeded); }
        self.slots(conn)?;
        let dir = fs::canonicalize(dir.ok_or(NotFound)?).map_err(|e| io_code(&e))?;
        if !dir.is_dir() { return Err(NotFound); }
        let dir_id = identity(&dir).ok_or(IoError)?;
        if !overwrite && fs::symlink_metadata(dir.join(&filename)).is_ok() { return Err(AlreadyExists); }
        let staged: u64 = self.t.values().filter(|t| t.up).map(|t| t.size).sum();
        let root = self.data_dir.join("staging");
        // ponytail: destino e staging no mesmo volume são checados separadamente, sem somar as duas reservas.
        let low = |p: &Path, need: u64| (self.env.free)(p).is_some_and(|f| f < need);
        if staged + size > self.lim.staging_total || low(&dir, size + self.lim.dest_margin) || low(&root, size + self.lim.staging_floor) { return Err(LimitExceeded); }
        let total = total_chunks(size) as u32;
        let t = T { owner: *pk, conn: Some(conn), up: true, filename, dir, dir_id, overwrite, size, total, sha, created: now, last: now, bitmap: vec![0; (total as usize).div_ceil(8)], ..T::default() };
        let ok = json!({"type":"file_upload_init_ok","file_id":uuid_str(&fid),"missing":missing(&t.bitmap, total, 0)});
        self.t.insert(fid, t);
        if size == 0 {
            // Vazio: finaliza no init (§2.3), sem staging.
            let v = self.finalize(&fid, now);
            self.t.remove(&fid);
            self.gone.remove(&fid);
            return match Error::from_code(v["code"].as_str().unwrap_or("")) { Some(e) => Err(e), None => Ok((ok, vec![v])) };
        }
        let st = self.staging(&fid);
        let r = fs::create_dir_all(&st).and_then(|_| create_new(&st.join("data.part"))).and_then(|f| { f.set_len(size)?; Ok(f) });
        let r = r.and_then(|f| { self.t.get_mut(&fid).unwrap().file = Some(f); self.persist(&fid, &self.t[&fid]) });
        if r.is_err() { self.t.remove(&fid); rm(&st); return Err(IoError); }
        Ok((ok, vec![]))
    }

    /// §5: hash total, identidade do diretório, temporário (outro volume) e rename sem/com substituição.
    fn finalize(&mut self, fid: &Fid, now: u64) -> Value {
        let (st, fail, same) = (self.staging(fid), self.env.fail_io, self.env.same_volume);
        let part = st.join("data.part");
        let t = self.t.get_mut(fid).unwrap();
        if t.size > 0 {
            match t.file.as_mut().map(hash_file) {
                Some(Ok(h)) if h == t.sha => {}
                Some(Ok(_)) => { self.delete(fid, now); return file_error(HashMismatch, fid); }
                _ => { self.detach(fid, now); return file_error(IoError, fid); }
            }
        }
        let code = 'f: {
            if identity(&t.dir) != Some(t.dir_id) { break 'f IoError; }
            let fin = t.dir.join(&t.filename);
            if !t.overwrite && fs::symlink_metadata(&fin).is_ok() { break 'f AlreadyExists; }
            let tmp = t.dir.join(tmp_name(&t.filename, fid));
            t.file = None;
            let r = (|| -> io::Result<PathBuf> {
                if fail { return Err(io::Error::other("injected")); }
                if t.size > 0 && same(&st, &t.dir) { return Ok(part.clone()); }
                let mut o = create_new(&tmp)?;
                if t.size > 0 { io::copy(&mut fs::File::open(&part)?, &mut o)?; }
                o.sync_all()?;
                Ok(tmp.clone())
            })().and_then(|src| move_file(&src, &fin, t.overwrite));
            match r {
                Ok(()) => { self.delete(fid, now); return json!({"type":"file_complete","file_id":uuid_str(fid)}); }
                Err(e) => {
                    rm(&tmp);
                    t.file = fs::OpenOptions::new().read(true).write(true).open(&part).ok();
                    // 80/183: ERROR_FILE_EXISTS / ERROR_ALREADY_EXISTS (corrida no rename sem substituir).
                    if matches!(e.raw_os_error(), Some(80 | 183)) { AlreadyExists } else { IoError }
                }
            }
        };
        self.detach(fid, now);
        file_error(code, fid)
    }

    fn download_init(&mut self, conn: ConnId, pk: &Pk, fid: Fid, path: &str, now: u64) -> R<Value> {
        if self.t.contains_key(&fid) || fs::symlink_metadata(self.staging(&fid)).is_ok() { return Err(InvalidMessage); }
        self.slots(conn)?;
        let p = fs::canonicalize(path).map_err(|e| io_code(&e))?;
        let m = fs::metadata(&p).map_err(|e| io_code(&e))?;
        if !m.is_file() { return Err(NotFound); }
        if m.len() > self.lim.max_file { return Err(LimitExceeded); }
        let mut f = fs::File::open(&p).map_err(|e| io_code(&e))?;
        // ponytail: hash síncrono no init; D4c move para o pool de disco (§7).
        let sha = hash_file(&mut f).map_err(|_| IoError)?;
        let t = T { owner: *pk, conn: Some(conn), filename: p.file_name().and_then(|n| n.to_str()).ok_or(NotFound)?.into(), dir_id: identity(&p).ok_or(IoError)?,
            mtime: mtime_ms(&m), size: m.len(), total: total_chunks(m.len()) as u32, sha, created: now, last: now, file: Some(f), dir: p, ..T::default() };
        if t.changed() { return Err(NotFound); }
        if self.persist(&fid, &t).is_err() { rm(&self.staging(&fid)); return Err(IoError); }
        let v = json!({"type":"file_download_init_ok","file_id":uuid_str(&fid),"filename":t.filename,"size_bytes":u64s(t.size),"chunk_size":CHUNK,"total_chunks":t.total,"sha256":hex(&sha)});
        self.t.insert(fid, t);
        Ok(v)
    }

    fn get(&mut self, conn: ConnId, pk: &Pk, fid: Fid, ranges: &[(u32, u32)]) -> R<()> {
        let max = self.lim.get_pending;
        let t = self.t.get_mut(&fid).filter(|t| !t.up && t.owner == *pk && t.conn == Some(conn)).ok_or(NotFound)?;
        if ranges.iter().any(|r| r.1 > t.total) { return Err(InvalidMessage); }
        if t.queue.len() + ranges.iter().map(|r| (r.1 - r.0) as usize).sum::<usize>() > max { return Err(LimitExceeded); }
        for r in ranges { t.queue.extend(r.0..r.1); }
        Ok(())
    }

    fn resume(&mut self, conn: ConnId, pk: &Pk, fid: Fid, cursor: Option<String>, now: u64) -> R<Value> {
        let from = cursor.map(|c| c.parse::<u32>().map_err(|_| InvalidMessage)).transpose()?.unwrap_or(0);
        let t = self.t.get(&fid).filter(|t| t.owner == *pk && now < t.last + self.lim.ttl_ms).ok_or(NotFound)?;
        if !t.up && t.changed() { self.delete(&fid, now); return Err(NotFound); }
        if t.conn != Some(conn) { self.slots(conn)?; }
        self.detach(&fid, now);
        self.gone.remove(&fid);
        let t = self.t.get_mut(&fid).unwrap();
        t.conn = Some(conn);
        // Download: o cliente sabe o que tem (§4); o host não guarda o que falta.
        let miss = if t.up { missing(&t.bitmap, t.total, from) } else { json!({"ranges":[],"next_cursor":null}) };
        Ok(json!({"type":"file_resume_ok","file_id":uuid_str(&fid),"direction":if t.up {"upload"} else {"download"},"filename":t.filename,
            "size_bytes":u64s(t.size),"total_chunks":t.total,"sha256":hex(&t.sha),"missing":miss}))
    }

    /// Frame binário (§3). `Err` = fatal; `Ok` = controles a enviar (`file_ack`, `file_complete`, `error` com `file_id`).
    pub fn on_frame(&mut self, conn: ConnId, b: &[u8], now: u64) -> R<Vec<Value>> {
        let f = parse_frag(b)?;
        if let Some(g) = self.gone.get_mut(&f.file_id).filter(|g| g.0 == conn) {
            if now <= g.1 + self.lim.cancel_grace_ms { g.1 = now; return Ok(vec![]); }
            return Err(InvalidMessage);
        }
        let window = self.lim.window;
        let t = self.t.get_mut(&f.file_id).filter(|t| t.up && t.conn == Some(conn)).ok_or(InvalidMessage)?;
        if f.index >= t.total || f.chunk_len != chunk_len(t.size, f.index) { return Err(InvalidMessage); }
        match &mut t.reasm {
            Some((i, s, buf)) if *i == f.index && *s == f.sha && buf.len() == f.offset as usize => buf.extend(&f.data),
            None if f.offset == 0 && t.pending.len() < window => t.reasm = Some((f.index, f.sha, f.data)),
            _ => return Err(InvalidMessage),
        }
        if t.reasm.as_ref().is_some_and(|r| r.2.len() < f.chunk_len as usize) { return Ok(vec![]); }
        let (i, s, data) = t.reasm.take().unwrap();
        Ok(self.chunk(f.file_id, i, s, data, now))
    }

    fn chunk(&mut self, fid: Fid, i: u32, s: [u8; 32], data: Vec<u8>, now: u64) -> Vec<Value> {
        let (fail, commit) = (self.env.fail_io, self.lim.commit_chunks);
        let t = self.t.get_mut(&fid).unwrap();
        if sha(&data) != s {
            let n = t.fails.entry(i).or_default();
            *n += 1;
            // Segunda falha no mesmo índice: cancela, staging preservado (§8).
            if *n >= 2 { self.detach(&fid, now); }
            return vec![file_error(HashMismatch, &fid)];
        }
        let off = i as u64 * CHUNK;
        if t.has(i) {
            if t.file.as_mut().and_then(|f| read_at(f, off, data.len()).ok()).is_some_and(|d| d == data) { return vec![ack(&fid, vec![i])]; }
            self.delete(&fid, now);
            return vec![file_error(HashMismatch, &fid)];
        }
        // ponytail: gravação síncrona; fila de 8 chunks e pool de disco (§7) ficam com D4c.
        let w = match (fail, t.file.as_mut()) {
            (false, Some(f)) => f.seek(SeekFrom::Start(off)).and_then(|_| f.write_all(&data)),
            _ => Err(io::Error::other("injected")),
        };
        if w.is_err() { self.detach(&fid, now); return vec![file_error(IoError, &fid)]; }
        if t.pending.is_empty() { t.since = now; }
        if !t.pending.contains(&i) { t.pending.push(i); }
        if t.pending.len() >= commit { self.commit(&fid, now) } else { vec![] }
    }

    /// Group commit (§5): `fsync` de `data.part`, bitmap atômico, depois `file_ack`; completo → finaliza.
    fn commit(&mut self, fid: &Fid, now: u64) -> Vec<Value> {
        let (st, fail) = (self.staging(fid), self.env.fail_io);
        let t = self.t.get_mut(fid).unwrap();
        let p = std::mem::take(&mut t.pending);
        if p.is_empty() { return vec![]; }
        let mut bm = t.bitmap.clone();
        for i in &p { bm[*i as usize / 8] |= 1 << (i % 8); }
        let r = match (fail, &t.file) { (false, Some(f)) => f.sync_all().and_then(|_| write_atomic(&st.join("bitmap"), &bm)), _ => Err(io::Error::other("injected")) };
        if r.is_err() { self.detach(fid, now); return vec![file_error(IoError, fid)]; }
        t.done += p.len() as u32;
        (t.bitmap, t.last) = (bm, now);
        let _ = write_atomic(&st.join("manifest.json"), t.manifest(fid).to_string().as_bytes());
        let mut out = vec![ack(fid, p)];
        if t.done == t.total { out.push(self.finalize(fid, now)); }
        out
    }

    /// Próximo chunk de download para `conn` (janela e mudança da origem conferidas antes de ler; §4).
    pub fn pull(&mut self, conn: ConnId, now: u64) -> Option<Pull> {
        let w = self.lim.window;
        let fid = *self.t.iter().find(|(_, t)| !t.up && t.conn == Some(conn) && !t.queue.is_empty() && t.sent.len() < w)?.0;
        let t = self.t.get_mut(&fid).unwrap();
        if t.changed() { self.delete(&fid, now); return Some(Pull::Ctl(file_error(NotFound, &fid))); }
        let i = t.queue.pop_first().unwrap();
        match t.file.as_mut().map(|f| read_at(f, i as u64 * CHUNK, chunk_len(t.size, i) as usize)) {
            Some(Ok(d)) => { t.sent.insert(i); t.last = now; Some(Pull::Frames(fid, frags(fid, i, &d))) }
            _ => { self.detach(&fid, now); Some(Pull::Ctl(file_error(IoError, &fid))) }
        }
    }
    /// `file_ack` do cliente no download: libera a janela.
    pub fn client_ack(&mut self, conn: ConnId, fid: &Fid, ranges: &[(u32, u32)], now: u64) {
        if let Some(t) = self.t.get_mut(fid).filter(|t| !t.up && t.conn == Some(conn)) {
            t.sent.retain(|i| !ranges.iter().any(|r| (r.0..r.1).contains(i)));
            t.last = now;
        }
    }
    /// `file_complete` do cliente: apaga o manifesto de download.
    pub fn client_complete(&mut self, conn: ConnId, fid: &Fid, now: u64) {
        if self.t.get(fid).is_some_and(|t| !t.up && t.conn == Some(conn)) { self.delete(fid, now); }
    }

    /// Chamar a cada ~100 ms: group commit por tempo, expurgo de 24 h, cursores e frames ignorados vencidos.
    pub fn tick(&mut self, now: u64) -> Vec<(ConnId, Value)> {
        let mut out = vec![];
        let due: Vec<_> = self.t.iter().filter(|(_, t)| !t.pending.is_empty() && now >= t.since + self.lim.commit_ms).map(|(k, t)| (*k, t.conn)).collect();
        for (fid, c) in due { out.extend(self.commit(&fid, now).into_iter().filter_map(|v| c.map(|c| (c, v)))); }
        let old: Vec<_> = self.t.iter().filter(|(_, t)| now >= t.last + self.lim.ttl_ms).map(|(k, _)| *k).collect();
        for fid in old { self.delete(&fid, now); }
        self.cursors.retain(|_, c| c.2 > now);
        let g = self.lim.cancel_grace_ms;
        self.gone.retain(|_, x| now <= x.1 + g);
        out
    }
    /// Queda da PeerConnection: pausa, não cancela (§8). Gravados sem commit contam como ausentes.
    pub fn disconnect(&mut self, conn: ConnId, now: u64) {
        let fids: Vec<_> = self.t.iter().filter(|(_, t)| t.conn == Some(conn)).map(|(k, _)| *k).collect();
        for fid in fids { self.detach(&fid, now); }
        self.gone.retain(|_, x| x.0 != conn);
    }
    /// Revogação (§8): cancela e apaga na hora staging e manifestos do dispositivo.
    pub fn revoke(&mut self, pk: &Pk, now: u64) {
        let fids: Vec<_> = self.t.iter().filter(|(_, t)| t.owner == *pk).map(|(k, _)| *k).collect();
        for fid in fids { self.delete(&fid, now); }
    }
}

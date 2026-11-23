//! IPC entre o app desktop e o daemon `live-cmd-termd` (dono das sessões PTY e do host do celular).
//!
//! Named pipe `\\.\pipe\live-cmd-termd-<SID>`, só deste usuário:
//! - DACL `D:P(A;;GA;;;<SID>)` no pipe e no arquivo do token;
//! - `PIPE_REJECT_REMOTE_CLIENTS` e `FILE_FLAG_FIRST_PIPE_INSTANCE` (segundo daemon não sobe);
//! - token aleatório de 32 bytes, novo a cada início do daemon, em `%LOCALAPPDATA%\live-cmd\termd\token`;
//! - o cliente confere que o processo servidor roda com o mesmo SID (anti-impostor).
//!
//! Mensagem = `u32` LE do tamanho + 1 byte de tipo + corpo. Tipo 0 = JSON, tipo 1 = frame C3 (`proto::encode_frame`).
//! Handshake: cliente manda `{"t":"hello","ipc_version","token"}`; token errado fecha sem resposta; o servidor
//! responde `{"t":"hello","ipc_version","pid","build"}`. Versão diferente só aceita `shutdown`.
//! Depois: `{"t":"req","body"}` (pedido C3; a resposta volta como `ctl` com o mesmo `request_id`),
//! frames de input/resize, e `{"t":"rpc","id","op","args"}` → `{"t":"rpc","id","ok"|"err"}`.
//! Do servidor: `{"t":"ctl","v"}`, frames de saída/fim e `{"t":"app","ev","data"}` (avisos do provedor do celular).
use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc as smpsc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::core::{HSTRING, PWSTR};

/// Sobe quando o formato das mensagens muda. App e daemon de versões diferentes: o app encerra o antigo.
pub const IPC_VERSION: u64 = 1;
/// Versão do crate que gerou o daemon (só para log e diagnóstico).
pub const BUILD: &str = env!("CARGO_PKG_VERSION");
const MAX_MSG: usize = 4 << 20;
const HELLO: Duration = Duration::from_secs(5);
const JSON: u8 = 0;
const FRAME: u8 = 1;

/// Onde app e daemon se encontram.
#[derive(Clone, Debug)]
pub struct Endpoint { pub pipe: String, pub token_file: PathBuf }
impl Endpoint {
    /// Pipe e token deste usuário do Windows.
    pub fn user() -> std::result::Result<Endpoint, String> {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("IO_ERROR")?;
        Ok(Endpoint { pipe: format!(r"\\.\pipe\live-cmd-termd-{}", user_sid()?), token_file: base.join("live-cmd").join("termd").join("token") })
    }
}

// ---------- segurança ----------

fn token_sid(process: HANDLE) -> std::result::Result<String, String> {
    unsafe {
        let mut tok = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut tok).map_err(|_| "IO_ERROR")?;
        let mut len = 0u32;
        let _ = GetTokenInformation(tok, TokenUser, None, 0, &mut len);
        // u64: TOKEN_USER começa com ponteiro; o buffer precisa do alinhamento dele.
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let r = GetTokenInformation(tok, TokenUser, Some(buf.as_mut_ptr().cast()), (buf.len() * 8) as u32, &mut len);
        let _ = CloseHandle(tok);
        r.map_err(|_| "IO_ERROR")?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s).map_err(|_| "IO_ERROR")?;
        let out = s.to_string().map_err(|_| "IO_ERROR".to_owned());
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}
/// SID do usuário deste processo (`S-1-5-21-…`).
pub fn user_sid() -> std::result::Result<String, String> { token_sid(unsafe { GetCurrentProcess() }) }
fn pid_sid(pid: u32) -> std::result::Result<String, String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|_| "IO_ERROR")?;
        let r = token_sid(h);
        let _ = CloseHandle(h);
        r
    }
}

/// Descritor "só o usuário atual, acesso total, sem herança" para o pipe e o arquivo do token.
struct Sa { sd: PSECURITY_DESCRIPTOR, attrs: SECURITY_ATTRIBUTES }
// O descritor é imutável depois de criado e só é liberado no Drop.
unsafe impl Send for Sa {}
unsafe impl Sync for Sa {}
impl Sa {
    fn user_only() -> std::result::Result<Sa, String> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", user_sid()?));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None) }.map_err(|_| "IO_ERROR")?;
        Ok(Sa { sd, attrs: SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() } })
    }
    fn ptr(&self) -> *mut core::ffi::c_void { &self.attrs as *const SECURITY_ATTRIBUTES as *mut _ }
}
impl Drop for Sa { fn drop(&mut self) { unsafe { let _ = LocalFree(Some(HLOCAL(self.sd.0))); } } }

/// Grava um token novo; o arquivo é recriado (CREATE_NEW) já com a DACL do usuário.
fn write_token(path: &Path, sa: &Sa) -> std::result::Result<String, String> {
    use windows::Win32::Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_MODE, WriteFile};
    let mut raw = [0u8; 32];
    getrandom::getrandom(&mut raw).map_err(|_| "IO_ERROR")?;
    let tok = files::hex(&raw);
    if let Some(d) = path.parent() { std::fs::create_dir_all(d).map_err(|_| "IO_ERROR")?; }
    let _ = std::fs::remove_file(path);
    unsafe {
        let h = CreateFileW(&HSTRING::from(path.as_os_str()), FILE_GENERIC_WRITE.0, FILE_SHARE_MODE(0), Some(&sa.attrs), CREATE_NEW, FILE_ATTRIBUTE_NORMAL, None).map_err(|_| "IO_ERROR")?;
        let r = WriteFile(h, Some(tok.as_bytes()), None, None);
        let _ = CloseHandle(h);
        r.map_err(|_| "IO_ERROR")?;
    }
    Ok(tok)
}
fn same(a: &str, b: &str) -> bool { a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0 }

// ---------- mensagens ----------

pub(crate) fn pack(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(5 + body.len());
    b.extend((body.len() as u32).to_le_bytes());
    b.push(kind);
    b.extend(body);
    b
}
pub(crate) fn pack_json(v: &Value) -> Vec<u8> { pack(JSON, v.to_string().as_bytes()) }
pub(crate) fn pack_frame(f: &proto::Frame) -> Vec<u8> { pack(FRAME, &proto::encode_frame(f)) }
pub(crate) enum Msg { Json(Value), Frame(proto::Frame) }
pub(crate) async fn read_msg<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Msg> {
    let bad = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let mut h = [0u8; 5];
    r.read_exact(&mut h).await?;
    let n = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
    if n > MAX_MSG { return Err(bad()); }
    let mut b = vec![0; n];
    r.read_exact(&mut b).await?;
    match h[4] {
        JSON => serde_json::from_slice(&b).map(Msg::Json).map_err(|_| bad()),
        FRAME => proto::parse_frame(&b).map(Msg::Frame).map_err(|_| bad()),
        _ => Err(bad()),
    }
}

// ---------- cliente ----------

/// Resposta do daemon ao `hello`.
#[derive(Clone, Debug)]
pub struct Hello { pub ipc_version: u64, pub pid: u32, pub build: String, pub app: String }
/// Versão do app que instalou este daemon: nome da pasta `daemon\<versão>\` do exe; vazio fora dela (debug).
pub fn app_tag() -> String {
    let Ok(exe) = std::env::current_exe() else { return String::new() };
    let dir = exe.parent();
    match (dir.and_then(|d| d.file_name()), dir.and_then(|d| d.parent()).and_then(|d| d.file_name())) {
        (Some(v), Some(p)) if p == "daemon" => v.to_string_lossy().into_owned(),
        _ => String::new(),
    }
}
/// Por que a conexão não abriu.
#[derive(Clone, Debug, PartialEq)]
pub enum Fail {
    /// Nenhum daemon escutando (ou sem token): subir um.
    Absent,
    /// Daemon de outro `ipc_version`: encerrar e subir o desta versão.
    Version(u64),
    /// Token recusado ou servidor de outro usuário.
    Denied,
    Io,
}
pub(crate) struct Conn { pub pipe: tokio::net::windows::named_pipe::NamedPipeClient, pub hello: Hello }

/// Runtime das conexões do lado do app (uma para o processo inteiro).
pub(crate) fn rt() -> &'static tokio::runtime::Runtime {
    static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("live-cmd-ipc").enable_all().build().expect("runtime IPC"));
    &RT
}
/// Roda `f` no runtime IPC e espera, de qualquer thread fora dele.
pub(crate) fn block<T: Send + 'static>(f: impl Future<Output = T> + Send + 'static) -> T {
    let (tx, rx) = smpsc::channel();
    rt().spawn(async move { let _ = tx.send(f.await); });
    rx.recv().expect("tarefa IPC")
}

/// Abre o pipe, confere o dono do servidor e faz o handshake. `allow_old`: aceita versão diferente (para `shutdown`).
pub(crate) async fn connect(ep: &Endpoint, allow_old: bool) -> std::result::Result<Conn, Fail> {
    use std::os::windows::io::AsRawHandle;
    let mut tries = 0;
    let mut pipe = loop {
        match ClientOptions::new().open(&ep.pipe) {
            Ok(p) => break p,
            // ERROR_PIPE_BUSY: todas as instâncias ocupadas por um instante.
            Err(e) if e.raw_os_error() == Some(231) && tries < 20 => { tries += 1; tokio::time::sleep(Duration::from_millis(50)).await; }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Fail::Absent),
            Err(_) => return Err(Fail::Io),
        }
    };
    let mut pid = 0u32;
    unsafe { windows::Win32::System::Pipes::GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid) }.map_err(|_| Fail::Io)?;
    if pid_sid(pid).ok() != user_sid().ok() { return Err(Fail::Denied); }
    let token = std::fs::read_to_string(&ep.token_file).map_err(|_| Fail::Absent)?;
    pipe.write_all(&pack_json(&json!({"t":"hello","ipc_version":IPC_VERSION,"token":token.trim()}))).await.map_err(|_| Fail::Io)?;
    let v = match tokio::time::timeout(HELLO, read_msg(&mut pipe)).await {
        Ok(Ok(Msg::Json(v))) if v["t"] == "hello" => v,
        Ok(Err(_)) => return Err(Fail::Denied),
        _ => return Err(Fail::Io),
    };
    let hello = Hello { ipc_version: v["ipc_version"].as_u64().unwrap_or(0), pid, build: v["build"].as_str().unwrap_or("").into(), app: v["app"].as_str().unwrap_or("").into() };
    if hello.ipc_version != IPC_VERSION && !allow_old { return Err(Fail::Version(hello.ipc_version)); }
    Ok(Conn { pipe, hello })
}

/// Confere se há daemon compatível escutando.
pub fn probe(ep: &Endpoint) -> std::result::Result<Hello, Fail> {
    let ep = ep.clone();
    block(async move { connect(&ep, false).await.map(|c| c.hello) })
}

/// Uma chamada de controle numa conexão curta. `allow_old` só para `shutdown`.
async fn call(ep: Endpoint, op: String, args: Value, allow_old: bool) -> std::result::Result<Value, String> {
    let mut c = connect(&ep, allow_old).await.map_err(|e| match e { Fail::Absent => "NOT_RUNNING".to_owned(), _ => "NOT_CONNECTED".to_owned() })?;
    c.pipe.write_all(&pack_json(&json!({"t":"rpc","id":1,"op":op,"args":args}))).await.map_err(|_| "NOT_CONNECTED")?;
    loop {
        match tokio::time::timeout(Duration::from_secs(15), read_msg(&mut c.pipe)).await {
            Ok(Ok(Msg::Json(v))) if v["t"] == "rpc" => {
                return match v.get("err").and_then(Value::as_str) { Some(e) => Err(e.into()), None => Ok(v["ok"].clone()) };
            }
            Ok(Ok(_)) => {}
            // `shutdown`: o daemon pode sair antes de a resposta chegar.
            Ok(Err(_)) if op == "shutdown" => return Ok(Value::Null),
            _ => return Err("NOT_CONNECTED".into()),
        }
    }
}

/// O [`Service`] que mora no daemon, visto do app. Cada chamada abre uma conexão curta.
#[derive(Clone, Debug)]
pub struct ServiceClient { pub ep: Endpoint }
impl ServiceClient {
    pub fn new(ep: Endpoint) -> ServiceClient { ServiceClient { ep } }
    pub fn rpc(&self, op: &str, args: Value) -> std::result::Result<Value, String> { block(call(self.ep.clone(), op.into(), args, false)) }
    /// Daemon fora do ar conta como "conectando" (o app vai subir outro).
    pub fn status(&self) -> Status {
        self.rpc("status", Value::Null).ok().and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or(Status { state: State::Connecting, connected: 0, pairing_expires_ms: None })
    }
    pub fn pair(&self) -> std::result::Result<String, String> { self.str("pair", Value::Null) }
    pub fn pair_with(&self, server: &str) -> std::result::Result<String, String> { self.str("pair", json!({"server":server})) }
    pub fn cancel_pair(&self) { let _ = self.rpc("cancel_pair", Value::Null); }
    pub fn devices(&self) -> Vec<Device> { self.rpc("devices", Value::Null).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default() }
    pub fn revoke(&self, client_pk: [u8; 32]) -> std::result::Result<(), String> { self.rpc("revoke", json!({"client_pk":client_pk})).map(|_| ()) }
    /// Para o host e sobe de novo lendo as configurações do disco (servidor, perfis).
    pub fn restart(&self) -> std::result::Result<(), String> { self.rpc("restart", Value::Null).map(|_| ()) }
    /// Encerra o daemon e, com ele, todas as sessões. Aceita daemon de outra versão.
    pub fn shutdown(&self) -> std::result::Result<(), String> { block(call(self.ep.clone(), "shutdown".into(), Value::Null, true)).map(|_| ()) }
    fn str(&self, op: &str, args: Value) -> std::result::Result<String, String> {
        self.rpc(op, args)?.as_str().map(str::to_owned).ok_or_else(|| "INVALID_MESSAGE".into())
    }

    /// Avisos do provedor do celular (`appearance`, `agent-waiting`…) enquanto o processo viver.
    /// Reconecta sozinho; avisos durante a queda se perdem.
    pub fn watch(&self, on: impl Fn(&str, Value) + Send + Sync + 'static) {
        let ep = self.ep.clone();
        rt().spawn(async move {
            loop {
                if let Ok(mut c) = connect(&ep, false).await {
                    while let Ok(m) = read_msg(&mut c.pipe).await {
                        if let Msg::Json(v) = m && v["t"] == "app" { on(v["ev"].as_str().unwrap_or(""), v["data"].clone()); }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }
}

// ---------- servidor ----------

/// O que o servidor do pipe precisa do daemon.
pub trait Daemon: Send + Sync + 'static {
    /// Conexão C3 local nova (uma por conexão do pipe).
    fn local(&self, on: Box<dyn Fn(ClientEvent) + Send + Sync>) -> std::result::Result<Client, String>;
    /// `status`, `pair`, `cancel_pair`, `devices`, `revoke`, `restart`.
    fn rpc(&self, op: &str, args: &Value) -> std::result::Result<Value, String>;
}

/// [`Daemon`] em cima de um [`Service`]. `restart` monta um novo a partir do disco.
pub struct ServiceDaemon {
    svc: Mutex<Option<Service>>,
    restart: Box<dyn Fn() -> std::result::Result<Service, String> + Send + Sync>,
}
impl ServiceDaemon {
    pub fn new(svc: Option<Service>, restart: impl Fn() -> std::result::Result<Service, String> + Send + Sync + 'static) -> ServiceDaemon {
        ServiceDaemon { svc: Mutex::new(svc), restart: Box::new(restart) }
    }
    pub fn stop(&self) { if let Some(s) = self.svc.lock().unwrap().take() { s.stop(); } }
}
impl Daemon for ServiceDaemon {
    fn local(&self, on: Box<dyn Fn(ClientEvent) + Send + Sync>) -> std::result::Result<Client, String> {
        let g = self.svc.lock().unwrap();
        Ok(Client::local(g.as_ref().ok_or("NOT_RUNNING")?, on))
    }
    fn rpc(&self, op: &str, a: &Value) -> std::result::Result<Value, String> {
        if op == "restart" {
            let mut g = self.svc.lock().unwrap();
            if let Some(s) = g.take() { s.stop(); }
            *g = Some((self.restart)()?);
            return Ok(Value::Null);
        }
        let g = self.svc.lock().unwrap();
        let Some(s) = g.as_ref() else {
            return if op == "status" { Ok(json!(Status { state: State::Error, connected: 0, pairing_expires_ms: None })) } else { Err("NOT_RUNNING".into()) };
        };
        Ok(match op {
            "status" => json!(s.status()),
            "pair" => json!(match a["server"].as_str() { Some(srv) => s.pair_with(srv)?, None => s.pair()? }),
            "cancel_pair" => { s.cancel_pair(); Value::Null }
            "devices" => json!(s.devices()),
            "revoke" => { s.revoke(serde_json::from_value(a["client_pk"].clone()).map_err(|_| "INVALID_MESSAGE")?)?; Value::Null }
            _ => return Err("INVALID_MESSAGE".into()),
        })
    }
}

/// Canal dos avisos `{"t":"app"}` para todas as conexões abertas.
#[derive(Clone)]
pub struct Events(tokio::sync::broadcast::Sender<Vec<u8>>);
impl Default for Events { fn default() -> Self { Events(tokio::sync::broadcast::channel(64).0) } }
impl Events {
    pub fn send(&self, ev: &str, data: Value) { let _ = self.0.send(pack_json(&json!({"t":"app","ev":ev,"data":data}))); }
}

/// Servidor do pipe pronto para aceitar (o pipe já existe e o token já foi gravado).
pub struct Server { ep: Endpoint, sa: Arc<Sa>, first: NamedPipeServer, token: Arc<String>, rt: tokio::runtime::Runtime }
impl Server {
    /// Cria a primeira instância do pipe e grava o token. `ALREADY_RUNNING` se outro daemon tem o pipe.
    pub fn bind(ep: &Endpoint) -> std::result::Result<Server, String> {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("live-cmd-termd").enable_all().build().map_err(|_| "IO_ERROR")?;
        let sa = Arc::new(Sa::user_only()?);
        let first = { let _g = rt.enter(); instance(&ep.pipe, &sa, true).map_err(|_| "ALREADY_RUNNING")? };
        let token = Arc::new(write_token(&ep.token_file, &sa)?);
        Ok(Server { ep: ep.clone(), sa, first, token, rt })
    }

    /// Atende até um `shutdown` chegar pelo pipe.
    pub fn run(self, d: Arc<dyn Daemon>, events: Events) {
        let Server { ep, sa, first, token, rt } = self;
        let (quit, mut quitting) = tokio::sync::watch::channel(false);
        rt.block_on(async move {
            let mut server = first;
            loop {
                tokio::select! {
                    r = server.connect() => {
                        let next = match instance(&ep.pipe, &sa, false) { Ok(n) => n, Err(_) => { tokio::time::sleep(Duration::from_millis(100)).await; continue } };
                        let conn = std::mem::replace(&mut server, next);
                        if r.is_ok() { tokio::spawn(serve_conn(conn, d.clone(), token.clone(), events.clone(), quit.clone())); }
                    }
                    _ = quitting.changed() => break,
                }
            }
        });
        // Desliga as tarefas das conexões sem esperar o cliente fechar.
        rt.shutdown_timeout(Duration::from_secs(1));
        let _ = std::fs::remove_file(&ep.token_file);
    }
}

fn instance(name: &str, sa: &Sa, first: bool) -> std::io::Result<NamedPipeServer> {
    let mut o = ServerOptions::new();
    o.first_pipe_instance(first).reject_remote_clients(true);
    unsafe { o.create_with_security_attributes_raw(name, sa.ptr()) }
}

async fn serve_conn(pipe: NamedPipeServer, d: Arc<dyn Daemon>, token: Arc<String>, events: Events, quit: tokio::sync::watch::Sender<bool>) {
    let (mut r, mut w) = tokio::io::split(pipe);
    let hello = match tokio::time::timeout(HELLO, read_msg(&mut r)).await { Ok(Ok(Msg::Json(v))) if v["t"] == "hello" => v, _ => return };
    if !same(hello["token"].as_str().unwrap_or(""), &token) { return; }
    let current = hello["ipc_version"].as_u64() == Some(IPC_VERSION);
    if w.write_all(&pack_json(&json!({"t":"hello","ipc_version":IPC_VERSION,"pid":std::process::id(),"build":BUILD,"app":app_tag()}))).await.is_err() { return; }

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut app = events.0.subscribe();
    let writer = tokio::spawn(async move {
        loop {
            let b = tokio::select! {
                m = rx.recv() => match m { Some(b) => b, None => break },
                e = app.recv() => match e { Ok(b) => b, Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, Err(_) => break },
            };
            if w.write_all(&b).await.is_err() { break; }
        }
    });

    // ponytail: o crédito de fluxo do hub é devolvido assim que a saída entra na fila do pipe;
    // app travado acumula saída em memória. Ligar o crédito à escrita no pipe se isso aparecer.
    let mut local: Option<Arc<Client>> = None;
    let open = |local: &mut Option<Arc<Client>>| -> Option<Arc<Client>> {
        if local.is_none() {
            let tx = tx.clone();
            let on = move |e: ClientEvent| {
                let _ = match e {
                    ClientEvent::Control(v) => tx.send(pack_json(&json!({"t":"ctl","v":v}))),
                    ClientEvent::Output { session_id, offset, data } => {
                        let Some(sid) = auth::uuid_bytes(&session_id) else { return };
                        let mut off = offset;
                        for c in data.chunks(proto::MAX_IO) {
                            let _ = tx.send(pack_frame(&proto::Frame { session_id: sid, seq: off, op: Op::Output(c.to_vec()) }));
                            off += c.len() as u64;
                        }
                        Ok(())
                    }
                    ClientEvent::Exited { session_id, code } => match auth::uuid_bytes(&session_id) {
                        Some(sid) => tx.send(pack_frame(&proto::Frame { session_id: sid, seq: 0, op: Op::Exited(code) })),
                        None => Ok(()),
                    },
                    _ => Ok(()),
                };
            };
            *local = d.local(Box::new(on)).ok().map(Arc::new);
        }
        local.clone()
    };

    while let Ok(m) = read_msg(&mut r).await {
        match m {
            Msg::Json(v) if v["t"] == "rpc" => {
                let (id, op) = (v["id"].clone(), v["op"].as_str().unwrap_or("").to_owned());
                if op == "shutdown" {
                    let _ = tx.send(pack_json(&json!({"t":"rpc","id":id,"ok":null})));
                    let _ = quit.send(true);
                    continue;
                }
                if !current { let _ = tx.send(pack_json(&json!({"t":"rpc","id":id,"err":"VERSION_UNSUPPORTED"}))); continue; }
                let (d, tx) = (d.clone(), tx.clone());
                tokio::task::spawn_blocking(move || {
                    let m = match d.rpc(&op, &v["args"]) { Ok(ok) => json!({"t":"rpc","id":id,"ok":ok}), Err(e) => json!({"t":"rpc","id":id,"err":e}) };
                    let _ = tx.send(pack_json(&m));
                });
            }
            Msg::Json(v) if v["t"] == "req" && current => {
                let mut body = v["body"].clone();
                let rid = body.as_object_mut().and_then(|o| o.remove("request_id")).unwrap_or(Value::Null);
                let (c, tx) = (open(&mut local), tx.clone());
                tokio::task::spawn_blocking(move || {
                    let mut reply = match c { Some(c) => c.request(body).unwrap_or_else(|code| json!({"type":"error","code":code})), None => json!({"type":"error","code":"NOT_RUNNING"}) };
                    reply["request_id"] = rid;
                    let _ = tx.send(pack_json(&json!({"t":"ctl","v":reply})));
                });
            }
            Msg::Frame(f) if current => {
                let Some(c) = open(&mut local) else { continue };
                let sid = proto::uuid_str(&f.session_id);
                let _ = match f.op { Op::Input(d) => c.input(&sid, &d), Op::Resize { cols, rows } => c.resize(&sid, cols, rows), _ => Ok(()) };
            }
            _ => {}
        }
    }
    // Fim da conexão: soltar o cliente local desanexa as sessões (elas continuam vivas no hub).
    drop(local);
    writer.abort();
}

/// Conexão do pipe usada por [`Client::pipe`]: status de "no ar" e fila de saída.
pub(crate) struct PipeLink { pub tx: mpsc::UnboundedSender<Vec<u8>>, pub up: Arc<AtomicBool> }

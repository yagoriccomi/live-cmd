//! Host live-cmd como biblioteca: o binário (`main.rs`) e o app de bandeja (D5b) usam [`Service`].
mod auth;
mod client;
mod conpty;
pub mod appdata;
pub mod extras;
pub mod ipc;
mod files;
mod proto;
mod screen;
mod session;
pub mod stats;
pub mod ssh;
pub use client::{Client, ClientEvent, RemotePin, parse_pair_link};
pub use screen::cursor_line;
pub use proto::{AGENTS, valid_args};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::{atomic::{AtomicBool, AtomicU64, Ordering::Relaxed}, Arc, LazyLock, Mutex, MutexGuard, OnceLock}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};
use auth::{Binding, Host, HostHandshake, Msg};
use proto::{Control, Focus, Op, Req};
use session::{ConnId, Out, Pk, Sid};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ed25519_dalek::SigningKey;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use webrtc::{data_channel::{DataChannel, DataChannelEvent}, peer_connection::*};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

enum Event { Ice(RTCPeerConnectionIceEvent), Channel(Arc<dyn DataChannel>), Connected, Failed }
struct Handler { tx: mpsc::Sender<Event>, failed: tokio::sync::Notify }
impl Handler {
    fn emit(&self, event: Event) { if self.tx.try_send(event).is_err() { self.failed.notify_one(); } }
}
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_candidate(&self, e: RTCPeerConnectionIceEvent) { self.emit(Event::Ice(e)); }
    async fn on_data_channel(&self, c: Arc<dyn DataChannel>) { self.emit(Event::Channel(c)); }
    async fn on_connection_state_change(&self, s: RTCPeerConnectionState) {
        if matches!(s, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) { self.emit(Event::Failed); }
        if s == RTCPeerConnectionState::Connected { self.emit(Event::Connected); }
    }
}
#[cfg(test)]
fn uuid(s: &str) -> bool { s.len() == 36 && s.bytes().enumerate().all(|(i,b)| if [8,13,18,23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }) }
/// Envelope C1 sem conferir `connection_id` (antes do `ready` ele é `null`).
fn envelope_any(text: &str) -> Result<Value> {
    if text.len() > 65536 { return Err("signal limit".into()); }
    let m: Value = serde_json::from_str(text)?;
    let request = m["request_id"].as_str().ok_or("request")?;
    if m["v"] != 1 || !m["payload"].is_object() || request.is_empty() || request.len()>128 || !request.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) { return Err("envelope".into()); }
    Ok(m)
}
#[cfg(test)]
fn envelope(text: &str, id: &str) -> Result<Value> {
    let m = envelope_any(text)?;
    if m["connection_id"] != id { return Err("envelope".into()); }
    Ok(m)
}
fn message(kind: &str, payload: Value, id: &str) -> String { json!({"v":1,"type":kind,"connection_id":id,"request_id":"host","payload":payload}).to_string() }
fn hex32(s: &str) -> Option<[u8; 32]> {
    s.split(':').map(|h| u8::from_str_radix(h, 16).ok()).collect::<Option<Vec<u8>>>()?.try_into().ok()
}
fn sdp_fingerprint(fp: &[u8; 32]) -> String { format!("a=fingerprint:sha-256 {}", fp.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")) }
/// Certificado DTLS gerado pelo processo e seu fingerprint sha-256 (C2 §4).
fn certificate() -> Result<(RTCCertificate, [u8; 32])> {
    let provider = crypto::default_provider()?;
    let cert = RTCCertificate::generate(provider.crypto(), crypto::SignatureScheme::EcdsaP256Sha256, rcgen::CertificateParams::new(vec!["live-cmd".to_owned()])?)?;
    let fp = cert.get_fingerprints(provider.crypto())?.into_iter().find(|f| f.algorithm == "sha-256").and_then(|f| hex32(&f.value)).ok_or("fingerprint")?;
    Ok((cert, fp))
}
/// SHA-256 do certificado DTLS remoto, lido do transporte após conectar (C2 §4). Vazio → erro.
async fn remote_fingerprint(pc: &impl PeerConnection) -> Result<[u8; 32]> {
    let certs = pc.sctp().await.ok_or("sctp")?.transport().get_remote_certificates().await?;
    Ok(Sha256::digest(certs.first().ok_or("no remote certificate")?).into())
}
fn peer(cert: RTCCertificate) -> PeerConnectionBuilder<&'static str> {
    // Acima de PAUSE o envio de saída já para (C3 §6); o limite só evita `send` bloqueante no loop.
    PeerConnectionBuilder::new()
        // ponytail: só STUN; NAT simétrico precisa de TURN (relay), adicionar quando aparecer.
        .with_configuration(RTCConfigurationBuilder::new().with_certificates(vec![cert])
            .with_ice_servers(vec![RTCIceServer { urls: vec!["stun:stun.cloudflare.com:3478".to_owned(), "stun:stun.l.google.com:19302".to_owned()], ..Default::default() }]).build())
        .with_udp_addrs(vec!["0.0.0.0:0", "127.0.0.1:0"]).with_data_channel_send_buffer_limit(PAUSE + (1 << 20))
}

// ---- D3c: pty-channel após AUTHENTICATED (C3) ----
const PAUSE: usize = 1 << 20;
const RESUME: u32 = 256 << 10;
#[derive(Clone)]
struct Profile { label: String, command: String, osc: bool }
/// Estado do processo: sessões vivem além da conexão; a saída do PTY chega por `conns`.
#[derive(Default)]
struct Hub { s: session::Sessions, screens: HashMap<Sid, screen::Screen>, /// Tela em `buffer_start`: é a do `screen_snapshot` (§5); a ao vivo só gera eventos.
    base: HashMap<Sid, screen::Screen>, plain: HashSet<Sid>, conns: HashMap<ConnId, mpsc::UnboundedSender<Out>>,
    /// Aparelho de cada conexão autenticada (`Status.connected`, `Device.connected`).
    peers: HashMap<ConnId, Pk>, profiles: Vec<(String, Profile)>,
    /// Último cwd anunciado por OSC 7 (destino padrão do C4, §2.3); na criação, o `cwd` pedido.
    cwds: HashMap<Sid, PathBuf>,
    /// Perfil de cada sessão, para recriar do disco.
    prof: HashMap<Sid, String>,
    /// Agente de IA da sessão: (cli, args extras).
    ai: HashMap<Sid, (String, String)>,
    /// Destino das sessões SSH (tela Servidores SSH), para recriar do disco.
    ssh: HashMap<Sid, ssh::Ssh>,
    /// `sessions.json` (só no serviço de verdade; testes ficam sem disco) e se mudou desde a última gravação.
    store: Option<PathBuf>, dirty: bool,
    /// Agentes parados num pedido de confirmação e conexões inscritas no evento `agent_waiting` (`agent_list` com `ext`).
    waiting: HashSet<Sid>, waiting_subs: HashSet<ConnId>,
    /// "Reconectar ao cair": tentativas seguidas e início da conexão atual de cada sessão SSH.
    retry: HashMap<Sid, (u32, u64)>,
    /// Saída de restauração (SSH adormecido): não dispara a reconexão automática.
    quiet_exit: bool }
/// Reconexão automática: até 6 tentativas seguidas, esperando 2, 4, 8, 16, 30 e 30 s. Conexão que durou 60 s zera a conta.
const RETRY_MAX: u32 = 6;
fn retry_delay_ms(n: u32) -> u64 { (2u64 << n.min(4)).min(30) * 1000 }
/// Fuso local em minutos a somar ao UTC (resumo do Histórico por dia).
// ponytail: usa o fuso de agora para todos os dias; um dia do outro lado do horário de verão pode cair 1 h deslocado.
pub fn local_offset_min() -> i32 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
    -bias
}
static HUB: LazyLock<Mutex<Hub>> = LazyLock::new(Default::default);
static NEXT_CONN: AtomicU64 = AtomicU64::new(1);
fn hub() -> MutexGuard<'static, Hub> { HUB.lock().unwrap_or_else(|e| e.into_inner()) }
/// DPAPI do usuário: só esta conta do Windows decifra a saída salva em `sessions.json`.
fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if protect { CryptProtectData(&input, None, None, None, None, 0, &mut out) } else { CryptUnprotectData(&input, None, None, None, None, 0, &mut out) }.ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        Some(v)
    }
}
fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }
impl Hub {
    /// Abre o PTY do perfil e registra a sessão (`sid` dado = restauração do disco).
    fn open(&mut self, sid: Option<Sid>, profile_id: &str, cwd: Option<PathBuf>, label: Option<String>, cols: u16, rows: u16, now: u64) -> std::result::Result<Sid, auth::Error> {
        let p = self.profiles.iter().find(|p| p.0 == profile_id).ok_or(auth::Error::NotFound)?.1.clone();
        self.open_with(sid, profile_id, p, cwd, label, cols, rows, now)
    }
    /// Sessão SSH: o PTY roda o `ssh.exe` direto (argumentos já validados), sem shell.
    #[allow(clippy::too_many_arguments)]
    fn open_ssh(&mut self, sid: Option<Sid>, ssh: ssh::Ssh, cwd: Option<PathBuf>, label: Option<String>, cols: u16, rows: u16, now: u64) -> std::result::Result<Sid, auth::Error> {
        // osc: true lê o OSC 7 (pasta) e 133 que o shell remoto emitir; sem eles a aba só fica sem pasta.
        // Id definido antes do PTY: nomeia o tmux da aba (`live-<8 hex>`), igual ao reconectar.
        let sid = Some(match sid { Some(s) => s, None => session::new_sid().map_err(|_| auth::Error::NotFound)? });
        let tag: String = sid.unwrap()[..4].iter().map(|b| format!("{b:02x}")).collect();
        let p = Profile { label: "SSH".into(), command: ssh.command_line(&format!("live-{tag}")), osc: true };
        let sid = self.open_with(sid, SSH_PROFILE, p, cwd, label, cols, rows, now)?;
        self.ssh.insert(sid, ssh);
        Ok(sid)
    }
    #[allow(clippy::too_many_arguments)]
    fn open_with(&mut self, sid: Option<Sid>, profile_id: &str, p: Profile, cwd: Option<PathBuf>, label: Option<String>, cols: u16, rows: u16, now: u64) -> std::result::Result<Sid, auth::Error> {
        let cell = Arc::new(OnceLock::new());
        let c = cell.clone();
        // O lock do hub é tomado antes de ler `cell`: a saída que chegar antes do `create` espera, não se perde.
        let pty = conpty::ConPty::spawn(&p.command, cwd.as_deref(), cols, rows, move |ev| { let mut h = hub(); if let Some(sid) = c.get() { h.pty(*sid, ev) } }).map_err(|_| auth::Error::IoError)?;
        let sid = self.register(sid, Box::new(pty), profile_id, p.osc, cwd, label, cols, rows, now)?;
        let _ = cell.set(sid);
        Ok(sid)
    }
    /// SSH restaurado do disco: entra na tabela sem processo e já `exited` ("desconectada").
    /// Não reconecta sozinho (evitaria pedir senha de novo sem ninguém olhando); `ssh_reconnect` abre o ssh.exe.
    #[allow(clippy::too_many_arguments)]
    fn open_ssh_dormant(&mut self, sid: Sid, ssh: ssh::Ssh, cwd: Option<PathBuf>, label: Option<String>, cols: u16, rows: u16, now: u64) -> std::result::Result<Sid, auth::Error> {
        let sid = self.register(Some(sid), Box::new(Dormant), SSH_PROFILE, true, cwd, label, cols, rows, now)?;
        self.ssh.insert(sid, ssh);
        Ok(sid)
    }
    #[allow(clippy::too_many_arguments)]
    fn register(&mut self, sid: Option<Sid>, pty: Box<dyn session::Pty + Send>, profile_id: &str, osc: bool, cwd: Option<PathBuf>, label: Option<String>, cols: u16, rows: u16, now: u64) -> std::result::Result<Sid, auth::Error> {
        let sid = match sid { Some(id) => self.s.create_as(id, pty, label, cols, rows, now)?, None => self.s.create(pty, label, cols, rows, now)? };
        self.screens.insert(sid, screen::Screen::new(cols, rows, osc));
        self.base.insert(sid, screen::Screen::new(cols, rows, false));
        if !osc { self.plain.insert(sid); }
        if let Some(d) = cwd { self.cwds.insert(sid, d); }
        self.prof.insert(sid, profile_id.into());
        self.dirty = true;
        Ok(sid)
    }
    /// Inicia (ou retoma) o CLI de IA dentro do shell da sessão; o id da conversa é o id da sessão.
    fn start_ai(&mut self, sid: Sid, agent: String, args: String, resume: bool) {
        let id = proto::uuid_str(&sid);
        let resume = resume && (agent == "codex" || conversation_exists(&agent, &id));
        self.s.type_in(&sid, format!("{}\r", ai_cmd(&agent, &args, &id, resume)).as_bytes());
        self.ai.insert(sid, (agent, args));
        self.dirty = true;
    }
    #[cfg(test)]
    fn handle_close_for_test(&mut self, sid: Sid) { let _ = self.s.close(sid); self.forget(&[sid]); self.dirty = true; }
    /// Grava `sessions.json` (tmp + rename) se algo mudou.
    /// Sob o lock do hub só copia os dados; cifrar (DPAPI) e escrever no disco vão para a thread de gravação,
    /// senão o terminal (saída e digitação) parava a cada gravação.
    fn save(&mut self) {
        if let Some((path, list)) = self.snapshot() { save_later(path, list); }
    }
    /// Cópia do que gravar (sem cifrar), com o número de ordem; `None` se nada mudou.
    fn snapshot(&mut self) -> Option<(PathBuf, Job)> {
        let path = self.store.clone().filter(|_| self.dirty)?;
        self.dirty = false;
        let list: Vec<(Saved, Vec<u8>)> = self.s.saved(SAVED_TAIL).into_iter().filter_map(|(sid, label, cols, rows, tail)| Some((Saved {
            id: proto::uuid_str(&sid), profile: self.prof.get(&sid)?.clone(), label, cwd: self.cwds.get(&sid).cloned(), cols, rows, tail: String::new(),
            agent: self.ai.get(&sid).map(|a| a.0.clone()), args: self.ai.get(&sid).map(|a| a.1.clone()).unwrap_or_default(),
            ssh: self.ssh.get(&sid).cloned(), created: self.s.created(&sid).unwrap_or(0),
        }, tail))).collect();
        Some((path, (SAVE_SEQ.fetch_add(1, Relaxed) + 1, list)))
    }
    /// Recria as sessões do disco: mesmo id, perfil, rótulo e pasta; a saída salva volta como histórico.
    fn restore(&mut self, list: Vec<Saved>) {
        for x in list {
            let Some(sid) = auth::uuid_bytes(&x.id) else { continue };
            let cwd = x.cwd.filter(|d| d.is_dir());
            let label = Some(x.label).filter(|l| !l.is_empty());
            let (cols, rows) = (x.cols.max(1), x.rows.max(1));
            // Sessão SSH volta "desconectada", sem processo; a de destino inválido (arquivo editado à mão) é descartada.
            let dormant = x.ssh.is_some();
            let opened = match x.ssh.filter(|t| t.check().is_ok()) {
                Some(t) => self.open_ssh_dormant(sid, t, cwd, label, cols, rows, now_ms()),
                None if dormant => continue,
                None => self.open(Some(sid), &x.profile, cwd, label, cols, rows, now_ms()),
            };
            if opened.is_err() { continue; }
            if x.created > 0 { self.s.set_created(sid, x.created); }
            // Arquivo antigo (texto puro) não decifra: usa como veio.
            let raw = B64.decode(&x.tail).unwrap_or_default();
            let mut out = dpapi(&raw, false).unwrap_or(raw);
            // Sai da tela alternativa e zera cores de um programa que estava aberto; depois, o separador.
            out.extend_from_slice("\x1b[?1049l\x1b[0m\r\n\x1b[2m— sessão restaurada —\x1b[0m\r\n".as_bytes());
            if dormant { out.extend_from_slice(SSH_DORMANT.as_bytes()); }
            self.pty(sid, conpty::Ev::Output(out));
            if dormant { self.quiet_exit = true; self.pty(sid, conpty::Ev::Exit(0)); self.quiet_exit = false; continue; }
            if let Some(a) = x.agent.filter(|a| proto::AGENTS.contains(&a.as_str())) { self.start_ai(sid, a, x.args, true); }
        }
    }
    fn deliver(&mut self, outs: impl IntoIterator<Item = (ConnId, Out)>) {
        for (c, o) in outs {
            let o = match o {
                Out::Snapshot { session_id, offset } => match self.base.get(&session_id) { Some(s) => Out::Ctl(s.snapshot_body(&session_id, offset)), None => continue },
                o => o,
            };
            if let Some(tx) = self.conns.get(&c) { let _ = tx.send(o); }
        }
    }
    /// A tela do host acompanha o tamanho do PTY.
    // ponytail: a tela-base muda de tamanho na hora, não no offset do resize; replay após resize antigo pode desalinhar. Guardar (offset, tamanho) se E7 mostrar.
    fn sync(&mut self, sid: &Sid) {
        let Some((c, r)) = self.s.size(sid) else { return };
        for m in [&mut self.screens, &mut self.base] { if let Some(s) = m.get_mut(sid) { s.resize(c, r); } }
    }
    fn pty(&mut self, sid: Sid, ev: conpty::Ev) {
        let outs = match ev {
            conpty::Ev::Output(d) => {
                let events = self.screens.get_mut(&sid).map(|s| s.feed(&d)).unwrap_or_default();
                for e in &events {
                    match e {
                        screen::Event::Cwd(c) => { self.cwds.insert(sid, c.into()); }
                        // Código de saída do último comando gravado no histórico (qualquer cliente pode ter digitado).
                        screen::Event::CommandCompleted(c) => extras::provider().command_done(&sid, *c),
                    }
                }
                self.dirty = true;
                let mut outs = self.s.on_output(sid, &d, now_ms());
                if let Some(b) = self.base.get_mut(&sid) { b.feed(self.s.dropped(&sid)); }
                for c in self.s.attached(&sid) { outs.extend(events.iter().map(|e| (c, Out::Ctl(screen::event_body(&sid, e))))); }
                if self.ai.contains_key(&sid) {
                    let w = self.screens.get(&sid).is_some_and(|s| extras::waiting_prompt(&s.text()));
                    self.set_waiting(sid, w);
                }
                outs
            }
            conpty::Ev::Exit(code) => {
                self.set_waiting(sid, false);
                if code != 0 && !self.quiet_exit && self.ssh.get(&sid).is_some_and(|t| t.auto_reconnect) { self.schedule_retry(sid, now_ms()); }
                self.s.on_exit(sid, code)
            }
        };
        self.deliver(outs);
        self.forget(&[sid]);
    }
    /// Solta a tela das sessões que saíram da tabela (close após o `05`, TTL).
    fn forget(&mut self, sids: &[Sid]) {
        for sid in sids {
            if self.s.size(sid).is_none() && self.screens.remove(sid).is_some() {
                self.base.remove(sid); self.plain.remove(sid); self.cwds.remove(sid); self.ai.remove(sid); self.ssh.remove(sid);
                self.waiting.remove(sid); self.retry.remove(sid);
                extras::provider().session_gone(sid);
            }
        }
    }
    /// Frame `01` de qualquer cliente (remoto ou o terminal local): escreve no PTY e, se a entrada é nova
    /// (não é reenvio do mesmo `seq`), passa pelo histórico. Ponto único: nada é gravado duas vezes.
    fn input(&mut self, conn: ConnId, sid: Sid, seq: u64, data: &[u8], now: u64) -> std::result::Result<Option<Value>, auth::Error> {
        let fresh = self.s.fresh(conn, &sid, seq);
        let r = self.s.input(conn, sid, seq, data, now)?;
        if fresh { self.capture(&sid, data); }
        Ok(r)
    }
    /// Histórico (tela Histórico, desktop e celular): entrega a entrada ao provedor com a linha do cursor da tela
    /// do host. Sessão de agente não grava (lá se digita prompt, não comando).
    fn capture(&self, sid: &Sid, data: &[u8]) {
        if self.ai.contains_key(sid) { return; }
        let data = String::from_utf8_lossy(data);
        let enter = data.contains('\r');
        let line = if enter { self.screens.get(sid).and_then(|s| s.cursor_line()) } else { None };
        let (label, cwd) = if enter { (self.label_of(sid), self.cwds.get(sid).map(|d| d.to_string_lossy().into_owned())) } else { (String::new(), None) };
        extras::provider().session_input(&extras::Input { session_id: *sid, data: &data, line, label, cwd, ssh: self.ssh.contains_key(sid) });
    }
    /// Rótulo da sessão; sem rótulo, o do perfil (como a aba mostra).
    fn label_of(&self, sid: &Sid) -> String {
        let id = proto::uuid_str(sid);
        let l = self.s.list().into_iter().find(|x| x["session_id"] == id.as_str()).and_then(|x| x["label"].as_str().map(str::to_owned)).unwrap_or_default();
        if !l.is_empty() { return l; }
        if self.ssh.contains_key(sid) { return "SSH".into(); }
        self.prof.get(sid).and_then(|p| self.profiles.iter().find(|x| &x.0 == p)).map(|x| x.1.label.clone()).unwrap_or_default()
    }
    /// Mudou o "esperando confirmação" do agente: avisa as conexões inscritas e a tela desktop.
    fn set_waiting(&mut self, sid: Sid, on: bool) {
        if on == self.waiting.contains(&sid) { return; }
        if on { self.waiting.insert(sid); } else { self.waiting.remove(&sid); }
        let label = self.s.list().into_iter().find(|x| x["session_id"] == proto::uuid_str(&sid).as_str())
            .and_then(|x| x["label"].as_str().map(str::to_owned)).unwrap_or_default();
        let ev = extras::agent_waiting_event(&sid, &label, on);
        let subs: Vec<ConnId> = self.waiting_subs.iter().copied().filter(|c| self.conns.contains_key(c)).collect();
        self.deliver(subs.into_iter().map(|c| (c, Out::Ctl(ev.clone()))));
        extras::provider().agent_waiting(&sid, &label, on);
    }
    /// ssh.exe saiu com erro e o servidor pede "Reconectar ao cair": tenta de novo depois de esperar.
    fn schedule_retry(&mut self, sid: Sid, now: u64) {
        let (n, started) = self.retry.get(&sid).copied().unwrap_or((0, now));
        let n = if now.saturating_sub(started) >= 60_000 { 0 } else { n };
        if n >= RETRY_MAX { self.retry.remove(&sid); return; }
        self.retry.insert(sid, (n + 1, started));
        let wait = retry_delay_ms(n);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let mut h = hub();
            // Fechada, reconectada à mão ou sem a opção nesse meio-tempo: não faz nada.
            let exited = h.s.list().iter().any(|x| x["session_id"] == proto::uuid_str(&sid).as_str() && x["state"] == "exited");
            if exited && h.retry.contains_key(&sid) && h.ssh.get(&sid).is_some_and(|t| t.auto_reconnect) {
                let (c, r) = h.s.size(&sid).unwrap_or((80, 24));
                let _ = h.reconnect_ssh(sid, c, r, now_ms());
            }
        });
    }
    /// Abre o ssh.exe de novo na mesma sessão (mesmo id, rótulo e pasta), se ela não está rodando.
    fn reconnect_ssh(&mut self, session_id: Sid, cols: u16, rows: u16, now: u64) -> std::result::Result<(), auth::Error> {
        let target = self.ssh.get(&session_id).cloned().ok_or(auth::Error::NotFound)?;
        if target.check().is_err() { return Err(auth::Error::InvalidMessage); }
        let running = self.s.list().iter().any(|x| x["session_id"] == proto::uuid_str(&session_id).as_str() && x["state"] == "running");
        if !running {
            // Mesmo id: a aba que o cliente conhece passa a mostrar a conexão nova.
            let label = self.s.list().into_iter().find(|x| x["session_id"] == proto::uuid_str(&session_id).as_str())
                .and_then(|x| x["label"].as_str().map(str::to_owned)).filter(|l| !l.is_empty());
            let cwd = self.cwds.get(&session_id).cloned();
            let retry = self.retry.remove(&session_id);
            self.s.close(session_id)?;
            self.forget(&[session_id]);
            self.open_ssh(Some(session_id), target, cwd, label, cols, rows, now)?;
            if let Some((n, _)) = retry { self.retry.insert(session_id, (n, now)); }
            self.save();
        }
        Ok(())
    }
    fn request(&mut self, conn: ConnId, pk: Pk, req: Req, now: u64) -> std::result::Result<Value, auth::Error> {
        Ok(match req {
            Req::Create { profile_id, cols, rows, label, cwd, agent, args, resume_id, ssh } => {
                let cwd = cwd.map(PathBuf::from);
                if cwd.as_ref().is_some_and(|d| !d.is_absolute() || !d.is_dir()) { return Err(auth::Error::NotFound); }
                // `resume_id`: mesmo id da sessão encerrada; o CLI retoma a conversa (`--resume`). Id ainda vivo é INVALID_MESSAGE.
                let sid = match ssh {
                    Some(t) => self.open_ssh(resume_id, t, cwd, label, cols, rows, now)?,
                    None => self.open(resume_id, &profile_id, cwd, label, cols, rows, now)?,
                };
                if let Some(a) = agent { self.start_ai(sid, a, args.unwrap_or_default(), resume_id.is_some()); self.note_agent(&sid); self.save(); }
                json!({"type":"session_create_ok","session_id":proto::uuid_str(&sid)})
            }
            Req::List { info } => json!({"type":"session_list_ok","sessions":self.s.list().into_iter().map(|mut x| {
                    // Pasta e git vão na lista: o card mostra e copia (campos opcionais, celular ignora).
                    if !info { return x; }
                    let sid = auth::uuid_bytes(x["session_id"].as_str().unwrap_or_default());
                    // SSH: destino vai junto (a UI acha o servidor mesmo após reiniciar); pasta é remota, git roda lá pela UI.
                    let ssh = sid.and_then(|s| self.ssh.get(&s));
                    if let Some(t) = ssh { x["ssh"] = json!({"user": t.user, "host": t.host, "port": t.port}); }
                    if let Some(d) = sid.and_then(|s| self.cwds.get(&s)) {
                        x["cwd"] = d.to_string_lossy().into_owned().into();
                        if ssh.is_none() && let Some(g) = git_cached(d) { x["git"] = g; }
                    }
                    x
                }).collect::<Vec<_>>(),
                "profiles":self.profiles.iter().map(|(id, p)| json!({"profile_id":id,"label":p.label})).collect::<Vec<_>>()}),
            Req::Attach { session_id, from_offset, size } => {
                let (ok, mut outs) = self.s.attach(conn, pk, session_id, from_offset, size, now)?;
                self.sync(&session_id);
                // Pasta conhecida (criação/restauração/OSC 7) vai no attach; senão `cmd` anuncia null.
                if let Some(d) = self.cwds.get(&session_id) { outs.push(Out::Ctl(screen::event_body(&session_id, &screen::Event::Cwd(d.to_string_lossy().into_owned())))); }
                else if self.plain.contains(&session_id) { outs.push(Out::Ctl(screen::cwd_unknown(&session_id))); }
                self.deliver(outs.into_iter().map(|o| (conn, o)));
                ok
            }
            Req::Detach(sid) => self.s.detach(conn, sid, now)?,
            Req::Close(sid) => { let ok = self.s.close(sid)?; self.forget(&[sid]); self.dirty = true; ok }
            Req::Focus(_) => unreachable!("tratado na conexão"),
            Req::Rename { session_id, label } => { self.dirty = true; self.s.rename(session_id, label)? }
            Req::Extra(r) => {
                if matches!(r, extras::Req::AgentList { ext: true }) { self.waiting_subs.insert(conn); }
                self.extra(r, now)?
            }
            Req::Dirs(path) => json!({"type":"dir_list_ok","dirs":list_dirs(&path)}),
            Req::Stats => { let mut v = stats::sys_stats(); v["type"] = "sys_stats_ok".into(); v }
        })
    }
    /// Telas do celular (`extras.rs`). Dados das fatias desktop vêm de `extras::provider()`.
    fn extra(&mut self, r: extras::Req, now: u64) -> std::result::Result<Value, auth::Error> {
        use extras::Req as X;
        let p = extras::provider();
        // Agente retomado: o perfil gravado, se ainda existe; senão o primeiro (como a tela desktop).
        let pick = |want: Option<&str>| self.profiles.iter().find(|p| Some(p.0.as_str()) == want).or(self.profiles.first()).map(|p| p.0.clone()).ok_or(auth::Error::NotFound);
        Ok(match r {
            X::Unsupported => return Err(auth::Error::VersionUnsupported),
            X::AgentList { ext } => extras::agent_list_ok(&extras::merge_agents(self.live_agents(), p.saved_agents()), ext),
            X::AgentResume { session_id, cols, rows } => {
                if !self.ai.contains_key(&session_id) {
                    let a = p.saved_agents().into_iter().find(|a| a.session_id == session_id).ok_or(auth::Error::NotFound)?;
                    // Mesmas regras do `session_create`: o registro vem do disco, não do proto.
                    if !proto::AGENTS.contains(&a.agent.as_str()) || !proto::valid_args(&a.args) { return Err(auth::Error::InvalidMessage); }
                    let cwd = a.cwd.map(PathBuf::from).filter(|d| d.is_dir());
                    let label = Some(a.label).filter(|l| !l.is_empty()).map(|l| l.chars().take(64).collect());
                    let profile = pick(a.profile_id.as_deref())?;
                    // Mesmo caminho do `session_create` com `resume_id`: mesmo id, CLI com `--resume` se a conversa existe.
                    let sid = self.open(Some(session_id), &profile, cwd, label, cols, rows, now)?;
                    self.start_ai(sid, a.agent, a.args, true);
                    self.save();
                }
                // Viva ou recriada: o registro marca o uso (`used_ms`), como a tela desktop.
                self.note_agent(&session_id);
                extras::session_ok("agent_resume_ok", &session_id)
            }
            X::AgentCreate { agent, args, cwd, label, cols, rows } => {
                // Mesmas regras do `session_create` com agente (já validados no parse): pasta absoluta que existe.
                let dir = cwd.as_ref().map(PathBuf::from);
                if dir.as_ref().is_some_and(|d| !d.is_absolute() || !d.is_dir()) { return Err(auth::Error::NotFound); }
                let label = label.unwrap_or_else(|| extras::agent_label(cwd.as_deref(), &agent));
                let profile = pick(None)?;
                let sid = self.open(None, &profile, dir, Some(label), cols, rows, now)?;
                self.start_ai(sid, agent, args, false);
                self.note_agent(&sid);
                self.save();
                extras::session_ok("agent_create_ok", &sid)
            }
            X::AgentRemove { session_id } => {
                let live = self.s.size(&session_id).is_some();
                if !live && !p.saved_agents().iter().any(|a| a.session_id == session_id) { return Err(auth::Error::NotFound); }
                if live && self.ai.contains_key(&session_id) {
                    self.s.close(session_id)?;
                    self.forget(&[session_id]);
                    self.dirty = true;
                    self.save();
                }
                p.agent_removed(&session_id);
                extras::empty_ok("agent_remove_ok")
            }
            X::SshList { ext } => extras::ssh_list_ok(&p.ssh_servers(), &self.ssh_sessions(), ext),
            X::SshSave { server_id, label, target } => {
                // Destino conferido de novo aqui (o parse já fez): nada chega ao disco sem `Ssh::check`.
                if target.check().is_err() { return Err(auth::Error::InvalidMessage); }
                let id = p.ssh_save(server_id.as_deref(), &label, &target).map_err(|c| extras::code(&c))?;
                extras::ssh_save_ok(&id)
            }
            X::SshRemove { server_id } => { p.ssh_remove(&server_id).map_err(|c| extras::code(&c))?; extras::empty_ok("ssh_remove_ok") }
            X::SshProbe { server_id } => { p.ssh_probe(&server_id).map_err(|c| extras::code(&c))?; extras::empty_ok("ssh_probe_ok") }
            X::SshReconnect { session_id, cols, rows } => {
                // Reconectar à mão zera a conta da reconexão automática.
                self.retry.remove(&session_id);
                self.reconnect_ssh(session_id, cols, rows, now)?;
                extras::session_ok("ssh_reconnect_ok", &session_id)
            }
            X::SshConnect { server_id, cols, rows } => {
                // Como a tela desktop: o PTY roda o ssh.exe com o destino validado; nada é digitado num shell.
                let target = p.ssh_target(&server_id).ok_or(auth::Error::NotFound)?;
                if target.check().is_err() { return Err(auth::Error::InvalidMessage); }
                let label = p.ssh_servers().into_iter().find(|s| s.server_id == server_id).map(|s| s.label).filter(|l| !l.is_empty()).map(|l| l.chars().take(64).collect());
                let sid = self.open_ssh(None, target, None, label, cols, rows, now)?;
                self.save();
                extras::session_ok("ssh_connect_ok", &sid)
            }
            X::HistoryQuery(f) => {
                let sessions = f.ext.then(|| p.history_sessions(&f, local_offset_min()));
                extras::history_query_ok(&p.history(&f), sessions.as_deref())
            }
            X::SettingsGet => extras::settings_ok("settings_get_ok", &p.settings_get().ok_or(auth::Error::NotFound)?),
            X::SettingsSet(m) => {
                let a = p.settings_get().ok_or(auth::Error::NotFound)?.patch(&m)?;
                p.settings_set(&a).map_err(|c| extras::code(&c))?;
                extras::settings_ok("settings_set_ok", &a)
            }
            X::HistoryClear => { p.history_clear().map_err(|c| extras::code(&c))?; extras::empty_ok("history_clear_ok") }
            // Mesma listagem do `dir_list` (navegador de pastas do agente no desktop).
            X::FolderList(path) => extras::folder_list_ok(&path, &list_dirs(&path)),
            X::SshKeyList(path) => {
                let root = extras::ssh_root().ok_or(auth::Error::NotFound)?;
                let (shown, dirs, keys) = extras::ssh_keys(&root, &path)?;
                extras::ssh_key_list_ok(&root.to_string_lossy(), &shown, &dirs, &keys)
            }
            X::AgentDefaultGet => extras::agent_default_ok(p.agent_default().as_ref()),
            X::AgentDefaultSet(d) => { p.agent_default_save(&d).map_err(|c| extras::code(&c))?; extras::empty_ok("agent_default_set_ok") }
            X::HistoryRun { entry_id, session_id } => {
                if self.s.size(&session_id).is_none() { return Err(auth::Error::NotFound); }
                let cmd = p.history_command(&entry_id).ok_or(auth::Error::NotFound)?;
                let data = format!("{cmd}\r");
                self.s.type_in(&session_id, data.as_bytes());
                // Como "Reexecutar" no desktop (que passa pelo frame `01`): entra de novo no histórico.
                self.capture(&session_id, data.as_bytes());
                extras::history_run_ok()
            }
        })
    }
    /// Avisa o provedor (registro `agents.json` no app) que a sessão de agente foi criada ou retomada, venha de onde vier.
    fn note_agent(&self, sid: &Sid) {
        if let Some(a) = self.live_agents().into_iter().find(|a| a.session_id == *sid) { extras::provider().agent_started(&a); }
    }
    /// Sessões SSH da tabela; `exited` (ssh.exe saiu ou restaurada do disco) é "desconectada".
    fn ssh_sessions(&self) -> Vec<extras::SshSession> {
        self.s.list().into_iter().filter_map(|x| {
            let sid = auth::uuid_bytes(x["session_id"].as_str()?)?;
            self.ssh.contains_key(&sid).then(|| extras::SshSession { session_id: sid, label: x["label"].as_str().unwrap_or_default().to_owned(), connected: x["state"] != "exited" })
        }).collect()
    }
    /// Sessões vivas com agente de IA, com rótulo, pasta e estado da tabela.
    fn live_agents(&self) -> Vec<extras::Agent> {
        let list = self.s.list();
        self.ai.iter().filter_map(|(sid, (agent, args))| {
            let id = proto::uuid_str(sid);
            let x = list.iter().find(|x| x["session_id"] == id.as_str())?;
            Some(extras::Agent {
                session_id: *sid, agent: agent.clone(), args: args.clone(), label: x["label"].as_str().unwrap_or_default().to_owned(),
                cwd: self.cwds.get(sid).map(|d| d.to_string_lossy().into_owned()),
                state: if x["state"] == "exited" { extras::AgentState::Exited } else { extras::AgentState::Running },
                waiting: self.waiting.contains(sid),
                last_activity_ms: x["last_activity_ms"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                profile_id: self.prof.get(sid).cloned(),
            })
        }).collect()
    }
}
/// Últimos bytes de saída de cada sessão guardados no disco.
const SAVED_TAIL: usize = 128 << 10;
/// Uma sessão em `sessions.json`.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
struct Saved { id: String, profile: String, label: String, cwd: Option<PathBuf>, cols: u16, rows: u16, tail: String,
    #[serde(default)] agent: Option<String>, #[serde(default)] args: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] ssh: Option<ssh::Ssh>,
    #[serde(default)] created: u64 }

/// Git da pasta para `session_list`, sem segurar o hub: devolve o último valor conhecido e, se tiver
/// mais de 10 s (ou nunca rodou), atualiza numa thread à parte. Git lento (4–6 s por chamada em repositório
/// grande) × várias sessões passava do prazo de 10 s do pedido e travava app e celular.
fn git_cached(dir: &std::path::Path) -> Option<Value> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    // pasta → (valor, quando atualizou, atualização em curso)
    type Cache = Mutex<HashMap<PathBuf, (Option<Value>, std::time::Instant, bool)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut m = cache.lock().unwrap();
    let e = m.entry(dir.to_path_buf()).or_insert((None, std::time::Instant::now() - std::time::Duration::from_secs(3600), false));
    if !e.2 && e.1.elapsed() > std::time::Duration::from_secs(10) {
        e.2 = true;
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            let v = git_info(&d);
            if let Some(e) = cache.lock().unwrap().get_mut(&d) { *e = (v, std::time::Instant::now(), false); }
        });
    }
    e.0.clone()
}

/// Branch e nº de arquivos pendentes da pasta; `None` fora de repositório git.
fn git_info(dir: &std::path::Path) -> Option<Value> {
    let run = |args: &[&str]| {
        let mut c = std::process::Command::new("git");
        c.arg("-C").arg(dir).args(args);
        #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
        // Git travado (drive de rede) não segura o hub: 2 s e mata o processo.
        let mut ch = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
        let mut out = ch.stdout.take()?;
        let rd = std::thread::spawn(move || { let mut v = Vec::new(); std::io::Read::read_to_end(&mut out, &mut v).map(|_| v) });
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let st = loop {
            if let Some(st) = ch.try_wait().ok()? { break st; }
            if std::time::Instant::now() > end { let _ = ch.kill(); let _ = ch.wait(); return None; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let v = rd.join().ok()?.ok()?;
        st.success().then(|| String::from_utf8_lossy(&v).into_owned())
    };
    let branch = run(&["branch", "--show-current"])?.trim().to_owned();
    let dirty = run(&["status", "--porcelain"]).map_or(0, |s| s.lines().count());
    Some(json!({"branch": branch, "dirty": dirty}))
}
/// Perfil gravado das sessões SSH (o comando sai de `Saved::ssh`, não de `profiles.json`).
const SSH_PROFILE: &str = "ssh";
/// Aviso no fim da saída de uma sessão SSH restaurada sem processo.
const SSH_DORMANT: &str = "\x1b[33m— SSH desconectado: use Reconectar na tela SSH —\x1b[0m\r\n";
/// PTY sem processo (SSH restaurado como desconectado): já nasce `exited`, então nada é escrito nele.
struct Dormant;
impl session::Pty for Dormant {
    fn write(&mut self, _: &[u8]) -> std::io::Result<()> { Ok(()) }
    fn resize(&mut self, _: u16, _: u16) -> std::io::Result<()> { Ok(()) }
    fn kill(&mut self) -> std::io::Result<()> { Ok(()) }
}
/// Até 500 subpastas, ordem alfabética; vazio lista as unidades. Erro de leitura: lista vazia.
fn list_dirs(path: &str) -> Vec<String> {
    if path.is_empty() { return ('A'..='Z').map(|c| format!("{c}:\\")).filter(|d| Path::new(d).is_dir()).collect(); }
    let Ok(rd) = std::fs::read_dir(path) else { return vec![] };
    let mut v: Vec<String> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).filter_map(|e| e.file_name().into_string().ok()).take(500).collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

/// O CLI só grava a conversa após a primeira mensagem; sem ela `--resume` falha ("Nenhuma conversa encontrada").
/// Arquivo: `~/.<agente>/projects/<pasta>/<id>.jsonl` (procura em todas: a pasta da aba pode ter mudado).
fn conversation_exists(agent: &str, id: &str) -> bool {
    let Some(home) = std::env::var_os("USERPROFILE") else { return false };
    let Ok(dirs) = std::fs::read_dir(PathBuf::from(home).join(format!(".{agent}")).join("projects")) else { return false };
    dirs.flatten().any(|d| d.path().join(format!("{id}.jsonl")).is_file())
}
/// Linha digitada no shell para iniciar/retomar o agente. `args` já validado no proto (sem metacaracteres de shell).
// ponytail: codex não aceita id na criação; retoma a última conversa da pasta (`resume --last`).
fn ai_cmd(agent: &str, args: &str, id: &str, resume: bool) -> String {
    let base = match (agent, resume) {
        ("codex", false) => "codex".to_owned(),
        ("codex", true) => "codex resume --last".to_owned(),
        (a, false) => format!("{a} --session-id {id}"),
        (a, true) => format!("{a} --resume {id}"),
    };
    if args.trim().is_empty() { base } else { format!("{base} {}", args.trim()) }
}
#[test]
fn ai_cmd_resume() {
    assert_eq!(ai_cmd("claude", " --model x ", "u1", false), "claude --session-id u1 --model x");
    assert_eq!(ai_cmd("openclaude", "", "u1", true), "openclaude --resume u1");
    assert_eq!(ai_cmd("codex", "", "u1", true), "codex resume --last");
    let s: Saved = serde_json::from_str(r#"{"id":"a","profile":"p","label":"","cwd":null,"cols":1,"rows":1,"tail":""}"#).unwrap();
    assert_eq!((s.agent, s.args), (None, String::new()));
}
/// Pedido de gravação: número de ordem e sessões com a saída ainda sem cifrar.
type Job = (u64, Vec<(Saved, Vec<u8>)>);
static SAVE_SEQ: AtomicU64 = AtomicU64::new(0);
/// Último número gravado; o lock também impede duas gravações ao mesmo tempo.
static SAVE_DONE: Mutex<u64> = Mutex::new(0);
/// Cifra as saídas e grava, fora do lock do hub; pedido mais velho que o já gravado é descartado.
/// Falha é silenciosa (sem console no app) e o próximo save tenta de novo.
fn write_pending(path: &Path, (seq, list): Job) {
    let mut done = SAVE_DONE.lock().unwrap();
    if seq <= *done { return; }
    let list: Vec<Saved> = list.into_iter().map(|(mut s, tail)| { s.tail = B64.encode(dpapi(&tail, true).unwrap_or_default()); s }).collect();
    if write_saved(path, &list).is_ok() { *done = seq; }
}
/// Testes: grava na hora (leem o arquivo logo depois).
#[cfg(test)]
fn save_later(path: PathBuf, job: Job) { write_pending(&path, job); }
/// Uma thread de gravação; se chegarem vários pedidos enquanto grava, só o mais novo é escrito.
#[cfg(not(test))]
fn save_later(path: PathBuf, job: Job) {
    use std::sync::mpsc;
    static TX: OnceLock<Mutex<mpsc::Sender<(PathBuf, Job)>>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(PathBuf, Job)>();
        std::thread::Builder::new().name("live-cmd-save".into()).spawn(move || {
            while let Ok(mut job) = rx.recv() {
                while let Ok(newer) = rx.try_recv() { job = newer; }
                write_pending(&job.0, job.1);
            }
        }).expect("thread de gravação");
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send((path, job));
}
fn write_saved(path: &Path, list: &[Saved]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(list)?)?;
    std::fs::rename(&tmp, path)
}
/// Arquivo ausente ou ilegível = nada a restaurar.
fn read_saved(path: &Path) -> Vec<Saved> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
/// Remove a conexão do hub em qualquer saída do loop.
struct Reg(ConnId);
impl Drop for Reg { fn drop(&mut self) { let mut h = hub(); h.conns.remove(&self.0); h.peers.remove(&self.0); h.waiting_subs.remove(&self.0); h.s.disconnect(self.0, now_ms()); } }
struct Live { conn: ConnId, pk: Pk, paused: bool, bg: bool, tokens: f64, refill: Instant, last_rx: Instant, ping: u64, awaiting: bool, _reg: Reg }
/// `a=max-message-size` do SDP remoto; ausente = 65536 (RFC 8841 §6.1).
fn remote_max_message(sdp: &str) -> u64 {
    sdp.lines().find_map(|l| l.trim().strip_prefix("a=max-message-size:")).and_then(|v| v.trim().parse().ok()).unwrap_or(65536)
}
async fn send_out(ch: &Arc<dyn DataChannel>, id: &str, conn: ConnId, o: Out) -> Result<()> {
    match o {
        Out::Ctl(v) => ch.send_text(&proto::encode_control(id, None, v)).await?,
        Out::Frame(f) => {
            ch.send(proto::encode_frame(&f)[..].into()).await?;
            if let Op::Output(d) = &f.op { let mut h = hub(); let outs = h.s.sent(conn, f.session_id, d.len()); h.deliver(outs.into_iter().map(|o| (conn, o))); }
        }
        Out::Snapshot { .. } => {}
    }
    Ok(())
}
/// Controles dos dois canais somam no mesmo balde (C3 §6, C4 §7): 50/s, rajada 100.
fn take_token(l: &mut Live) -> bool {
    l.tokens = (l.tokens + l.refill.elapsed().as_secs_f64() * 50.0).min(100.0);
    l.refill = Instant::now();
    if l.tokens < 1.0 { return false; }
    l.tokens -= 1.0;
    true
}

// ---- D4c: file-channel após AUTHENTICATED (C4) ----
/// `bufferedAmount` do `file-channel` (§7): pausa acima de 512 KiB, retoma no `bufferedamountlow` de 128 KiB.
const FPAUSE: usize = 512 << 10;
const FRESUME: u32 = 128 << 10;
enum FIn { Text(Vec<u8>), Frame(Vec<u8>), Pull, Tick }
enum FOut { Ctl(Option<String>, Value), Frames(Vec<Vec<u8>>), Idle, Fatal(auth::Error), Close }
struct FileHub { f: files::Files, tx: HashMap<ConnId, mpsc::UnboundedSender<FOut>> }
/// Transferências C4; lock próprio para o disco nunca segurar o do hub (saída do PTY).
static FILES: Mutex<Option<FileHub>> = Mutex::new(None);
fn files() -> MutexGuard<'static, Option<FileHub>> { FILES.lock().unwrap_or_else(|e| e.into_inner()) }
/// Commit por tempo e expurgo (§5); os `file_ack` vão para a conexão dona.
fn files_tick() {
    let mut g = files();
    let Some(FileHub { f, tx }) = g.as_mut() else { return };
    for (c, v) in f.tick(now_ms()) { if let Some(t) = tx.get(&c) { let _ = t.send(FOut::Ctl(None, v)); } }
}
/// Abre o staging uma vez por processo (§5).
fn files_open(data_dir: &Path) {
    let mut g = files();
    if g.is_none() { *g = Some(FileHub { f: files::Files::open(data_dir, Default::default(), Default::default(), now_ms()), tx: HashMap::new() }); }
}
/// Revogação (§8): apaga staging e manifestos do aparelho.
fn files_revoke(pk: &Pk) { if let Some(h) = files().as_mut() { h.f.revoke(pk, now_ms()); } }
/// Pool de disco da conexão (§7): hash, gravação, `fsync` e leitura fora do laço do PTY.
/// Fila de 8 chunks (× 4 fragmentos); cheia, o laço para de ler o `file-channel`. Fim da fila = queda (§8, pausa).
fn file_worker(conn: ConnId, pk: Pk, id: String, host: Arc<Host>) -> (mpsc::Sender<FIn>, mpsc::UnboundedReceiver<FOut>) {
    let (tx, mut rx) = mpsc::channel(8 * 4);
    let (otx, orx) = mpsc::unbounded_channel();
    if let Some(h) = files().as_mut() { h.tx.insert(conn, otx.clone()); }
    tokio::task::spawn_blocking(move || {
        while let Some(m) = rx.blocking_recv() {
            match file_step(conn, &pk, &id, &host, m, &otx) {
                Ok(true) => {}
                Ok(false) => { let _ = otx.send(FOut::Close); break }
                Err(e) => { let _ = otx.send(FOut::Fatal(e)); break }
            }
        }
        if let Some(h) = files().as_mut() { h.f.disconnect(conn, now_ms()); h.tx.remove(&conn); }
    });
    (tx, orx)
}
/// Destino de upload por tipo: print vai a `<projeto>\screenshots` ou `~\Pictures\Screenshots`; arquivo a `<projeto>\archives` ou `~\Downloads\archives`.
fn upload_dir(kind: &str, project: Option<&Path>, home: &Path) -> PathBuf {
    match (kind, project) {
        ("print", Some(p)) => p.join("screenshots"),
        ("print", None) => home.join("Pictures").join("Screenshots"),
        (_, Some(p)) => p.join("archives"),
        (_, None) => home.join("Downloads").join("archives"),
    }
}
/// Um item da fila. `Err` = fatal; `Ok(false)` = `error` fatal do peer (fecha sem responder).
fn file_step(conn: ConnId, pk: &Pk, id: &str, host: &Host, m: FIn, out: &mpsc::UnboundedSender<FOut>) -> std::result::Result<bool, auth::Error> {
    use files::{Control as C, Pull, Req as F};
    let now = now_ms();
    let send = |rid: Option<String>, v: Value| { let _ = out.send(FOut::Ctl(rid, v)); };
    // O cwd sai do hub antes do lock de arquivos: os dois locks nunca ficam juntos.
    let ctl = match &m {
        FIn::Tick => { files_tick(); return Ok(true) }
        FIn::Text(t) => {
            let c = files::parse_control(std::str::from_utf8(t).map_err(|_| auth::Error::InvalidMessage)?, id)?;
            // C3 §7 vale aqui: todo pedido confere a trust store.
            if matches!(c, C::Request { .. }) { host.check_trusted(pk)?; }
            let sid = match &c { C::Request { req: F::FsList { session_id, .. } | F::UploadInit { session_id, .. } | F::BatchUploadInit { session_id, .. }, .. } => *session_id, _ => None };
            let mut cwd = sid.and_then(|s| hub().cwds.get(&s).cloned());
            // Upload sem `target_dir` nem cwd conhecido (cmd não anuncia OSC 7): pasta do usuário em vez de NOT_FOUND.
            if let C::Request { req: F::UploadInit { batch: None, target_dir: None, kind: Some(k), .. }, .. } = &c {
                cwd = std::env::var_os("USERPROFILE").map(|h| upload_dir(k, cwd.as_deref(), Path::new(&h)));
                if let Some(d) = &cwd { let _ = std::fs::create_dir_all(d); }
            } else if cwd.is_none() && matches!(c, C::Request { req: F::UploadInit { batch: None, .. }, .. }) { cwd = std::env::var_os("USERPROFILE").map(PathBuf::from); }
            Some((c, cwd))
        }
        _ => None,
    };
    let mut g = files();
    let f = &mut g.as_mut().ok_or(auth::Error::IoError)?.f;
    match (m, ctl) {
        (_, Some((C::Error { request_id: None, file_id: None, .. }, _))) => return Ok(false),
        // ponytail: `error` do cliente com `file_id` não pausa a transferência no host; o cliente só para de pedir/enviar.
        (_, Some((C::Error { .. }, _))) => {}
        (_, Some((C::Ack { file_id, ranges }, _))) => f.client_ack(conn, &file_id, &ranges, now),
        (_, Some((C::Complete(fid), _))) => f.client_complete(conn, &fid, now),
        (_, Some((C::BatchComplete(bid), _))) => f.client_batch_complete(conn, &bid, now),
        (_, Some((C::Request { request_id, req }, cwd))) => {
            let (body, extra) = f.request(conn, pk, req, cwd.as_deref(), now)?;
            if !body.is_null() { send(Some(request_id), body); }
            for v in extra { send(None, v); }
        }
        (FIn::Frame(b), _) => for v in f.on_frame(conn, &b, now)? { send(None, v); },
        (FIn::Pull, _) => { let _ = out.send(match f.pull(conn, now) {
            Some(Pull::Frames(_, fr)) => FOut::Frames(fr),
            // Erro de uma transferência: a vez volta ao laço, que pede de novo.
            Some(Pull::Ctl(v)) => { send(None, v); FOut::Frames(vec![]) }
            None => FOut::Idle,
        }); }
        _ => {}
    }
    Ok(true)
}

/// Mensagem do `pty-channel` após AUTHENTICATED. `Some(e)`: erro fatal (fecha com `error` sem `request_id`).
async fn c3_message(ch: &Arc<dyn DataChannel>, id: &str, host: &Host, l: &mut Live, data: &[u8], is_string: bool) -> Result<Option<auth::Error>> {
    use auth::Error::*;
    l.last_rx = Instant::now();
    let now = now_ms();
    if !is_string {
        let f = match proto::parse_frame(data) { Ok(f) => f, Err(e) => return Ok(Some(e)) };
        let reply = {
            let mut h = hub();
            let r = match &f.op {
                // Eco do ping do cliente; o eco do nosso ping só renova `last_rx`.
                Op::Heartbeat if l.awaiting && f.seq == l.ping => { l.awaiting = false; Ok(None) }
                Op::Heartbeat => Ok(Some(Out::Frame(f.clone()))),
                Op::Input(d) => h.input(l.conn, f.session_id, f.seq, d, now).map(|a| a.map(Out::Ctl)),
                Op::Resize { cols, rows } => h.s.resize(l.conn, f.session_id, *cols, *rows).map(|_| None),
                _ => Err(InvalidMessage),
            };
            h.sync(&f.session_id);
            match r {
                // `input_ack` pela fila do hub: fica atrás do replay e da saída já enfileirados.
                Ok(Some(o @ Out::Ctl(_))) => { h.deliver([(l.conn, o)]); None }
                Ok(r) => r,
                Err(e) => return Ok(Some(e)),
            }
        };
        if let Some(o) = reply { send_out(ch, id, l.conn, o).await?; }
        return Ok(None);
    }
    if !take_token(l) { return Ok(Some(LimitExceeded)); }
    let Ok(t) = std::str::from_utf8(data) else { return Ok(Some(InvalidMessage)) };
    let (rid, req) = match proto::parse_control(t, id) {
        Ok(Control::Request { request_id, req }) => (request_id, req),
        Ok(Control::Error { request_id: Some(_), .. }) => return Ok(None),
        Ok(Control::Error { request_id: None, .. }) => return Err("peer fatal error".into()),
        Err(e) => return Ok(Some(e)),
    };
    // §7: todo pedido confere a trust store (a releitura do arquivo é o tick de 1 s).
    if let Err(e) = host.check_trusted(&l.pk) { return Ok(Some(e)); }
    let body = match req {
        Req::Focus(f) => { l.bg = f == Focus::Background; Ok(json!({"type":"client_focus_ok"})) }
        // Pergunta de capacidades (`extras::CAPS_PROBE`): host antigo responde NOT_FOUND pelo hub.
        Req::Attach { session_id, .. } if extras::is_caps_probe(&session_id) => Ok(extras::caps_ok()),
        req => hub().request(l.conn, l.pk, req, now),
    };
    let body = match body { Ok(v) => v, Err(InvalidMessage) => return Ok(Some(InvalidMessage)), Err(e) => proto::error(e) };
    ch.send_text(&proto::encode_control(id, Some(&rid), body)).await?;
    Ok(None)
}
/// Perfis do host (`profiles.json` ao lado da trust store); sem arquivo, pwsh e WSL com integração OSC 133/7.
fn profiles(path: &std::path::Path) -> Result<Vec<(String, Profile)>> {
    let Ok(t) = std::fs::read_to_string(path) else {
        let utf16: Vec<u8> = screen::PWSH_PROMPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let on_path = |exe: &str| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(exe).is_file()));
        let ps = if on_path("pwsh.exe") { "pwsh.exe" } else { "powershell.exe" };
        let bash = base64::engine::general_purpose::STANDARD.encode(screen::BASH_PROMPT_COMMAND);
        return Ok(vec![
            ("pwsh".into(), Profile { label: "PowerShell".into(), command: format!("{ps} -NoLogo -NoExit -EncodedCommand {}", base64::engine::general_purpose::STANDARD.encode(utf16)), osc: true }),
            ("wsl".into(), Profile { label: "WSL".into(), command: format!("wsl.exe -e bash -c \"PROMPT_COMMAND=$(echo {bash} | base64 -d) exec bash -i\""), osc: true }),
        ]);
    };
    let v: Value = serde_json::from_str(&t)?;
    v.as_array().ok_or("profiles")?.iter().map(|p| Ok((p["profile_id"].as_str().ok_or("profile_id")?.into(), with_cmd_prompt(Profile {
        label: p["label"].as_str().ok_or("label")?.into(), command: p["command"].as_str().ok_or("command")?.into(), osc: p["osc"].as_bool().unwrap_or(false),
    })))).collect()
}
/// Perfil `cmd.exe` puro (sem argumentos): ganha o prompt com OSC 133/7, como pwsh e WSL.
fn with_cmd_prompt(p: Profile) -> Profile {
    if p.osc || !matches!(p.command.trim().to_ascii_lowercase().as_str(), "cmd" | "cmd.exe") { return p; }
    Profile { command: format!("{} /k prompt {}", p.command.trim(), screen::CMD_PROMPT), osc: true, ..p }
}
/// Log de conexão do host em `<dados>/host.log` (sem chaves, SDP nem dados do terminal); acima de 1 MB recomeça.
// ponytail: arquivo simples com corte por tamanho; trocar por crate de log se precisar de níveis/rotação.
pub fn hlog(msg: &str) {
    use std::io::Write;
    let Ok(dir) = default_data_dir() else { return };
    let p = dir.join("host.log");
    if std::fs::metadata(&p).is_ok_and(|m| m.len() > 1 << 20) { let _ = std::fs::rename(&p, dir.join("host.log.1")); }
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) { let _ = writeln!(f, "{ts} {msg}"); }
}
/// Fase da tentativa atual, lida por [`Service::status`] e pelo backoff do laço de presença.
#[derive(Default)]
struct Link { state: std::sync::atomic::AtomicU8, ready: AtomicBool }
// 0 (padrão) = conectando.
const RUNNING: u8 = 1; const ERROR: u8 = 2;
/// `true` quando [`Service::stop`] foi pedido.
async fn stopped(stop: &mut tokio::sync::watch::Receiver<bool>) { if stop.wait_for(|s| *s).await.is_err() { std::future::pending::<()>().await } }
/// Socket de presença na sala (SPEC-C1-presenca §3): `hello` com `connection_id: null`, espera sem prazo;
/// cada `ready` traz o `connection_id` de um cliente e abre uma [`attempt`] própria, em paralelo com as outras.
/// Envelopes são roteados pelo `connection_id`; o fim de uma tentativa não derruba as demais nem o socket.
/// `once`: devolve o resultado da primeira tentativa (testes de uma conexão). Sem `once`, sai só quando o WS cai.
async fn run(url: &str, room: &str, host: Arc<Host>, link: &Link, mut stop: tokio::sync::watch::Receiver<bool>, attempts: &mut Vec<tokio::task::JoinHandle<Result<()>>>, once: bool) -> Result<()> {
    let rid = auth::uuid_bytes(room).ok_or("room UUID")?;
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(65536)).max_frame_size(Some(65536));
    let connected = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async_with_config(format!("{url}/v1/ws/{room}"), Some(config), false)).await;
    let Ok(Ok((mut ws, _))) = connected else { hlog(&format!("sinalização inacessível: {url}")); return Err("signaling unreachable".into()) };
    hlog("presença: WS conectado");
    ws.send(json!({"v":1,"type":"hello","connection_id":null,"request_id":"host","payload":{"role":"host"}}).to_string().into()).await?;
    // Saída de todas as tentativas passa por um único escritor (este laço).
    let (out, mut out_rx) = mpsc::unbounded_channel::<String>();
    let mut routes: HashMap<String, mpsc::UnboundedSender<String>> = HashMap::new();
    let mut first: Option<tokio::task::JoinHandle<Result<()>>> = None;
    loop {
        let frame = tokio::select! {
            f = ws.next() => f.ok_or("websocket closed")??,
            Some(wire) = out_rx.recv() => { tokio::time::timeout(Duration::from_secs(2), ws.send(wire.into())).await??; continue }
            r = async { first.as_mut().unwrap().await }, if first.is_some() => return r?,
            _ = stopped(&mut stop) => return Ok(()),
        };
        let raw = match frame { tokio_tungstenite::tungstenite::Message::Text(t) => t, tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(()), _ => return Err("websocket type".into()) };
        let m = envelope_any(&raw)?;
        match (m["type"].as_str(), m["payload"]["status"].as_str(), &m["connection_id"]) {
            (Some("hello"), Some("waiting"), Value::Null) => link.state.store(RUNNING, Relaxed),
            (Some("hello"), Some("ready"), Value::String(id)) if auth::uuid_v4_lower(id).is_some() => {
                routes.retain(|_, tx| !tx.is_closed());
                if routes.contains_key(id) || (once && first.is_some()) { return Err("signal phase".into()); }
                link.state.store(RUNNING, Relaxed);
                link.ready.store(true, Relaxed);
                hlog(&format!("ready: cliente {id}"));
                let (tx, inbox) = mpsc::unbounded_channel();
                routes.insert(id.clone(), tx);
                let task = tokio::spawn(attempt(id.clone(), rid, host.clone(), stop.clone(), out.clone(), inbox));
                if once { first = Some(task); } else { attempts.retain(|h| !h.is_finished()); attempts.push(task); }
            }
            // Papel de host já ocupado: `error` LIMIT_EXCEEDED + `close busy` (§4); o laço espera com backoff.
            (Some("error"), _, Value::Null) if m["payload"]["code"] == "LIMIT_EXCEEDED" => { hlog("presença: papel de host ocupado (busy)"); return Err("busy".into()) }
            // Envelope de uma tentativa: só a dona recebe. Id já encerrado: descartado (§3.6).
            (_, _, Value::String(id)) if m["type"] != "hello" => { if let Some(tx) = routes.get(id) { let _ = tx.send(raw.to_string()); } }
            (Some("close"), _, Value::Null) => return Ok(()),
            _ => return Err("signal phase".into()),
        }
    }
}
/// Uma tentativa (C2 §5): PeerConnection nova para o `connection_id` do cliente. `inbox` traz só os envelopes dela;
/// `out` vai ao WS de presença. Ao sair, avisa o Worker com `close` para liberar a vaga do cliente.
async fn attempt(id: String, rid: [u8; 16], host: Arc<Host>, stop: tokio::sync::watch::Receiver<bool>, out: mpsc::UnboundedSender<String>, inbox: mpsc::UnboundedReceiver<String>) -> Result<()> {
    let result = negotiate(&id, rid, host, stop, &out, inbox).await;
    let reason = if result.is_ok() { "done" } else { "protocol_error" };
    hlog(&format!("tentativa {id}: {}", result.as_ref().err().map_or("fim ok".into(), |e| format!("erro {e}"))));
    let _ = out.send(message("close", json!({"reason":reason}), &id));
    result
}
async fn negotiate(id: &str, rid: [u8; 16], host: Arc<Host>, mut stop: tokio::sync::watch::Receiver<bool>, out: &mpsc::UnboundedSender<String>, mut inbox: mpsc::UnboundedReceiver<String>) -> Result<()> {
    let send = |wire: String| out.send(wire).map_err(|_| "websocket closed");
    let cid = auth::uuid_v4_lower(id).ok_or("connection UUID")?;
    let (cert, local_fp) = certificate()?;
    let (tx, mut rx) = mpsc::channel(260);
    let handler = Arc::new(Handler { tx, failed: tokio::sync::Notify::new() });
    let pc = peer(cert).with_handler(handler.clone()).build().await?;
    let channel_open = tokio::sync::Notify::new();
    let mut stop2 = stop.clone();
    let attempt = async {
        let deadline = tokio::time::sleep(Duration::from_secs(30)); tokio::pin!(deadline);
        let mut offered = false; let mut ended = false; let mut candidates = 0;
        let mut channel: Option<Arc<dyn DataChannel>> = None; let mut opened = false;
        let mut file: Option<Arc<dyn DataChannel>> = None;
        let mut hs: Option<HostHandshake> = None;
        let mut client_pk: Option<[u8; 32]> = None;
        // C2 §2.1: os 30 s contam do DTLS connected, com ou sem pty-channel aberto.
        let mut connected_at: Option<Instant> = None;
        // N12 exige fechar em ≤ 1 s após revogar: poll de 1 s + reload/fechamento estourava; 250 ms deixa folga.
        let mut revocation = tokio::time::interval(Duration::from_millis(250));
        let mut live: Option<Live> = None;
        let mut out_rx: Option<mpsc::UnboundedReceiver<Out>> = None;
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        // C4: fila do pool de disco, fragmentos de download à espera da vez, pausa do file-channel.
        let mut fq: Option<mpsc::Sender<FIn>> = None;
        let mut fo: Option<mpsc::UnboundedReceiver<FOut>> = None;
        let mut fpend: std::collections::VecDeque<Vec<u8>> = Default::default();
        let (mut fpaused, mut pulling, mut fidle, mut fdirty) = (false, false, true, false);
        let mut ftick = tokio::time::interval(Duration::from_millis(100));
        let mut inbox_closed = false;
        loop {
            // Erro de auth: envia o código ao peer e encerra; o chamador fecha a PeerConnection inteira.
            let auth_error: auth::Error = tokio::select! {
                _ = &mut deadline, if !opened => return Err("handshake timeout".into()),
                _ = stopped(&mut stop2) => return Ok(()),
                _ = handler.failed.notified() => return Err("callback overflow".into()),
                event = rx.recv() => match event.ok_or("events closed")? {
                    Event::Failed => return Err("connection closed".into()),
                    Event::Connected => { hlog(&format!("tentativa {id}: DTLS conectado")); connected_at.get_or_insert_with(Instant::now); continue }
                    Event::Channel(c) => {
                        let slot = match c.label().await?.as_str() { "pty-channel" => &mut channel, "file-channel" => &mut file, _ => return Err("channel properties".into()) };
                        if slot.is_some() || !c.ordered().await? || c.max_retransmits().await?.is_some() || c.max_packet_life_time().await?.is_some() || !c.protocol().await?.is_empty() { return Err("channel properties".into()); }
                        *slot = Some(c);
                        continue;
                    }
                    Event::Ice(e) => {
                        if !offered { return Err("early local ICE".into()); }
                        let payload = serde_json::to_value(e.candidate.to_json()?)?;
                        if payload["candidate"].as_str().is_some_and(|s| !s.is_empty()) {
                            candidates += 1;
                            if candidates > 256 { return Err("ice limit".into()); }
                        }
                        let wire = message("ice", payload, id);
                        if wire.len() > 65536 { return Err("signal limit".into()); }
                        send(wire)?;
                        continue;
                    }
                },
                _ = async { tokio::time::sleep_until((connected_at.unwrap() + auth::AUTH_TIMEOUT).into()).await }, if hs.is_none() && connected_at.is_some() => auth::Error::Expired,
                _ = async { tokio::time::sleep_until(hs.as_ref().unwrap().deadline().into()).await }, if hs.as_ref().is_some_and(|h| h.state() != auth::State::Authenticated) => {
                    match hs.as_mut().unwrap().tick(Instant::now()) { Err(e) => e, Ok(()) => continue }
                }
                // ponytail: revogação vista ao reler o arquivo a cada 1 s; trocar por IPC se precisar de menos.
                _ = revocation.tick(), if client_pk.is_some() => {
                    match host.reload().map_err(|_| auth::Error::IoError).and_then(|_| host.check_trusted(&client_pk.unwrap())) {
                        Err(e) => { hlog(&format!("tentativa {id}: revogação/reload {e:?}")); if e != auth::Error::IoError { hub().s.revoke(&client_pk.unwrap(), now_ms()); files_revoke(&client_pk.unwrap()); } e }
                        Ok(()) => continue,
                    }
                }
                // C3 §3: ping a cada 10 s; 30 s sem nada recebido fecha. Em segundo plano não há heartbeat (§9 item 9).
                _ = heartbeat.tick(), if live.as_ref().is_some_and(|l| !l.bg) => {
                    let l = live.as_mut().unwrap();
                    if l.last_rx.elapsed() >= Duration::from_secs(30) { return Err("heartbeat timeout".into()); }
                    l.ping += 1;
                    l.awaiting = true;
                    let f = proto::Frame { session_id: [0; 16], seq: l.ping, op: Op::Heartbeat };
                    tokio::time::timeout(Duration::from_secs(2), channel.as_ref().unwrap().send(proto::encode_frame(&f)[..].into())).await??;
                    continue
                }
                // Saída do hub; pausa acima de 1 MiB pendente e em segundo plano (D10). A fila de 1 MiB da sessão segura o resto.
                // ponytail: cliente PC fora de foco (5 s) trata como celular em segundo plano; separar quando existir cliente PC.
                o = async { out_rx.as_mut().unwrap().recv().await }, if live.as_ref().is_some_and(|l| !l.paused && !l.bg) => {
                    let (ch, l) = (channel.as_ref().unwrap(), live.as_mut().unwrap());
                    tokio::time::timeout(Duration::from_secs(2), send_out(ch, id, l.conn, o.ok_or("hub closed")?)).await??;
                    l.paused = ch.outstanding_bytes().await? > PAUSE;
                    continue
                }
                // C4 §7: o terminal tem a vez; um fragmento só sai sem frame PTY pendente (ou com o PTY pausado).
                _ = std::future::ready(()), if !fpend.is_empty() && !fpaused && (live.as_ref().is_some_and(|l| l.paused || l.bg) || out_rx.as_ref().is_none_or(|r| r.is_empty())) => {
                    let fc = file.as_ref().ok_or("file-channel")?;
                    let f = fpend.pop_front().unwrap();
                    tokio::time::timeout(Duration::from_secs(2), fc.send(f[..].into())).await??;
                    fpaused = fc.outstanding_bytes().await? > FPAUSE;
                    continue
                }
                // Próximo chunk de download; `fidle` até chegar controle novo (file_get/file_ack).
                _ = std::future::ready(()), if fpend.is_empty() && !fpaused && !pulling && !fidle && fq.as_ref().is_some_and(|q| q.capacity() > 0) => {
                    pulling = fq.as_ref().unwrap().try_send(FIn::Pull).is_ok();
                    (fidle, fdirty) = (!pulling, false);
                    continue
                }
                _ = ftick.tick(), if fq.is_some() => { let _ = fq.as_ref().unwrap().try_send(FIn::Tick); continue }
                o = async { fo.as_mut().unwrap().recv().await }, if fo.is_some() => match o {
                    Some(FOut::Ctl(rid, v)) => {
                        let fc = file.as_ref().ok_or("file-channel")?;
                        tokio::time::timeout(Duration::from_secs(2), fc.send_text(&proto::encode_control(id, rid.as_deref(), v))).await??;
                        continue
                    }
                    Some(FOut::Frames(fr)) => { fpend.extend(fr); pulling = false; continue }
                    // Controle chegado durante o `Pull` pode ter enfileirado chunks: não dorme.
                    Some(FOut::Idle) => { (pulling, fidle) = (false, !fdirty); continue }
                    Some(FOut::Fatal(e)) => e,
                    Some(FOut::Close) => return Err("peer error".into()),
                    None => return Err("file worker".into()),
                },
                // Antes da auth qualquer mensagem fecha (C2 N11). Fila do pool cheia: para de ler o canal (§7).
                event = async { file.as_ref().unwrap().poll().await }, if file.is_some() && fq.as_ref().is_none_or(|q| q.capacity() > 0) => match event {
                    Some(DataChannelEvent::OnMessage(m)) => {
                        let Some(l) = live.as_mut() else { return Err("file-channel".into()) };
                        if fq.is_none() {
                            file.as_ref().unwrap().set_buffered_amount_low_threshold(FRESUME).await?;
                            let (q, o) = file_worker(l.conn, l.pk, id.to_owned(), host.clone());
                            (fq, fo) = (Some(q), Some(o));
                        }
                        l.last_rx = Instant::now();
                        let q = fq.as_ref().unwrap();
                        if !m.is_string { let _ = q.try_send(FIn::Frame(m.data.to_vec())); continue }
                        if !take_token(l) { auth::Error::LimitExceeded } else { (fidle, fdirty) = (false, true); let _ = q.try_send(FIn::Text(m.data.to_vec())); continue }
                    }
                    Some(DataChannelEvent::OnBufferedAmountLow) => { fpaused = false; continue }
                    // Canal fechado: a fila cai, o pool pausa as transferências (§8).
                    Some(DataChannelEvent::OnClose) | None => { (file, fq, fo) = (None, None, None); fpend.clear(); continue }
                    _ => continue,
                },
                event = async { channel.as_ref().unwrap().poll().await }, if channel.is_some() => {
                    let ch = channel.as_ref().unwrap();
                    match event {
                        Some(DataChannelEvent::OnOpen) => {
                            opened = true; channel_open.notify_one();
                            match (remote_fingerprint(&pc).await, host.reload()) {
                                (Err(_), _) => auth::Error::Unauthorized,
                                (_, Err(_)) => auth::Error::IoError,
                                (Ok(remote_fp), Ok(())) => {
                                    hs = Some(HostHandshake::new(host.clone(), Binding { connection_id: cid, room_id: rid, fp_offerer: remote_fp, fp_answerer: local_fp }, *connected_at.get_or_insert_with(Instant::now)));
                                    continue;
                                }
                            }
                        }
                        Some(DataChannelEvent::OnMessage(m)) => match hs.as_mut() {
                            Some(h) if h.state() == auth::State::Authenticated => {
                                match tokio::time::timeout(Duration::from_secs(2), c3_message(ch, id, &host, live.as_mut().unwrap(), &m.data, m.is_string)).await?? { Some(e) => e, None => continue }
                            }
                            Some(h) => {
                                let parsed = if m.is_string { std::str::from_utf8(&m.data).map_err(|_| auth::Error::InvalidMessage).and_then(|t| auth::parse_msg(t, id)) } else { Err(auth::Error::InvalidMessage) };
                                match parsed.and_then(|msg| h.handle(msg, Instant::now())) {
                                    Ok(reply) => {
                                        tokio::time::timeout(Duration::from_secs(2), ch.send_text(&reply.to_json(id))).await??;
                                        client_pk = h.client_pk();
                                        if let Some(pk) = client_pk {
                                            // C3 §1: min(local 65536, remoto) < 65536 fecha; o erro é o log local.
                                            if remote_max_message(&pc.remote_description().await.map(|d| d.sdp).unwrap_or_default()) < 65536 { return Err("LIMIT_EXCEEDED".into()); }
                                            ch.set_buffered_amount_low_threshold(RESUME).await?;
                                            let conn = NEXT_CONN.fetch_add(1, Relaxed);
                                            let (tx, rx) = mpsc::unbounded_channel();
                                            { let mut h = hub(); h.conns.insert(conn, tx); h.peers.insert(conn, pk); }
                                            out_rx = Some(rx);
                                            live = Some(Live { conn, pk, paused: false, bg: false, tokens: 100.0, refill: Instant::now(), last_rx: Instant::now(), ping: 0, awaiting: false, _reg: Reg(conn) });
                                            heartbeat.reset();
                                        }
                                        continue;
                                    }
                                    Err(e) => e,
                                }
                            }
                            None => return Err("message before open".into()),
                        },
                        Some(DataChannelEvent::OnBufferedAmountLow) => { if let Some(l) = live.as_mut() { l.paused = false; } continue }
                        Some(DataChannelEvent::OnClose) | None => return Ok(()),
                        _ => continue,
                    }
                },
                // WS de presença caído não derruba a conexão: o laço reabre a sala e esta tentativa segue pelo WebRTC.
                raw = inbox.recv(), if !inbox_closed => {
                    let Some(raw) = raw else { inbox_closed = true; continue };
                    // O Worker zera o id antes do `close` final (SPEC-C1-presenca §3.6): `close` com `null` também vale.
                    let m = envelope_any(&raw)?; let p = &m["payload"];
                    if m["connection_id"] != id && !(m["type"] == "close" && m["connection_id"].is_null()) { return Err("envelope".into()); }
                    match m["type"].as_str() {
                        Some("offer") if !offered => {
                            let sdp = p["sdp"].as_str().filter(|s| !s.trim().is_empty()).ok_or("sdp")?;
                            pc.set_remote_description(RTCSessionDescription::offer(sdp.to_owned())?).await?;
                            let answer = pc.create_answer(None).await?;
                            // O certificado gerado precisa ser o anunciado (C2 §4).
                            if !answer.sdp.to_ascii_lowercase().contains(&sdp_fingerprint(&local_fp)) { return Err("local certificate not in SDP".into()); }
                            pc.set_local_description(answer.clone()).await?;
                            let wire = message("answer", json!({"sdp":answer.sdp}), id);
                            if wire.len() > 65536 { return Err("signal limit".into()); }
                            send(wire)?; offered=true;
                        }
                        Some("ice") if offered && !ended => {
                            let c = p["candidate"].as_str().ok_or("candidate")?;
                            if p.get("sdpMid").is_none() || p.get("usernameFragment").is_none() || p.get("sdpMLineIndex").is_none() || c.len()>4096 || !(p["sdpMid"].is_null() || p["sdpMid"].is_string()) || !(p["usernameFragment"].is_null() || p["usernameFragment"].is_string()) || !(p["sdpMLineIndex"].is_null() || p["sdpMLineIndex"].as_u64().is_some_and(|n| n<=65535)) { return Err("ice".into()); }
                            ended=c.is_empty(); if !ended { candidates+=1; if candidates>256 { return Err("ice limit".into()); } }
                            pc.add_ice_candidate(serde_json::from_value(p.clone())?).await?;
                        }
                        // O Worker derruba o cliente na sala aos 300 s; com o canal já aberto a sessão segue só pelo WebRTC.
                        Some("close") if live.is_some() && matches!(p["reason"].as_str(), Some("timeout" | "peer_left")) => { inbox_closed = true; }
                        Some("close") if matches!(p["reason"].as_str(), Some("done" | "timeout" | "peer_left" | "protocol_error")) => return Ok(()),
                        Some("error") if p["code"] == "INVALID_MESSAGE" && p["message"].is_string() => return Err("signaling error".into()),
                        _ => return Err("signal phase".into())
                    }
                    continue;
                }
            };
            if let Some(ch) = &channel {
                // Drena o erro antes do close da PeerConnection; sem isso o peer pode ver só o OnClose.
                // N12: revogação fecha em ≤ 1 s (poll de 250 ms + isto). Com upload em voo o ack do Fail pode esperar um RTO de SCTP (~1 s,
                // perda no loopback/rede), então a revogação só espera 400 ms pelo ack; o cliente vê o fechamento de qualquer jeito.
                let drain = if auth_error == auth::Error::Revoked { Duration::from_millis(400) } else { Duration::from_secs(2) };
                let _ = tokio::time::timeout(drain, async {
                    ch.send_text(&Msg::Fail(auth_error).to_json(id)).await?;
                    while ch.outstanding_bytes().await? > 0 { tokio::time::sleep(Duration::from_millis(10)).await; }
                    Ok::<(), webrtc::error::Error>(())
                }).await;
            }
            return Err(auth_error.code().into());
        }
    };
    let handshake = async {
        if tokio::time::timeout(Duration::from_secs(30), channel_open.notified()).await.is_ok() {
            std::future::pending::<()>().await;
        }
    };
    let result: Result<()> = tokio::select! {
        _ = handshake => Err("handshake timeout".into()),
        // Sem teto de duração: após a auth, o heartbeat (30 s) derruba conexão morta.
        result = attempt => result,
        _ = stopped(&mut stop) => Ok(()),
    };
    let close_result = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    result?;
    close_result??;
    Ok(())
}
/// Diretório de dados padrão: `LIVE_CMD_DATA_DIR`, senão `%APPDATA%/live-cmd`, senão `~/.live-cmd`.
// ponytail: APPDATA (Windows) ou HOME; usar um crate de diretórios se precisar seguir XDG/macOS à risca.
pub fn default_data_dir() -> std::result::Result<PathBuf, String> {
    std::env::var_os("LIVE_CMD_DATA_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("live-cmd")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".live-cmd")))
        .ok_or_else(|| "data dir".into())
}
/// Sala fixa do host (`session` do QR, SPEC-C1-presenca §8): `room.txt` em `data_dir`, criada na primeira vez.
pub fn room_id(data_dir: &Path) -> std::result::Result<String, String> {
    let path = data_dir.join("room.txt");
    match std::fs::read_to_string(&path) {
        Ok(t) => auth::uuid_v4_lower(t.trim()).map(|_| t.trim().to_owned()).ok_or_else(|| "room.txt".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let room = uuid_v4()?;
            std::fs::create_dir_all(data_dir).and_then(|_| std::fs::write(&path, &room)).map_err(|e| e.to_string())?;
            Ok(room)
        }
        Err(e) => Err(e.to_string()),
    }
}
/// UUID v4 minúsculo aleatório.
fn uuid_v4() -> std::result::Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|_| "random")?;
    (b[6], b[8]) = ((b[6] & 0x0f) | 0x40, (b[8] & 0x3f) | 0x80);
    Ok(proto::uuid_str(&b))
}
/// Chave Ed25519 do host no cofre do SO, criada na primeira execução (C2 §6).
// ponytail: só o backend Windows (Credential Manager) habilitado; ativar apple-native/linux-native ao portar.
fn host_key() -> Result<SigningKey> { stored_key("host-ed25519") }
/// Chave Ed25519 guardada no cofre do SO sob `name`; criada se não existir.
fn stored_key(name: &str) -> Result<SigningKey> {
    let entry = keyring::Entry::new("live-cmd", name)?;
    let seed: [u8; 32] = match entry.get_secret() {
        Ok(s) => s.try_into().map_err(|_| "host key")?,
        Err(keyring::Error::NoEntry) => { let mut s = [0; 32]; getrandom::getrandom(&mut s).map_err(|_| "random")?; entry.set_secret(&s)?; s }
        Err(e) => return Err(e.into()),
    };
    Ok(SigningKey::from_bytes(&seed))
}

// ---- D5a: serviço do host (contrato com o app de bandeja, D5b) ----
pub struct Config {
    pub data_dir: PathBuf,
    /// URL do Worker, ex. `ws://127.0.0.1:8787`.
    pub server: String,
    /// Sala fixa (UUID v4 minúsculo); ver [`room_id`].
    pub room: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum State { Running, Connecting, Error }
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub state: State,
    /// Conexões autenticadas agora.
    pub connected: usize,
    /// Fim da OTK pendente, em ms desde a época Unix.
    pub pairing_expires_ms: Option<u64>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Device { pub client_pk: [u8; 32], pub label: String, pub paired_at_ms: u64, pub connected: bool }
struct Inner { host: Arc<Host>, server: String, room: String, link: Link, stop: tokio::sync::watch::Sender<bool>, thread: Mutex<Option<std::thread::JoinHandle<()>>> }
/// Host em segundo plano: laço de presença numa thread com runtime tokio próprio.
#[derive(Clone)]
pub struct Service(Arc<Inner>);
impl Service {
    pub fn start(cfg: Config) -> std::result::Result<Service, String> {
        let key = host_key().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&cfg.data_dir).map_err(|e| e.to_string())?;
        let host = Host::with_store(key, cfg.data_dir.join("trust.json")).map_err(|e| e.to_string())?;
        let store = cfg.data_dir.join("sessions.json");
        {
            let mut h = hub();
            h.profiles = profiles(&cfg.data_dir.join("profiles.json")).map_err(|e| e.to_string())?;
            h.restore(read_saved(&store));
            h.store = Some(store);
            h.save();
        }
        let svc = Self::with_host(cfg, host)?;
        // Debounce: grava no máximo a cada 2 s, só se algo mudou.
        std::thread::spawn(|| loop { std::thread::sleep(Duration::from_secs(2)); hub().save(); });
        Ok(svc)
    }
    fn with_host(cfg: Config, host: Arc<Host>) -> std::result::Result<Service, String> {
        auth::uuid_v4_lower(&cfg.room).ok_or("room UUID")?;
        files_open(&cfg.data_dir);
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().map_err(|e| e.to_string())?;
        let inner = Arc::new(Inner { host, server: cfg.server, room: cfg.room, link: Link::default(), stop: tokio::sync::watch::channel(false).0, thread: Mutex::new(None) });
        let i = inner.clone();
        let thread = std::thread::Builder::new().name("live-cmd-host".into()).spawn(move || rt.block_on(async move {
            // TTL de 24 h (C3 §5): varre a cada minuto.
            tokio::spawn(async { let mut t = tokio::time::interval(Duration::from_secs(60)); loop { t.tick().await; { let mut h = hub(); let dead = h.s.tick(now_ms()); h.forget(&dead); } let _ = tokio::task::spawn_blocking(files_tick).await; } });
            presence(&i).await
        })).map_err(|e| e.to_string())?;
        *inner.thread.lock().unwrap() = Some(thread);
        Ok(Service(inner))
    }
    pub fn status(&self) -> Status {
        let state = match self.0.link.state.load(Relaxed) { RUNNING => State::Running, ERROR => State::Error, _ => State::Connecting };
        let pairing_expires_ms = self.0.host.otk_remaining(Instant::now()).map(|d| now_ms() + d.as_millis() as u64);
        Status { state, connected: hub().peers.len(), pairing_expires_ms }
    }
    /// Emite OTK (300 s) e devolve o URI do QR; `server` do QR = [`Config::server`] sem o esquema. Nunca logar.
    pub fn pair(&self) -> std::result::Result<String, String> {
        let s = &self.0.server;
        self.pair_with(s.strip_prefix("ws://").or_else(|| s.strip_prefix("wss://")).unwrap_or(s))
    }
    /// Como [`Service::pair`], com o `server` do QR explícito (`host[:porta]`, C2 §1).
    pub fn pair_with(&self, server: &str) -> std::result::Result<String, String> {
        let otk = self.0.host.issue_otk(Instant::now()).map_err(|e| e.code())?;
        let qr = format!("remoteterm://pair?v=1&session={}&otk={}&host_pk={}&server={server}", self.0.room, B64.encode(otk), B64.encode(self.0.host.pk()));
        if auth::parse_qr(&qr).is_err() { self.cancel_pair(); return Err("server".into()); }
        Ok(qr)
    }
    pub fn cancel_pair(&self) { self.0.host.cancel_otk(); }
    pub fn devices(&self) -> Vec<Device> {
        let _ = self.0.host.reload();
        let live: HashSet<Pk> = hub().peers.values().copied().collect();
        let mut d: Vec<Device> = self.0.host.clients().into_iter().map(|(pk, label, at)| Device { client_pk: pk, label, paired_at_ms: at, connected: live.contains(&pk) }).collect();
        d.sort_by_key(|d| d.paired_at_ms);
        d
    }
    /// Tombstone persistido; a conexão ativa desse aparelho cai na releitura de 1 s.
    pub fn revoke(&self, client_pk: [u8; 32]) -> std::result::Result<(), String> {
        let _ = self.0.host.reload();
        self.0.host.revoke(client_pk).map_err(|e| e.code().to_owned())?;
        files_revoke(&client_pk);
        Ok(())
    }
    /// Encerra a tentativa atual e o laço; espera a thread (a PeerConnection fecha em até 2 s).
    pub fn stop(&self) {
        // Ao sair, grava já (síncrono, fora do lock do hub): o app vai fechar.
        let snap = { let mut h = hub(); h.dirty = true; h.snapshot() };
        if let Some((path, job)) = snap { write_pending(&path, job); }
        self.0.stop.send_replace(true);
        let t = self.0.thread.lock().unwrap().take();
        if let Some(t) = t { let _ = t.join(); }
    }
}
/// Revogação sem serviço rodando (CLI `--revoke`): grava o tombstone; o host em execução derruba em ≤ 1 s.
pub fn revoke_offline(data_dir: &Path, client_pk: [u8; 32]) -> std::result::Result<(), String> {
    let host = Host::with_store(host_key().map_err(|e| e.to_string())?, data_dir.join("trust.json")).map_err(|e| e.to_string())?;
    host.revoke(client_pk).map_err(|e| e.code().into())
}
/// Laço de presença (SPEC-C1-presenca §8): um WS na sala com várias tentativas em paralelo; ao cair, reabre.
/// Backoff 1 s dobrando até 30 s; zera depois de um `ready`.
async fn presence(i: &Inner) {
    let mut stop = i.stop.subscribe();
    let mut backoff = Duration::from_secs(1);
    // Tentativas vivas sobrevivem à queda do WS de presença.
    let mut attempts = Vec::new();
    while !*stop.borrow() {
        i.link.ready.store(false, Relaxed);
        let r = run(&i.server, &i.room, i.host.clone(), &i.link, stop.clone(), &mut attempts, false).await;
        hlog(&format!("presença saiu: {}", r.as_ref().err().map_or("ok".into(), |e| e.to_string())));
        let failed = r.is_err();
        let ready = i.link.ready.load(Relaxed);
        if ready { backoff = Duration::from_secs(1); } else if failed { i.link.state.store(ERROR, Relaxed); }
        tokio::select! { _ = tokio::time::sleep(backoff) => {}, _ = stopped(&mut stop) => break }
        if !ready { backoff = (backoff * 2).min(Duration::from_secs(30)); }
    }
    // Cada tentativa vê o `stop` e fecha a PeerConnection em até 2 s.
    let _ = tokio::time::timeout(Duration::from_secs(3), futures_util::future::join_all(attempts)).await;
}
#[cfg(test)]
mod tests {
    #[test]
    fn upload_dir_by_kind() {
        let (h, p) = (Path::new(r"C:\Users\u"), Path::new(r"D:\proj"));
        assert_eq!(upload_dir("print", Some(p), h), Path::new(r"D:\proj\screenshots"));
        assert_eq!(upload_dir("print", None, h), Path::new(r"C:\Users\u\Pictures\Screenshots"));
        assert_eq!(upload_dir("file", Some(p), h), Path::new(r"D:\proj\archives"));
        assert_eq!(upload_dir("file", None, h), Path::new(r"C:\Users\u\Downloads\archives"));
    }
    use super::*;
    /// Persistência: sessão salva em disco volta com mesmo id, rótulo, pasta e histórico; a fechada some do arquivo.
    #[test]
    fn sessions_persist_and_restore() {
        let dir = std::env::temp_dir().join(format!("live-cmd-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = dir.join("sessions.json");
        let prof = || vec![("test-cmd".to_string(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false })];
        let mut a = Hub { profiles: prof(), store: Some(store.clone()), ..Default::default() };
        let keep = a.open(None, "test-cmd", Some(dir.clone()), Some("build".into()), 80, 24, now_ms()).unwrap();
        let gone = a.open(None, "test-cmd", None, None, 80, 24, now_ms()).unwrap();
        a.s.on_output(keep, b"marca-persist\r\n", now_ms());
        a.handle_close_for_test(gone);
        a.save();
        let saved = read_saved(&store);
        assert_eq!(saved.len(), 1, "fechada de propósito sai do arquivo");
        assert_eq!((saved[0].id.as_str(), saved[0].label.as_str(), saved[0].cwd.as_deref()), (proto::uuid_str(&keep).as_str(), "build", Some(dir.as_path())));
        for sid in [keep] { let _ = a.s.close(sid); }
        let mut b = Hub { profiles: prof(), ..Default::default() };
        b.restore(saved);
        assert_eq!(b.s.size(&keep), Some((80, 24)), "mesmo id");
        let tail = b.s.saved(1 << 20).remove(0).4;
        let t = String::from_utf8_lossy(&tail);
        assert!(t.contains("marca-persist") && t.contains("sessão restaurada"), "{t}");
        let _ = b.s.close(keep);
        let _ = std::fs::remove_dir_all(dir);
    }
    /// Retomar agente encerrado: `session_create` com `resume_id` recria a sessão com o mesmo id; id vivo é recusado.
    #[test]
    fn create_resume_keeps_id() {
        let mut h = Hub { profiles: vec![("test-cmd".to_string(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false })], ..Default::default() };
        let id = [0x5au8; 16];
        let req = || Req::Create { profile_id: "test-cmd".into(), cols: 80, rows: 24, label: Some("app · claude".into()), cwd: None, agent: Some("claude".into()), args: None, resume_id: Some(id), ssh: None };
        let v = h.request(1, [0; 32], req(), now_ms()).unwrap();
        assert_eq!(v["session_id"], proto::uuid_str(&id));
        assert_eq!(h.ai.get(&id).map(|a| a.0.as_str()), Some("claude"));
        assert!(h.request(1, [0; 32], req(), now_ms()).is_err(), "id ainda vivo");
        let _ = h.s.close(id);
    }
    /// Celular: `ssh_connect` abre o ssh.exe direto com o destino do provedor (revalidado); `agent_resume` usa o perfil gravado.
    #[test]
    fn extras_usa_provedor() {
        use extras::Req as X;
        struct P;
        impl extras::Provider for P {
            fn ssh_servers(&self) -> Vec<extras::SshServer> { vec![extras::SshServer { server_id: "s1".into(), label: "srv".into(), host: "127.0.0.1".into(), user: "u".into(), port: 1, key: None, latency_ms: None, status: "unknown", keepalive_s: 0, auto_reconnect: false }] }
            fn agent_started(&self, a: &extras::Agent) { STARTED.lock().unwrap().push((a.session_id, a.agent.clone(), a.label.clone())); }
            fn agent_removed(&self, sid: &[u8; 16]) { STARTED.lock().unwrap().retain(|x| x.0 != *sid); }
            fn ssh_save(&self, id: Option<&str>, label: &str, t: &ssh::Ssh) -> std::result::Result<String, String> {
                if t.user == "cheio" { return Err("LIMIT_EXCEEDED".into()); }
                Ok(id.map_or_else(|| format!("novo-{label}"), str::to_owned))
            }
            fn ssh_target(&self, id: &str) -> Option<ssh::Ssh> {
                let t = |user: &str| ssh::Ssh { user: user.into(), host: "127.0.0.1".into(), port: 1, key: None, ..Default::default() };
                match id { "s1" => Some(t("u")), "bad" => Some(t("u;calc")), _ => None }
            }
            fn session_input(&self, i: &extras::Input) { INPUTS.lock().unwrap().push((i.session_id, i.data.to_owned(), i.line.clone(), i.label.clone(), i.ssh)); }
            fn command_done(&self, sid: &[u8; 16], code: Option<i32>) { DONE.lock().unwrap().push((*sid, code)); }
            fn history_command(&self, id: &str) -> Option<String> { (id == "h1").then(|| "echo de-novo".into()) }
            fn agent_default(&self) -> Option<extras::AgentDefault> { DEFAULT.lock().unwrap().clone() }
            fn agent_default_save(&self, d: &extras::AgentDefault) -> std::result::Result<(), String> { *DEFAULT.lock().unwrap() = Some(d.clone()); Ok(()) }
            fn saved_agents(&self) -> Vec<extras::Agent> {
                let a = extras::Agent { session_id: [0x6b; 16], agent: "claude".into(), args: String::new(), label: "app".into(), cwd: None, state: extras::AgentState::Saved, last_activity_ms: 1, profile_id: Some("test-b".into()), waiting: false };
                vec![extras::Agent { session_id: [0x6c; 16], args: "& calc".into(), ..a.clone() }, a]
            }
        }
        extras::set_provider(Arc::new(P));
        let prof = |id: &str| (id.to_string(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false });
        let mut h = Hub { profiles: vec![prof("test-a"), prof("test-b")], ..Default::default() };
        let v = h.extra(X::SshConnect { server_id: "s1".into(), cols: 80, rows: 24 }, now_ms()).unwrap();
        let sid = auth::uuid_bytes(v["session_id"].as_str().unwrap()).unwrap();
        assert_eq!((h.ssh.get(&sid).map(|t| t.port), h.prof.get(&sid).map(String::as_str)), (Some(1), Some(SSH_PROFILE)));
        assert!(matches!(h.extra(X::SshConnect { server_id: "bad".into(), cols: 80, rows: 24 }, now_ms()), Err(auth::Error::InvalidMessage)), "destino inválido não abre");
        assert!(matches!(h.extra(X::SshConnect { server_id: "x".into(), cols: 80, rows: 24 }, now_ms()), Err(auth::Error::NotFound)));
        let v = h.extra(X::AgentResume { session_id: [0x6b; 16], cols: 80, rows: 24 }, now_ms()).unwrap();
        assert_eq!(v["session_id"], proto::uuid_str(&[0x6b; 16]));
        assert_eq!(h.prof.get(&[0x6b; 16]).map(String::as_str), Some("test-b"), "perfil gravado, não o primeiro");
        assert!(matches!(h.extra(X::AgentResume { session_id: [0x6c; 16], cols: 80, rows: 24 }, now_ms()), Err(auth::Error::InvalidMessage)), "args do disco com metacaractere");
        assert!(STARTED.lock().unwrap().iter().any(|x| x.0 == [0x6b; 16]), "retomar pelo celular grava no registro");
        // Nova sessão de agente pelo celular: primeiro perfil, rótulo `<pasta> · <agente>`, registrada no provedor.
        let dir = std::env::temp_dir().canonicalize().unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_owned();
        let v = h.extra(X::AgentCreate { agent: "codex".into(), args: String::new(), cwd: Some(dir.clone()), label: None, cols: 80, rows: 24 }, now_ms()).unwrap();
        let created = auth::uuid_bytes(v["session_id"].as_str().unwrap()).unwrap();
        assert_eq!(h.prof.get(&created).map(String::as_str), Some("test-a"));
        let folder = dir.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap().to_owned();
        assert!(STARTED.lock().unwrap().iter().any(|x| x.0 == created && x.1 == "codex" && x.2 == format!("{folder} · codex")));
        assert!(matches!(h.extra(X::AgentCreate { agent: "codex".into(), args: String::new(), cwd: Some(r"C:\nao\existe\x".into()), label: None, cols: 80, rows: 24 }, now_ms()), Err(auth::Error::NotFound)));
        // Remover: fecha o terminal e tira do registro.
        h.extra(X::AgentRemove { session_id: created }, now_ms()).unwrap();
        assert!(!STARTED.lock().unwrap().iter().any(|x| x.0 == created));
        assert!(matches!(h.extra(X::AgentRemove { session_id: [0x7f; 16] }, now_ms()), Err(auth::Error::NotFound)));
        // Cadastro SSH: erro do provedor vira o código do protocolo.
        let t = |user: &str| ssh::Ssh { user: user.into(), host: "h".into(), port: 22, key: None, ..Default::default() };
        assert_eq!(h.extra(X::SshSave { server_id: None, label: "x".into(), target: t("u") }, now_ms()).unwrap()["server_id"], "novo-x");
        assert!(matches!(h.extra(X::SshSave { server_id: None, label: "x".into(), target: t("cheio") }, now_ms()), Err(auth::Error::LimitExceeded)));
        assert!(matches!(h.extra(X::SshSave { server_id: None, label: "x".into(), target: t("-oX") }, now_ms()), Err(auth::Error::InvalidMessage)));
        // Sessão SSH aparece em `ssh_list`; reconectar uma viva só devolve o id.
        let l = h.extra(X::SshList { ext: false }, now_ms()).unwrap();
        assert!(l["sessions"].as_array().unwrap().iter().any(|s| s["session_id"] == proto::uuid_str(&sid)));
        assert_eq!(h.extra(X::SshReconnect { session_id: sid, cols: 80, rows: 24 }, now_ms()).unwrap()["session_id"], proto::uuid_str(&sid));
        assert!(matches!(h.extra(X::SshReconnect { session_id: [0x6b; 16], cols: 80, rows: 24 }, now_ms()), Err(auth::Error::NotFound)), "não é SSH");
        let _ = h.s.close(sid);
        // Histórico: ponto comum do frame `01` (qualquer cliente) e do "Reexecutar"; reenvio não grava de novo; agente não grava.
        h.profiles.push(("test-osc".into(), Profile { label: "PowerShell".into(), command: "cmd.exe".into(), osc: true }));
        let plain = h.open(None, "test-osc", None, None, 80, 24, now_ms()).unwrap();
        for s in [plain, [0x6b; 16]] { h.request(9, [9; 32], Req::Attach { session_id: s, from_offset: None, size: None }, now_ms()).unwrap(); }
        let mine = |s: Sid| INPUTS.lock().unwrap().iter().filter(|x| x.0 == s).cloned().collect::<Vec<_>>();
        h.input(9, plain, 1, b"dir\r", now_ms()).unwrap();
        h.input(9, plain, 1, b"dir\r", now_ms()).unwrap();
        h.input(9, plain, 2, b"x", now_ms()).unwrap();
        let got = mine(plain);
        assert_eq!(got.len(), 2, "reenvio do mesmo seq não grava de novo: {got:?}");
        assert_eq!((got[0].1.as_str(), got[0].2.is_some(), got[0].3.as_str(), got[0].4), ("dir\r", true, "PowerShell", false), "rótulo do perfil, linha do cursor no Enter");
        assert_eq!((got[1].2.as_ref(), got[1].3.as_str()), (None, ""), "sem Enter não manda linha nem rótulo");
        h.input(9, [0x6b; 16], 1, b"oi\r", now_ms()).unwrap();
        assert!(mine([0x6b; 16]).is_empty(), "sessão de agente não grava");
        h.extra(X::HistoryRun { entry_id: "h1".into(), session_id: plain }, now_ms()).unwrap();
        assert_eq!(mine(plain).last().map(|x| x.1.clone()), Some("echo de-novo\r".into()), "reexecutar pelo celular grava como no desktop");
        h.pty(plain, conpty::Ev::Output(b"\x1b]133;D;3\x07".to_vec()));
        assert!(DONE.lock().unwrap().contains(&(plain, Some(3))), "código de saída vai ao provedor");
        // Padrão do agente: o mesmo registro do provedor (o app grava em `agent-default.json`).
        assert_eq!(h.extra(X::AgentDefaultGet, now_ms()).unwrap()["agent"], Value::Null);
        let d = extras::AgentDefault { agent: "codex".into(), cwd: Some(r"C:\p".into()), args: "--full-auto".into() };
        assert_eq!(h.extra(X::AgentDefaultSet(d), now_ms()).unwrap()["type"], "agent_default_set_ok");
        let g = h.extra(X::AgentDefaultGet, now_ms()).unwrap();
        assert_eq!((g["agent"].as_str(), g["cwd"].as_str(), g["args"].as_str()), (Some("codex"), Some(r"C:\p"), Some("--full-auto")));
        // Pastas: a mesma listagem do `dir_list`.
        let f = h.extra(X::FolderList(String::new()), now_ms()).unwrap();
        assert!(f["dirs"].as_array().unwrap().iter().any(|d| d == "C:\\"), "{f}");
        assert_eq!(f, extras::folder_list_ok("", &list_dirs("")));
        let _ = h.s.close(plain);
        let _ = h.s.close(sid);
        let _ = h.s.close([0x6b; 16]);
    }
    static STARTED: Mutex<Vec<([u8; 16], String, String)>> = Mutex::new(vec![]);
    static INPUTS: Mutex<Vec<([u8; 16], String, Option<String>, String, bool)>> = Mutex::new(vec![]);
    static DONE: Mutex<Vec<([u8; 16], Option<i32>)>> = Mutex::new(vec![]);
    static DEFAULT: Mutex<Option<extras::AgentDefault>> = Mutex::new(None);
    /// SSH restaurado do disco volta desconectado (sem processo) e `ssh_reconnect` abre o ssh.exe com o mesmo id.
    #[test]
    fn ssh_restaurado_fica_desconectado() {
        use extras::Req as X;
        let mut h = Hub::default();
        let id = "3f1e2d3c-4b5a-4968-8778-695a4b3c2d1e";
        let sid = auth::uuid_bytes(id).unwrap();
        let target = ssh::Ssh { user: "u".into(), host: "127.0.0.1".into(), port: 1, key: None, ..Default::default() };
        h.restore(vec![
            Saved { id: id.into(), profile: SSH_PROFILE.into(), label: "srv".into(), cwd: None, cols: 80, rows: 24, tail: B64.encode("antes"), agent: None, args: String::new(), ssh: Some(target), created: 0 },
            Saved { id: "4f1e2d3c-4b5a-4968-8778-695a4b3c2d1e".into(), profile: SSH_PROFILE.into(), label: "ruim".into(), cwd: None, cols: 80, rows: 24, tail: String::new(), agent: None, args: String::new(), created: 0,
                ssh: Some(ssh::Ssh { user: "-oProxyCommand=calc".into(), host: "h".into(), port: 22, key: None, ..Default::default() }) },
        ]);
        let list = h.s.list();
        assert_eq!(list.len(), 1, "destino inválido não volta");
        assert_eq!((list[0]["state"].as_str(), list[0]["label"].as_str()), (Some("exited"), Some("srv")), "sem processo: não pede senha sozinho");
        let s = h.ssh_sessions();
        assert_eq!((s.len(), s[0].connected), (1, false));
        h.extra(X::SshReconnect { session_id: sid, cols: 80, rows: 24 }, now_ms()).unwrap();
        let list = h.s.list();
        assert_eq!((list[0]["state"].as_str(), list[0]["label"].as_str()), (Some("running"), Some("srv")), "mesmo id e rótulo");
        assert!(h.ssh_sessions()[0].connected);
        let _ = h.s.close(sid);
    }
    fn text(data: &[u8], is_string: bool) -> Result<&str> {
        if !is_string || data.len()>16384 { return Err("message limit/type".into()); }
        Ok(std::str::from_utf8(data)?)
    }
    #[tokio::test]
    async fn handler_overflow() {
        let (tx, mut rx) = mpsc::channel(260);
        let handler = Arc::new(Handler { tx, failed: tokio::sync::Notify::new() });
        for _ in 0..260 { handler.emit(Event::Failed); }
        assert_eq!(rx.len(), 260);
        assert_eq!(handler.tx.capacity(), 0);
        assert!(futures_util::FutureExt::now_or_never(handler.failed.notified()).is_none());
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let producer = handler.clone();
        std::thread::spawn(move || { producer.emit(Event::Failed); done_tx.send(()).unwrap(); });
        done_rx.recv_timeout(Duration::from_secs(1)).expect("overflow emit blocked");
        tokio::time::timeout(Duration::from_millis(100), handler.failed.notified()).await.expect("overflow not signaled");
        assert_eq!(rx.len(), 260);
        for _ in 0..260 { assert!(matches!(rx.try_recv(), Ok(Event::Failed))); }
        assert!(matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
        assert_eq!(handler.tx.capacity(), 260);
    }
    #[tokio::test]
    async fn loopback_candidate() {
        let (tx, mut rx) = mpsc::channel(260);
        let pc = PeerConnectionBuilder::new()
            .with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() }))
            .with_udp_addrs(vec!["127.0.0.1:0"])
            .with_data_channel_send_buffer_limit(32768).build().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            pc.create_data_channel("pty-channel", None).await.unwrap();
            let offer = pc.create_offer(None).await.unwrap();
            pc.set_local_description(offer).await.unwrap();
            loop {
                if let Some(Event::Ice(e)) = rx.recv().await {
                    let candidate = e.candidate.to_json().unwrap();
                    assert!(candidate.candidate.contains("127.0.0.1"));
                    break;
                }
            }
        }).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
        assert!(result.is_ok(), "loopback ICE candidate unavailable");
    }
    #[tokio::test]
    async fn loopback_backpressure() {
        let _heavy = HEAVY.lock().await;
        const MESSAGE_BYTES: usize = 16384;
        const MAX_MESSAGES: usize = 1024;
        const BUFFER_LIMIT: usize = 32768;
        const RESUME_DEADLINE_SECS: u64 = 20;
        let started = std::time::Instant::now();
        let (tx_a, mut rx_a) = mpsc::channel(260);
        let (tx_b, mut rx_b) = mpsc::channel(260);
        let handler_a = Arc::new(Handler { tx: tx_a, failed: tokio::sync::Notify::new() });
        let handler_b = Arc::new(Handler { tx: tx_b, failed: tokio::sync::Notify::new() });
        let mut peer_a = None;
        let mut peer_b = None;
        // Inclui construção e negociação; qualquer erro passa pelo cleanup abaixo.
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(45), async {
            peer_a = Some(PeerConnectionBuilder::new().with_handler(handler_a.clone())
                .with_udp_addrs(vec!["127.0.0.1:0"])
                .with_data_channel_send_buffer_limit(BUFFER_LIMIT).build().await?);
            peer_b = Some(PeerConnectionBuilder::new().with_handler(handler_b.clone())
                .with_udp_addrs(vec!["127.0.0.1:0"])
                .with_data_channel_send_buffer_limit(BUFFER_LIMIT).build().await?);
            let a = peer_a.as_ref().ok_or("peer A ausente")?;
            let b = peer_b.as_ref().ok_or("peer B ausente")?;
            let sender = a.create_data_channel("pty-channel", Some(webrtc::data_channel::RTCDataChannelInit {
                ordered: true, ..Default::default()
            })).await?;
            let offer = a.create_offer(None).await?;
            a.set_local_description(offer.clone()).await?;
            b.set_remote_description(offer).await?;
            let answer = b.create_answer(None).await?;
            b.set_local_description(answer.clone()).await?;
            a.set_remote_description(answer).await?;
            let mut receiver: Option<Arc<dyn DataChannel>> = None;
            let (mut opened_a, mut opened_b) = (false, false);
            while !opened_a || !opened_b {
                tokio::select! {
                    _ = handler_a.failed.notified() => return Err("overflow A".into()),
                    _ = handler_b.failed.notified() => return Err("overflow B".into()),
                    event = rx_a.recv() => match event {
                        Some(Event::Ice(e)) => b.add_ice_candidate(e.candidate.to_json()?).await?,
                        Some(Event::Connected) => {}
                        _ => return Err("evento inesperado A".into()),
                    },
                    event = rx_b.recv() => match event {
                        Some(Event::Ice(e)) => a.add_ice_candidate(e.candidate.to_json()?).await?,
                        Some(Event::Channel(c)) if receiver.is_none() => receiver = Some(c),
                        Some(Event::Connected) => {}
                        _ => return Err("evento inesperado B".into()),
                    },
                    event = sender.poll(), if !opened_a => match event {
                        Some(DataChannelEvent::OnOpen) => opened_a = true,
                        _ => return Err("abertura A falhou".into()),
                    },
                    event = async { receiver.as_ref().ok_or("receptor ausente")?.poll().await.ok_or("receptor fechado") }, if receiver.is_some() && !opened_b => match event? {
                        DataChannelEvent::OnOpen => opened_b = true,
                        _ => return Err("abertura B falhou".into()),
                    },
                }
            }
            let receiver = receiver.ok_or("receptor ausente")?;
            for channel in [&sender, &receiver] {
                if channel.label().await? != "pty-channel" || !channel.ordered().await?
                    || channel.max_retransmits().await?.is_some() || channel.max_packet_life_time().await?.is_some() {
                    return Err("canal não confiável/ordenado".into());
                }
            }
            // Conserva o receptor sem poll; reserva uma mensagem do orçamento para o probe.
            // O teto cobre payloads tentados, não RSS nem buffers internos do runtime.
            for index in 0..MAX_MESSAGES - 1 {
                let payload = format!("{index:0>16384}");
                if payload.len() != MESSAGE_BYTES { return Err("tamanho do payload".into()); }
                let pending = sender.send_text(&payload);
                tokio::pin!(pending);
                let blocked_at = std::time::Instant::now();
                if let Ok(sent) = tokio::time::timeout(Duration::from_secs(2), &mut pending).await {
                    sent?;
                    continue;
                }
                let blocked_for = blocked_at.elapsed();
                let outstanding = sender.outstanding_bytes().await?;
                eprintln!("loopback_backpressure: enviados={index} pendentes=1 saldo={outstanding} bloqueio={blocked_for:?}");
                if blocked_for < Duration::from_secs(2) || outstanding < BUFFER_LIMIT {
                    return Err("timeout sem pressão comprovada".into());
                }
                let probe = format!("{:0>16384}", MAX_MESSAGES - 1);
                if !matches!(sender.try_send_text(&probe).await, Err(webrtc::error::Error::ErrSendBufferFull)) {
                    return Err("probe não rejeitado com ErrSendBufferFull".into());
                }
                let total = index + 1;
                let resumed_at = std::time::Instant::now();
                let resume = async {
                    // Reutiliza a MESMA future: o timeout anterior não cancela o envio.
                    // Receptor sem poll fecha a janela SCTP; quem reabre é a sonda de janela zero no RTO (1 s, dobra a cada
                    // tentativa, sem knob no rtc-sctp). A retomada cai no 1º, 2º ou 3º disparo (~1/3/7 s após a drenagem
                    // começar, mais com CPU disputada): o prazo cobre o backoff, não o tempo de cópia. Sem retomada = falha.
                    tokio::time::timeout(Duration::from_secs(RESUME_DEADLINE_SECS), &mut pending).await.map_err(|_| "retomada: send pendente não concluiu no prazo")??;
                    let elapsed = resumed_at.elapsed();
                    eprintln!("loopback_backpressure: retomada={elapsed:?}");
                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                };
                let drain = async {
                    let mut received = 0;
                    let mut tick = tokio::time::interval(Duration::from_millis(10));
                    loop {
                        tokio::select! {
                            event = receiver.poll() => match event {
                                Some(DataChannelEvent::OnMessage(m)) => {
                                    if received >= total || text(&m.data, m.is_string)? != format!("{received:0>16384}") {
                                        return Err("conteúdo/ordem/contagem incorretos ou probe entregue".into());
                                    }
                                    received += 1;
                                }
                                Some(DataChannelEvent::OnClose | DataChannelEvent::OnError) | None => return Err("receptor interrompido".into()),
                                _ => {}
                            },
                            _ = tick.tick() => {
                                if received == total && sender.outstanding_bytes().await? == 0 { break; }
                            }
                        }
                    }
                    // Janela limitada para detectar mensagem extra após drenar e zerar o saldo.
                    let quiet = tokio::time::sleep(Duration::from_millis(250));
                    tokio::pin!(quiet);
                    loop {
                        tokio::select! {
                            _ = &mut quiet => break,
                            event = receiver.poll() => match event {
                                Some(DataChannelEvent::OnMessage(_)) => return Err("mensagem extra/probe entregue".into()),
                                Some(DataChannelEvent::OnClose | DataChannelEvent::OnError) | None => return Err("receptor interrompido".into()),
                                _ => {}
                            }
                        }
                    }
                    let balance = sender.outstanding_bytes().await?;
                    if balance != 0 { return Err("saldo final não zerou".into()); }
                    eprintln!("loopback_backpressure: recebidos={received} saldo_final={balance} payload_tentado={} duração={:?}", (total + 1) * MESSAGE_BYTES, started.elapsed());
                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                };
                let drain_bounded = async {
                    tokio::time::timeout(Duration::from_secs(RESUME_DEADLINE_SECS + 5), drain).await.map_err(|_| "drenagem não concluiu no prazo")?
                };
                tokio::try_join!(resume, drain_bounded)?;
                return Ok(());
            }
            Err(format!("sem saturação: {} mensagens, saldo={}, teto={} bytes incluindo reserva do probe", MAX_MESSAGES - 1, sender.outstanding_bytes().await?, MAX_MESSAGES * MESSAGE_BYTES).into())
        }).await.unwrap_or_else(|_| Err("prazo global de 45s excedido".into()));
        // Tenta ambos independentemente; nenhum assert antecede o encerramento.
        let close_a = async {
            if let Some(peer) = peer_a { tokio::time::timeout(Duration::from_secs(2), peer.close()).await??; }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        };
        let close_b = async {
            if let Some(peer) = peer_b { tokio::time::timeout(Duration::from_secs(2), peer.close()).await??; }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (close_a, close_b) = tokio::join!(close_a, close_b);
        assert!(result.is_ok() && close_a.is_ok() && close_b.is_ok(),
            "loopback_backpressure: ensaio={result:?}, cleanup A={close_a:?}, cleanup B={close_b:?}");
    }
    /// Uma tentativa de `run` contra `ws://127.0.0.1:{port}`, sem laço de presença.
    fn spawn_run(port: u16, room: &str, host: Arc<Host>) -> tokio::task::JoinHandle<Result<()>> {
        let room = room.to_owned();
        tokio::spawn(async move { run(&format!("ws://127.0.0.1:{port}"), &room, host, &Link::default(), tokio::sync::watch::channel(false).1, &mut Vec::new(), true).await })
    }
    // Run Wrangler with a fresh ROOM_ID; pass the same value here.
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_echo() { wrangler_channel("reliable").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_retransmits_zero() { wrangler_channel("retransmits-zero").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_retransmits_one() { wrangler_channel("retransmits-one").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_lifetime_zero() { wrangler_channel("lifetime-zero").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_lifetime_one() { wrangler_channel("lifetime-one").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_unordered() { wrangler_channel("unordered").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_wrong_label() { wrangler_channel("wrong-label").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_second_channel() { wrangler_channel("second-channel").await; }
    /// S2 (P14) pelo Wrangler real: host em presença; dois clientes negociam ao mesmo tempo,
    /// cada um com C2 próprio e eco C3; a queda de A não derruba B nem o host.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires local Wrangler and fresh ROOM_ID"]
    async fn wrangler_two_clients() {
        let room = std::env::var("ROOM_ID").unwrap();
        let port = std::env::var("PORT").unwrap_or("8787".into()).parse::<u16>().unwrap();
        let (h, _, _tmp) = store("wrangler-two");
        hub().profiles.push(("test-cmd".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let hh = h.clone();
        let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let url = format!("ws://127.0.0.1:{port}");
        let r = room.clone();
        let host = tokio::spawn(async move { run(&url, &r, hh, &Link::default(), stop_rx, &mut Vec::new(), false).await });
        type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
        // Cliente: WS próprio na sala, connection_id sorteado, offer só após `ready` (SPEC-C1-presenca §3).
        async fn client(port: u16, room: &str) -> R2<(impl PeerConnection, Arc<dyn DataChannel>, Binding, String, Ws)> {
            let s = |e: Box<dyn std::error::Error + Send + Sync>| e.to_string();
            let mut b = [0u8; 16]; getrandom::getrandom(&mut b).unwrap();
            (b[6], b[8]) = ((b[6] & 0x0f) | 0x40, (b[8] & 0x3f) | 0x80);
            let id = proto::uuid_str(&b);
            let (cert, local_fp) = certificate().map_err(s)?;
            let (tx, mut events) = mpsc::channel(260);
            let pc = peer(cert).with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() })).build().await.map_err(|x| x.to_string())?;
            let dc = pc.create_data_channel("pty-channel", None).await.map_err(|x| x.to_string())?;
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws/{room}")).await.map_err(|x| x.to_string())?;
            ws.send(message("hello", json!({"role":"mobile"}), &id).into()).await.map_err(|x| x.to_string())?;
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    tokio::select! {
                        f = ws.next() => {
                            let f = f.ok_or("ws ended")?.map_err(|x| x.to_string())?;
                            let m = envelope_any(f.to_text().map_err(|x| x.to_string())?).map_err(s)?;
                            if m["connection_id"] != id.as_str() && !m["connection_id"].is_null() { return Err(format!("vazamento de connection_id: {m}")); }
                            match m["type"].as_str() {
                                Some("hello") if m["payload"]["status"] == "ready" => {
                                    let o = pc.create_offer(None).await.map_err(|x| x.to_string())?;
                                    pc.set_local_description(o.clone()).await.map_err(|x| x.to_string())?;
                                    ws.send(message("offer", json!({"sdp":o.sdp}), &id).into()).await.map_err(|x| x.to_string())?;
                                }
                                Some("hello") => {}
                                Some("answer") => pc.set_remote_description(RTCSessionDescription::answer(m["payload"]["sdp"].as_str().ok_or("answer")?.into()).map_err(|x| x.to_string())?).await.map_err(|x| x.to_string())?,
                                Some("ice") => pc.add_ice_candidate(serde_json::from_value(m["payload"].clone()).map_err(|x| x.to_string())?).await.map_err(|x| x.to_string())?,
                                _ => return Err(format!("{m}")),
                            }
                        }
                        Some(ev) = events.recv() => match ev {
                            Event::Ice(c) => ws.send(message("ice", serde_json::to_value(c.candidate.to_json().map_err(|x| x.to_string())?).map_err(|x| x.to_string())?, &id).into()).await.map_err(|x| x.to_string())?,
                            Event::Failed => return Err("peer failed".to_owned()),
                            _ => {}
                        },
                        ev = dc.poll() => match ev { Some(DataChannelEvent::OnOpen) => return Ok(()), Some(DataChannelEvent::OnClose) | None => return Err("closed".to_owned()), _ => {} },
                    }
                }
            }).await.map_err(|_| "open timeout".to_owned())??;
            let b = Binding { connection_id: auth::uuid_bytes(&id).ok_or("id")?, room_id: auth::uuid_bytes(room).ok_or("room")?, fp_offerer: local_fp, fp_answerer: remote_fingerprint(&pc).await.map_err(s)? };
            Ok((pc, dc, b, id, ws))
        }
        let result: R2<()> = tokio::time::timeout(Duration::from_secs(60), async {
            // As duas negociações correm ao mesmo tempo pelo Worker real.
            let (a, b) = tokio::join!(client(port, &room), client(port, &room));
            let ((pc_a, dc_a, bind_a, id_a, mut ws_a), (pc_b, dc_b, bind_b, id_b, ws_b)) = (a?, b?);
            if id_a == id_b { return Err("ids iguais".to_owned()); }
            // OTK é de uso único: um pareamento por vez, com as duas conexões abertas.
            let otk = h.issue_otk(Instant::now()).map_err(|e| e.code().to_owned())?;
            authenticate_dc(&dc_a, &id_a, bind_a, Some(otk), SigningKey::from_bytes(&[0x41; 32]), h.pk()).await?;
            let otk = h.issue_otk(Instant::now()).map_err(|e| e.code().to_owned())?;
            authenticate_dc(&dc_b, &id_b, bind_b, Some(otk), SigningKey::from_bytes(&[0x42; 32]), h.pk()).await?;
            let (ea, eb) = tokio::join!(c3_echo_once(&dc_a, &id_a), c3_echo_once(&dc_b, &id_b));
            ea?; eb?;
            // Queda de A (peer e WS); B segue ecoando e o host segue na sala.
            let _ = tokio::time::timeout(Duration::from_secs(2), pc_a.close()).await;
            let _ = ws_a.close(None).await;
            tokio::time::sleep(Duration::from_millis(500)).await;
            echo(&dc_b, &id_b).await?;
            c3_echo_once(&dc_b, &id_b).await?;
            if host.is_finished() { return Err("host saiu da presença".to_owned()); }
            let _ = tokio::time::timeout(Duration::from_secs(2), pc_b.close()).await;
            drop(ws_b);
            Ok(())
        }).await.unwrap_or_else(|_| Err("integration timeout".into()));
        let alive = !host.is_finished();
        host.abort(); let _ = host.await;
        assert!(result.is_ok() && alive, "wrangler_two_clients: client={result:?}, host vivo={alive}");
    }

    async fn wrangler_channel(case: &str) {
        use webrtc::data_channel::RTCDataChannelInit;
        let init = RTCDataChannelInit {
            ordered: case != "unordered",
            max_retransmits: match case { "retransmits-zero" => Some(0), "retransmits-one" => Some(1), _ => None },
            max_packet_life_time: match case { "lifetime-zero" => Some(0), "lifetime-one" => Some(1), _ => None },
            ..Default::default()
        };
        let reject = case != "reliable";
        let room = std::env::var("ROOM_ID").unwrap();
        // O cliente sorteia o connection_id da tentativa (SPEC-C1-presenca §3).
        let mut b = [0u8; 16]; getrandom::getrandom(&mut b).unwrap();
        (b[6], b[8]) = ((b[6] & 0x0f) | 0x40, (b[8] & 0x3f) | 0x80);
        let id = proto::uuid_str(&b);
        assert!(uuid(&room) && auth::uuid_v4_lower(&id).is_some());
        let port = std::env::var("PORT").unwrap_or("8787".into()).parse::<u16>().unwrap();
        // C2: trust store temporária + OTK; o cliente pareia antes de C3 (reliable/second-channel).
        let (h, _, _tmp) = store(&format!("wrangler-{case}"));
        let otk = h.issue_otk(Instant::now()).unwrap();
        hub().profiles.push(("test-cmd".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let mut host = spawn_run(port, &room, h.clone());
        let (tx, mut rx) = mpsc::channel(260);
        let (cert, local_fp) = certificate().unwrap();
        let pc = peer(cert).with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() })).build().await.unwrap();
        let mut host_result = None;
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(30), async {
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws/{room}")).await?;
            ws.send(message("hello", json!({"role":"mobile"}), &id).into()).await?;
            let dc = pc.create_data_channel(if case == "wrong-label" { "wrong-channel" } else { "pty-channel" }, Some(init)).await?;
            loop {
                tokio::select! {
                    outcome = &mut host => {
                        host_result = Some(outcome);
                        return Err("host exited".into());
                    }
                    frame = ws.next() => {
                        let frame = frame.ok_or("ws ended")??;
                        let m = envelope_any(frame.to_text()?)?;
                        // `null` antes do `ready` (cliente chegou primeiro: `waiting`) e no `close` final.
                        if m["connection_id"] != id.as_str() && !m["connection_id"].is_null() { return Err("envelope".into()); }
                        match m["type"].as_str() {
                            Some("hello") if m["payload"]["status"] == "ready" => {
                                let offer = pc.create_offer(None).await?;
                                pc.set_local_description(offer.clone()).await?;
                                ws.send(message("offer",json!({"sdp":offer.sdp}),&id).into()).await?;
                            }
                            Some("hello") => {},
                            Some("answer") => pc.set_remote_description(RTCSessionDescription::answer(m["payload"]["sdp"].as_str().ok_or("answer")?.into())?).await?,
                            Some("ice") => pc.add_ice_candidate(serde_json::from_value(m["payload"].clone())?).await?,
                            _ => return Err("unexpected signaling".into())
                        }
                    }
                    event = rx.recv() => match event {
                        Some(Event::Ice(e)) => ws.send(message("ice",serde_json::to_value(e.candidate.to_json()?)?,&id).into()).await?,
                        Some(Event::Failed) | None => return Err("peer failed".into()),
                        _ => {}
                    },
                    event = dc.poll() => match event {
                        Some(DataChannelEvent::OnOpen) => {
                            if reject && case != "second-channel" {
                                host_result = Some(tokio::time::timeout(Duration::from_secs(5), &mut host).await?);
                                return Ok(());
                            }
                            let b = Binding { connection_id: auth::uuid_bytes(&id).ok_or("id")?, room_id: auth::uuid_bytes(&room).ok_or("room")?, fp_offerer: local_fp, fp_answerer: remote_fingerprint(&pc).await? };
                            authenticate_dc(&dc, &id, b, Some(otk), SigningKey::from_bytes(&[0x61; 32]), h.pk()).await?;
                            if reject {
                                // Primeiro canal aceito (C2 completo); um segundo `pty-channel` viola C3 §1.
                                pc.create_data_channel("pty-channel", None).await?;
                                host_result = Some(tokio::time::timeout(Duration::from_secs(5), &mut host).await?);
                                return Ok(());
                            }
                            c3_echo_once(&dc, &id).await?;
                            ws.close(None).await?;
                            host_result = Some(tokio::time::timeout(Duration::from_secs(5), &mut host).await?);
                            return Ok(());
                        },
                        Some(DataChannelEvent::OnClose) | None => return Err("channel closed".into()),
                        _ => {}
                    }
                }
            }
        }).await.unwrap_or_else(|_| Err("integration timeout".into()));
        let verdict = if reject {
            // A close/timeout alone is not rejection: require the unchanged host guard.
            if host_result.is_none() {
                host_result = tokio::time::timeout(Duration::from_secs(5), &mut host).await.ok();
            }
            let rejected = matches!(&host_result,
                Some(Ok(Err(e))) if e.to_string() == "channel properties");
            rejected && (case != "second-channel" || result.is_ok())
        } else { result.is_ok() && matches!(&host_result, Some(Ok(Ok(())))) };
        let _ = tokio::time::timeout(Duration::from_secs(2),pc.close()).await;
        if !verdict && !host.is_finished() { host.abort(); let _ = host.await; }
        assert!(verdict, "local channel case {case} failed: client={result:?}, host={host_result:?}");
    }
    #[test] fn boundaries() {
        assert!(text(b"",true).is_ok()); assert_eq!(text("ação 🦀".as_bytes(),true).unwrap(),"ação 🦀");
        assert!(text(&vec![b'a';16384],true).is_ok()); assert!(text(&vec![b'a';16385],true).is_err());
        assert!(text(b"x",false).is_err()); assert!(text(&[255],true).is_err());
        assert!(uuid("01234567-0123-0123-0123-0123456789ab")); assert!(!uuid("x"));
        assert!(envelope(&message("hello",json!({}),"id"),"id").is_ok()); assert!(envelope(&message("hello",json!({}),"other"),"id").is_err());
    }
    // --- D2b: loopback com sinalização falsa no próprio teste; peer Rust no papel offerer. ---
    static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    const ROOM: &str = "10111213-1415-4617-9819-1a1b1c1d1e1f";
    type R2<T> = std::result::Result<T, String>;
    struct TmpDir(PathBuf);
    impl Drop for TmpDir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn store(name: &str) -> (Arc<Host>, PathBuf, TmpDir) {
        let dir = std::env::temp_dir().join(format!("live-cmd-d2b-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trust.json");
        (Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap(), path, TmpDir(dir))
    }
    struct Loopback { dc: Arc<dyn DataChannel>, fc: Option<Arc<dyn DataChannel>>, host: tokio::task::JoinHandle<Result<()>>, keep: tokio::task::JoinHandle<()>, b: Binding, id: String }
    async fn loopback(host: Arc<Host>, file: bool) -> Result<(impl PeerConnection, Loopback)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let task = spawn_run(port, ROOM, host);
        offer_on(&listener, task, file).await
    }
    /// Lado cliente de uma tentativa: Worker falso em `listener` + offerer com `connection_id` novo.
    async fn offer_on(listener: &tokio::net::TcpListener, task: tokio::task::JoinHandle<Result<()>>, file: bool) -> Result<(impl PeerConnection + use<>, Loopback)> {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let id = format!("00000000-0000-4000-8000-0000000000{n:02x}");
        let mut ws = fake_ready(listener, &id).await?;
        let (cert, local_fp) = certificate()?;
        let (tx, mut rx) = mpsc::channel(260);
        let pc = peer(cert).with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() })).build().await?;
        let dc = pc.create_data_channel("pty-channel", None).await?;
        let fc = if file { Some(pc.create_data_channel("file-channel", None).await?) } else { None };
        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;
        ws.send(message("offer", json!({"sdp":offer.sdp}), &id).into()).await?;
        let (mut open, mut fopen) = (false, !file);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !open || !fopen {
                tokio::select! {
                    f = ws.next() => {
                        let f = f.ok_or("ws ended")??;
                        let m = envelope(f.to_text()?, &id)?;
                        match m["type"].as_str() {
                            Some("answer") => pc.set_remote_description(RTCSessionDescription::answer(m["payload"]["sdp"].as_str().ok_or("answer")?.into())?).await?,
                            Some("ice") => pc.add_ice_candidate(serde_json::from_value(m["payload"].clone())?).await?,
                            _ => return Err("signal".into()),
                        }
                    }
                    e = rx.recv() => match e {
                        Some(Event::Ice(e)) => ws.send(message("ice", serde_json::to_value(e.candidate.to_json()?)?, &id).into()).await?,
                        Some(Event::Failed) | None => return Err("peer failed".into()),
                        _ => {}
                    },
                    e = dc.poll(), if !open => match e {
                        Some(DataChannelEvent::OnOpen) => open = true,
                        Some(DataChannelEvent::OnClose) | None => return Err("pty closed".into()),
                        _ => {}
                    },
                    e = async { fc.as_ref().unwrap().poll().await }, if !fopen => match e {
                        Some(DataChannelEvent::OnOpen) => fopen = true,
                        Some(DataChannelEvent::OnClose) | None => return Err("file closed".into()),
                        _ => {}
                    },
                }
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }).await??;
        let remote_fp = remote_fingerprint(&pc).await?;
        let b = Binding { connection_id: auth::uuid_bytes(&id).unwrap(), room_id: auth::uuid_bytes(ROOM).unwrap(), fp_offerer: local_fp, fp_answerer: remote_fp };
        let keep = tokio::spawn(async move { while let Some(Ok(_)) = ws.next().await {} });
        Ok((pc, Loopback { dc, fc, host: task, keep, b, id }))
    }
    /// Worker falso: aceita o WS do host, confere `hello {role:"host"}` com `connection_id: null` e responde `ready` com `id`.
    async fn fake_ready(listener: &tokio::net::TcpListener, id: &str) -> Result<tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>> {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let mut ws = tokio_tungstenite::accept_async(stream).await?;
        let first = envelope_any(ws.next().await.ok_or("ws")??.to_text()?)?;
        if first["type"] != "hello" || first["payload"]["role"] != "host" || !first["connection_id"].is_null() { return Err("host hello".into()); }
        ws.send(json!({"v":1,"type":"hello","connection_id":null,"request_id":"w","payload":{"status":"waiting"}}).to_string().into()).await?;
        ws.send(message("hello", json!({"status":"ready"}), id).into()).await?;
        Ok(ws)
    }
    async fn next_text(dc: &Arc<dyn DataChannel>) -> R2<String> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match dc.poll().await {
                    Some(DataChannelEvent::OnMessage(m)) => return Ok(String::from_utf8_lossy(&m.data).into_owned()),
                    Some(DataChannelEvent::OnClose) | None => return Err("closed".to_owned()),
                    _ => {}
                }
            }
        }).await.map_err(|_| "recv timeout".to_owned())?
    }
    async fn authenticate(l: &Loopback, b: Binding, otk: Option<[u8; 32]>, sk: SigningKey, pin: [u8; 32]) -> R2<()> {
        authenticate_dc(&l.dc, &l.id, b, otk, sk, pin).await
    }
    async fn authenticate_dc(dc: &Arc<dyn DataChannel>, id: &str, b: Binding, otk: Option<[u8; 32]>, sk: SigningKey, pin: [u8; 32]) -> R2<()> {
        let (mut o, hello) = auth::Offerer::new(otk, sk, pin, b, Instant::now()).map_err(|e| e.code().to_owned())?;
        dc.send_text(&hello.to_json(id)).await.map_err(|e| e.to_string())?;
        loop {
            let m = auth::parse_msg(&next_text(dc).await?, id).map_err(|e| e.code().to_owned())?;
            if let Msg::Fail(e) = m { return Err(e.code().to_owned()); }
            match o.handle(m, Instant::now()).map_err(|e| e.code().to_owned())? {
                Some(p) => dc.send_text(&p.to_json(id)).await.map_err(|e| e.to_string())?,
                None => return Ok(()),
            }
        }
    }
    /// C3 ponta a ponta: create → attach → um input → saída (eco do cmd + resultado) e um `input_ack`,
    /// sem duplicar nos 500 ms seguintes → close → `05`.
    async fn c3_echo_once(dc: &Arc<dyn DataChannel>, id: &str) -> R2<()> {
        let ctl = |t: Value| { let mut t = t; t["v"] = 1.into(); t["connection_id"] = id.into(); t["request_id"] = RID.into(); t.to_string() };
        dc.send_text(&ctl(json!({"type":"session_create","profile_id":"test-cmd","cols":80,"rows":24}))).await.map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&next_text(dc).await?).map_err(|e| e.to_string())?;
        let sid_s = v["session_id"].as_str().filter(|_| v["type"] == "session_create_ok").ok_or(format!("{v}"))?.to_owned();
        let sid = auth::uuid_bytes(&sid_s).ok_or("sid")?;
        dc.send_text(&ctl(json!({"type":"session_attach","session_id":sid_s,"from_offset":null}))).await.map_err(|e| e.to_string())?;
        let input = proto::encode_frame(&proto::Frame { session_id: sid, seq: 1, op: Op::Input(b"echo wrangler-ok\r".to_vec()) });
        dc.send(input[..].into()).await.map_err(|e| e.to_string())?;
        let (mut out, mut acks) = (Vec::new(), 0);
        let read = |m: (bool, Vec<u8>), out: &mut Vec<u8>, acks: &mut usize| -> R2<()> {
            match m {
                (true, t) => if serde_json::from_slice::<Value>(&t).map_err(|e| e.to_string())?["type"] == "input_ack" { *acks += 1 },
                (false, b) => if let Op::Output(d) = proto::parse_frame(&b).map_err(|e| format!("{e:?}"))?.op { out.extend(d) },
            }
            Ok(())
        };
        while !(String::from_utf8_lossy(&out).matches("wrangler-ok").count() >= 2 && acks >= 1) {
            read(next_msg(dc).await?, &mut out, &mut acks)?;
        }
        let quiet = tokio::time::sleep(Duration::from_millis(500));
        tokio::pin!(quiet);
        loop {
            tokio::select! { _ = &mut quiet => break, m = next_msg(dc) => read(m?, &mut out, &mut acks)? }
        }
        // Conta na tela renderizada: o ConPTY pode repintar linhas já mostradas, e o fluxo bruto contaria a mesma linha 2x.
        // Execução duplicada ainda aparece como 4 ocorrências (comando + saída, duas vezes).
        let mut screen = vt100::Parser::new(24u16.try_into().unwrap(), 80u16.try_into().unwrap(), 0);
        screen.process(&out);
        let n = screen.screen().contents().matches("wrangler-ok").count();
        if n != 2 || acks != 1 { return Err(format!("duplicado: saída={n} acks={acks}")); }
        dc.send_text(&ctl(json!({"type":"session_close","session_id":sid_s}))).await.map_err(|e| e.to_string())?;
        loop {
            if let (false, b) = next_msg(dc).await? && let Op::Exited(_) = proto::parse_frame(&b).map_err(|e| format!("{e:?}"))?.op { return Ok(()); }
        }
    }
    async fn host_error(h: tokio::task::JoinHandle<Result<()>>, secs: u64) -> String {
        match tokio::time::timeout(Duration::from_secs(secs), h).await {
            Ok(Ok(Err(e))) => e.to_string(),
            other => format!("{other:?}"),
        }
    }
    const RID: &str = "00000000-0000-4000-8000-000000000001";
    /// Pedido C3 autenticado e sua resposta (substitui o eco do M2).
    async fn echo(dc: &Arc<dyn DataChannel>, id: &str) -> R2<()> {
        dc.send_text(&json!({"v":1,"type":"session_list","connection_id":id,"request_id":RID}).to_string()).await.map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&next_text(dc).await?).map_err(|e| e.to_string())?;
        if v["type"] == "session_list_ok" && v["request_id"] == RID && v["sessions"].is_array() { Ok(()) } else { Err(format!("{v}")) }
    }
    async fn next_msg(dc: &Arc<dyn DataChannel>) -> R2<(bool, Vec<u8>)> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match dc.poll().await {
                    Some(DataChannelEvent::OnMessage(m)) => return Ok((m.is_string, m.data.to_vec())),
                    Some(DataChannelEvent::OnClose) | None => return Err("closed".to_owned()),
                    _ => {}
                }
            }
        }).await.map_err(|_| "recv timeout".to_owned())?
    }
    /// D3c A1/A6 no loopback: create → attach → input `echo` → saída + `input_ack` → close → `05`. ConPTY real (cmd, sem OSC).
    #[tokio::test]
    async fn d3c_loopback_create_attach_input_close() {
        hub().profiles.push(("test-cmd".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let (host, _, _tmp) = store("d3c");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[9; 32]), host.pk()).await.unwrap();
        let ctl = |t: Value| { let mut t = t; t["v"] = 1.into(); t["connection_id"] = l.id.clone().into(); t["request_id"] = RID.into(); t.to_string() };
        l.dc.send_text(&ctl(json!({"type":"session_create","profile_id":"test-cmd","cols":80,"rows":24}))).await.unwrap();
        let v: Value = serde_json::from_str(&next_text(&l.dc).await.unwrap()).unwrap();
        assert_eq!(v["type"], "session_create_ok", "{v}");
        let sid_s = v["session_id"].as_str().unwrap().to_owned();
        let sid = auth::uuid_bytes(&sid_s).unwrap();
        l.dc.send_text(&ctl(json!({"type":"session_attach","session_id":sid_s,"from_offset":null}))).await.unwrap();
        let input = proto::encode_frame(&proto::Frame { session_id: sid, seq: 1, op: Op::Input(b"echo d3c-ok\r".to_vec()) });
        l.dc.send(input[..].into()).await.unwrap();
        let (mut out, mut types, mut next) = (Vec::new(), Vec::<String>::new(), 0u64);
        while !(String::from_utf8_lossy(&out).matches("d3c-ok").count() >= 2 && types.iter().any(|t| t == "input_ack")) {
            match next_msg(&l.dc).await.unwrap() {
                (true, t) => types.push(serde_json::from_slice::<Value>(&t).unwrap()["type"].as_str().unwrap().into()),
                (false, b) => { let f = proto::parse_frame(&b).unwrap(); if let Op::Output(d) = f.op { assert_eq!(f.seq, next, "offset sem buraco"); next += d.len() as u64; out.extend(d); } }
            }
        }
        assert!(types.starts_with(&["session_attach_ok".into(), "replay_begin".into()]), "{types:?}");
        assert!(types.contains(&"cwd_changed".into()), "cmd anuncia cwd null");
        l.dc.send_text(&ctl(json!({"type":"session_close","session_id":sid_s}))).await.unwrap();
        loop {
            if let (false, b) = next_msg(&l.dc).await.unwrap() && let Op::Exited(_) = proto::parse_frame(&b).unwrap().op { break; }
        }
        { let h = hub(); assert!(h.s.size(&sid).is_none() && !h.screens.contains_key(&sid) && !h.plain.contains(&sid), "close libera a vaga e a tela"); }
        finish(pc, l).await;
    }
    /// §6: rajada acima de 100 controles → `error` LIMIT_EXCEEDED sem `request_id` e fecha.
    #[tokio::test]
    async fn d3c_control_burst_limit() {
        let (host, _, _tmp) = store("d3c-burst");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[10; 32]), host.pk()).await.unwrap();
        let m = json!({"v":1,"type":"session_list","connection_id":l.id,"request_id":RID}).to_string();
        for _ in 0..130 { if l.dc.send_text(&m).await.is_err() { break; } }
        let mut fatal = None;
        while let Ok(t) = next_text(&l.dc).await { let v: Value = serde_json::from_str(&t).unwrap(); if v["type"] == "error" { fatal = Some(v); break; } }
        let v = fatal.expect("sem error");
        assert!(v["code"] == "LIMIT_EXCEEDED" && v.get("request_id").is_none(), "{v}");
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "LIMIT_EXCEEDED");
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    #[test]
    fn d3c_max_message_size_and_profiles() {
        assert_eq!(remote_max_message("v=0
a=max-message-size:1024
"), 1024);
        assert_eq!(remote_max_message("v=0
"), 65536);
        let p = profiles(std::path::Path::new("Z:/não/existe.json")).unwrap();
        assert_eq!(p.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(), ["pwsh", "wsl"]);
        assert!(p.iter().all(|p| p.1.osc));
    }
    async fn finish(pc: impl PeerConnection, l: Loopback) {
        l.keep.abort();
        if !l.host.is_finished() { l.host.abort(); }
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    /// D5a: laço de presença do `Service`. Pareia, cai, reconecta com `connection_id` novo;
    /// recebe `busy` (outro host na sala) e volta com backoff; `stop` encerra a thread.
    #[tokio::test(flavor = "multi_thread")]
    async fn d5a_presence_reconnect_and_busy() {
        let (host, _path, tmp) = store("presence");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = format!("ws://127.0.0.1:{}", listener.local_addr().unwrap().port());
        fdir();
        let svc = Service::with_host(Config { data_dir: tmp.0.clone(), server, room: ROOM.into() }, host.clone()).unwrap();
        let c_sk = SigningKey::from_bytes(&[21; 32]);
        let c_pk = c_sk.verifying_key().to_bytes();
        let qr = svc.pair().unwrap();
        let otk = auth::parse_qr(&qr).unwrap().otk;
        assert!(svc.status().pairing_expires_ms.is_some_and(|t| t > now_ms() + 290_000));
        let idle = || tokio::spawn(async { Ok(()) });
        // 1ª tentativa: pareamento.
        let (pc, l) = offer_on(&listener, idle(), false).await.unwrap();
        let first = l.id.clone();
        authenticate(&l, l.b.clone(), Some(otk), c_sk.clone(), host.pk()).await.unwrap();
        echo(&l.dc, &l.id).await.unwrap();
        assert_eq!(svc.status().state, State::Running);
        assert!(svc.status().pairing_expires_ms.is_none(), "OTK consumida");
        assert!(svc.devices().iter().any(|d| d.client_pk == c_pk && d.connected));
        finish(pc, l).await;
        // 2ª tentativa: o host reabre o WS sozinho; reconexão com connection_id novo.
        let (pc, l) = offer_on(&listener, idle(), false).await.unwrap();
        assert_ne!(l.id, first);
        authenticate(&l, l.b.clone(), None, c_sk.clone(), host.pk()).await.unwrap();
        echo(&l.dc, &l.id).await.unwrap();
        finish(pc, l).await;
        // busy: papel de host ocupado; o host espera o backoff e tenta de novo.
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let hello = envelope_any(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        let rid = hello["request_id"].as_str().unwrap();
        for m in [json!({"v":1,"type":"error","connection_id":null,"request_id":rid,"payload":{"code":"LIMIT_EXCEEDED","message":"Room busy"}}),
                  json!({"v":1,"type":"close","connection_id":null,"request_id":rid,"payload":{"reason":"busy"}})] { ws.send(m.to_string().into()).await.unwrap(); }
        let _ = ws.close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame { code: 1013.into(), reason: "busy".into() })).await;
        let t = Instant::now();
        while svc.status().state != State::Error { assert!(t.elapsed() < Duration::from_secs(2)); tokio::time::sleep(Duration::from_millis(10)).await; }
        let (pc, l) = offer_on(&listener, idle(), false).await.unwrap();
        assert!(t.elapsed() >= Duration::from_millis(900), "backoff após busy");
        authenticate(&l, l.b.clone(), None, c_sk, host.pk()).await.unwrap();
        // Revogação pelo serviço derruba a conexão ativa.
        svc.revoke(c_pk).unwrap();
        let got = next_text(&l.dc).await.unwrap();
        assert!(auth::parse_msg(&got, &l.id) == Ok(Msg::Fail(auth::Error::Revoked)), "{got}");
        assert!(svc.devices().iter().all(|d| d.client_pk != c_pk));
        finish(pc, l).await;
        let s2 = svc.clone();
        tokio::time::timeout(Duration::from_secs(5), tokio::task::spawn_blocking(move || s2.stop())).await.unwrap().unwrap();
    }
    /// S2 (P14): dois clientes negociam ao mesmo tempo na mesma ws de presença; cada um tem C2 próprio,
    /// ecoa C3 em paralelo, e a queda de um não derruba o outro nem o socket de presença.
    #[tokio::test(flavor = "multi_thread")]
    async fn s2_two_clients_simultaneous() {
        let _heavy = HEAVY.lock().await;
        hub().profiles.push(("test-cmd".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let (host, _path, _tmp) = store("s2");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = host.clone();
        let task = tokio::spawn(async move { run(&format!("ws://127.0.0.1:{port}"), ROOM, h, &Link::default(), tokio::sync::watch::channel(false).1, &mut Vec::new(), false).await });
        let ids = ["00000000-0000-4000-8000-00000000a001", "00000000-0000-4000-8000-00000000b002"];
        // Worker falso: um WS de host, dois `ready`, envelopes roteados por connection_id.
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let first = envelope_any(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(first["type"] == "hello" && first["connection_id"].is_null());
        for id in ids { ws.send(message("hello", json!({"status":"ready"}), id).into()).await.unwrap(); }
        let (to_ws, mut from_clients) = mpsc::unbounded_channel::<String>();
        let (a_tx, a_rx) = mpsc::unbounded_channel::<Value>();
        let (b_tx, b_rx) = mpsc::unbounded_channel::<Value>();
        let closes = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = closes.clone();
        let worker = tokio::spawn(async move {
            loop {
                tokio::select! {
                    f = ws.next() => {
                        let Some(Ok(f)) = f else { return };
                        let m = envelope_any(f.to_text().unwrap()).unwrap();
                        let id = m["connection_id"].as_str().unwrap().to_owned();
                        if m["type"] == "close" { seen.lock().unwrap().push(id); continue }
                        let _ = if id == ids[0] { a_tx.send(m) } else if id == ids[1] { b_tx.send(m) } else { panic!("vazamento de connection_id") };
                    }
                    Some(w) = from_clients.recv() => { ws.send(w.into()).await.unwrap(); }
                }
            }
        });
        async fn offer(id: &'static str, to_ws: mpsc::UnboundedSender<String>, mut rx: mpsc::UnboundedReceiver<Value>) -> R2<(impl PeerConnection, Arc<dyn DataChannel>, Binding)> {
            let e = |e: Box<dyn std::error::Error + Send + Sync>| e.to_string();
            let (cert, local_fp) = certificate().map_err(e)?;
            let (tx, mut events) = mpsc::channel(260);
            let pc = peer(cert).with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() })).build().await.map_err(|x| x.to_string())?;
            let dc = pc.create_data_channel("pty-channel", None).await.map_err(|x| x.to_string())?;
            let o = pc.create_offer(None).await.map_err(|x| x.to_string())?;
            pc.set_local_description(o.clone()).await.map_err(|x| x.to_string())?;
            to_ws.send(message("offer", json!({"sdp":o.sdp}), id)).unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    tokio::select! {
                        Some(m) = rx.recv() => match m["type"].as_str() {
                            Some("answer") => pc.set_remote_description(RTCSessionDescription::answer(m["payload"]["sdp"].as_str().unwrap().into()).unwrap()).await.map_err(|x| x.to_string())?,
                            Some("ice") => pc.add_ice_candidate(serde_json::from_value(m["payload"].clone()).unwrap()).await.map_err(|x| x.to_string())?,
                            _ => return Err(format!("{m}")),
                        },
                        Some(ev) = events.recv() => if let Event::Ice(c) = ev { to_ws.send(message("ice", serde_json::to_value(c.candidate.to_json().unwrap()).unwrap(), id)).unwrap(); },
                        ev = dc.poll() => match ev { Some(DataChannelEvent::OnOpen) => return Ok(()), Some(DataChannelEvent::OnClose) | None => return Err("closed".to_owned()), _ => {} },
                    }
                }
            }).await.map_err(|_| "open timeout".to_owned())??;
            let b = Binding { connection_id: auth::uuid_bytes(id).unwrap(), room_id: auth::uuid_bytes(ROOM).unwrap(), fp_offerer: local_fp, fp_answerer: remote_fingerprint(&pc).await.map_err(e)? };
            Ok((pc, dc, b))
        }
        // As duas negociações correm ao mesmo tempo.
        let (a, b) = tokio::join!(offer(ids[0], to_ws.clone(), a_rx), offer(ids[1], to_ws.clone(), b_rx));
        let ((pc_a, dc_a, bind_a), (pc_b, dc_b, bind_b)) = (a.unwrap(), b.unwrap());
        let (sk_a, sk_b) = (SigningKey::from_bytes(&[31; 32]), SigningKey::from_bytes(&[32; 32]));
        // OTK é de uso único: um pareamento por vez, com as duas conexões abertas.
        let otk = host.issue_otk(Instant::now()).unwrap();
        authenticate_dc(&dc_a, ids[0], bind_a, Some(otk), sk_a, host.pk()).await.unwrap();
        let otk = host.issue_otk(Instant::now()).unwrap();
        authenticate_dc(&dc_b, ids[1], bind_b, Some(otk), sk_b, host.pk()).await.unwrap();
        let (ea, eb) = tokio::join!(c3_echo_once(&dc_a, ids[0]), c3_echo_once(&dc_b, ids[1]));
        ea.unwrap(); eb.unwrap();
        // Queda de A: o host avisa o Worker só com o id de A; B e o WS de presença seguem.
        let _ = tokio::time::timeout(Duration::from_secs(2), pc_a.close()).await;
        let t = Instant::now();
        while !closes.lock().unwrap().contains(&ids[0].to_owned()) { assert!(t.elapsed() < Duration::from_secs(10), "close de A"); tokio::time::sleep(Duration::from_millis(20)).await; }
        assert!(!closes.lock().unwrap().contains(&ids[1].to_owned()));
        echo(&dc_b, ids[1]).await.unwrap();
        assert!(!task.is_finished(), "socket de presença sobrevive");
        let _ = tokio::time::timeout(Duration::from_secs(2), pc_b.close()).await;
        task.abort(); worker.abort();
    }
    #[test] fn d5a_room_id_persisted() {
        let dir = std::env::temp_dir().join(format!("live-cmd-d5a-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _tmp = TmpDir(dir.clone());
        let room = room_id(&dir).unwrap();
        assert!(auth::uuid_v4_lower(&room).is_some());
        assert_eq!(room_id(&dir).unwrap(), room);
        std::fs::write(dir.join("room.txt"), "x").unwrap();
        assert!(room_id(&dir).is_err());
    }
    #[tokio::test]
    async fn d2b_pair_reconnect_revoke_during_session() {
        let (host, path, _tmp) = store("revoke");
        let c_sk = SigningKey::from_bytes(&[7; 32]);
        let c_pk = c_sk.verifying_key().to_bytes();
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), c_sk.clone(), host.pk()).await.unwrap();
        echo(&l.dc, &l.id).await.unwrap();
        finish(pc, l).await;
        assert!(std::fs::read_to_string(&path).unwrap().contains(&B64.encode(c_pk)), "pair persistido");
        // Reconexão: nova instância do host lê a store do disco.
        let host2 = Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap();
        let (pc, l) = loopback(host2.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), None, c_sk.clone(), host2.pk()).await.unwrap();
        echo(&l.dc, &l.id).await.unwrap();
        // Revogação por outro processo (outra instância), com a sessão ativa.
        Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap().revoke(c_pk).unwrap();
        let got = next_text(&l.dc).await.unwrap();
        assert!(auth::parse_msg(&got, &l.id) == Ok(Msg::Fail(auth::Error::Revoked)), "{got}");
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "REVOKED");
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
        let (pc, l) = loopback(host2.clone(), false).await.unwrap();
        assert_eq!(authenticate(&l, l.b.clone(), None, c_sk, host2.pk()).await, Err("REVOKED".into()));
        finish(pc, l).await;
    }
    /// C3 N7: revogação com sessão anexada e saída fluindo fecha em até 1 s; a sessão continua viva.
    #[tokio::test]
    async fn c3_n7_revoke_with_output_flowing() {
        let _heavy = HEAVY.lock().await;
        hub().profiles.push(("test-n7".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let (host, path, _tmp) = store("n7");
        let c_sk = SigningKey::from_bytes(&[11; 32]);
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), c_sk.clone(), host.pk()).await.unwrap();
        let ctl = |t: Value| { let mut t = t; t["v"] = 1.into(); t["connection_id"] = l.id.clone().into(); t["request_id"] = RID.into(); t.to_string() };
        l.dc.send_text(&ctl(json!({"type":"session_create","profile_id":"test-n7","cols":80,"rows":24}))).await.unwrap();
        let v: Value = serde_json::from_str(&next_text(&l.dc).await.unwrap()).unwrap();
        let sid_s = v["session_id"].as_str().unwrap().to_owned();
        let sid = auth::uuid_bytes(&sid_s).unwrap();
        l.dc.send_text(&ctl(json!({"type":"session_attach","session_id":sid_s,"from_offset":null}))).await.unwrap();
        let input = proto::encode_frame(&proto::Frame { session_id: sid, seq: 1, op: Op::Input(b"for /L %i in (1,0,2) do @echo n7-flow\r".to_vec()) });
        l.dc.send(input[..].into()).await.unwrap();
        let mut out = Vec::new();
        while String::from_utf8_lossy(&out).matches("n7-flow").count() < 20 {
            if let (false, b) = next_msg(&l.dc).await.unwrap() && let Op::Output(d) = proto::parse_frame(&b).unwrap().op { out.extend(d); }
        }
        let started = Instant::now();
        Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path).unwrap().revoke(c_sk.verifying_key().to_bytes()).unwrap();
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "REVOKED");
        assert!(started.elapsed() <= Duration::from_millis(1100), "fechou em {:?}", started.elapsed());
        assert!(hub().s.size(&sid).is_some(), "sessão viva após revogar");
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    /// C3 N11: sem nada recebido por 30 s após AUTHENTICATED, o host fecha.
    #[tokio::test]
    async fn c3_n11_heartbeat_timeout() {
        let (host, _, _tmp) = store("n11");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[12; 32]), host.pk()).await.unwrap();
        let started = Instant::now();
        // Ping do cliente com id 0, antes de qualquer ping do host, volta igual.
        let ping = proto::encode_frame(&proto::Frame { session_id: [0; 16], seq: 0, op: Op::Heartbeat });
        l.dc.send(ping[..].into()).await.unwrap();
        assert_eq!(next_msg(&l.dc).await.unwrap(), (false, ping.to_vec()));
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 45).await, "heartbeat timeout");
        assert!(started.elapsed() >= Duration::from_secs(29), "fechou cedo: {:?}", started.elapsed());
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    #[tokio::test]
    async fn d2b_n1_swapped_fingerprints_rejected() {
        let (host, _, _tmp) = store("n1");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        let mut b = l.b.clone();
        std::mem::swap(&mut b.fp_offerer, &mut b.fp_answerer);
        assert_eq!(authenticate(&l, b, Some(otk), SigningKey::from_bytes(&[7; 32]), host.pk()).await, Err("UNAUTHORIZED".into()));
        finish(pc, l).await;
    }
    #[tokio::test]
    async fn d2b_n11_nothing_before_auth() {
        // C4 N1: controle ou frame no file-channel antes de AUTHENTICATED fecha.
        for case in ["binary", "control", "file", "file-binary"] {
            let (host, _, _tmp) = store(case);
            let (pc, l) = loopback(host, case.starts_with("file")).await.unwrap();
            match case {
                "binary" => l.dc.send(b"\x01\x02"[..].into()).await.unwrap(),
                "control" => l.dc.send_text(&json!({"v":1,"type":"session_list","connection_id":l.id}).to_string()).await.unwrap(),
                "file" => l.fc.as_ref().unwrap().send_text("x").await.unwrap(),
                _ => l.fc.as_ref().unwrap().send(b"\x01"[..].into()).await.unwrap(),
            }
            let Loopback { host: h, keep, .. } = l;
            let want = if case.starts_with("file") { "file-channel" } else { "INVALID_MESSAGE" };
            assert_eq!(host_error(h, 5).await, want, "{case}");
            keep.abort();
            let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
        }
    }
    // --- D4c: file-channel no loopback (C4 §10). Tamanhos menores que os 100 MB da spec; a queda é o fim da PeerConnection. ---
    type Rx = mpsc::UnboundedReceiver<(bool, Vec<u8>)>;
    /// Testes que saturam o loopback UDP (vazão e tempo) rodam um de cada vez.
    pub(crate) static HEAVY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    /// Staging de teste único por processo (`FILES` é global): todo teste que abre `Service` chama antes, senão `FILES` fica num diretório que o teste apaga.
    pub(crate) fn fdir() -> PathBuf {
        static FRESH: std::sync::Once = std::sync::Once::new();
        let dir = std::env::temp_dir().join(format!("live-cmd-d4c-{}", std::process::id()));
        // O PID do Windows é reaproveitado e nada apaga esta pasta: o staging de um processo anterior com o mesmo PID
        // (file_id fixo dos testes) fazia o `file_upload_init` virar INVALID_MESSAGE. Começa sempre do zero.
        FRESH.call_once(|| { let _ = std::fs::remove_dir_all(&dir); });
        files_open(&dir);
        files().as_ref().unwrap().f.data_dir().to_path_buf()
    }
    fn new_fid(n: u8) -> [u8; 16] { let mut b = [n; 16]; (b[6], b[8]) = ((b[6] & 0x0f) | 0x40, (b[8] & 0x3f) | 0x80); b }
    fn fctl(id: &str, t: Value, rid: bool) -> String { let mut t = t; t["v"] = 1.into(); t["connection_id"] = id.into(); if rid { t["request_id"] = RID.into(); } t.to_string() }
    fn drain(fc: Arc<dyn DataChannel>) -> Rx {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(e) = fc.poll().await {
                match e { DataChannelEvent::OnMessage(m) => if tx.send((m.is_string, m.data.to_vec())).is_err() { break }, DataChannelEvent::OnClose => break, _ => {} }
            }
        });
        rx
    }
    /// Lê até o controle `ty`; `file_ack` no caminho é ignorado, qualquer outro controle falha o teste.
    async fn until(rx: &mut Rx, ty: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(30), async { loop {
            let (s, b) = rx.recv().await.expect("file-channel fechou");
            if !s { continue }
            let v: Value = serde_json::from_slice(&b).unwrap();
            if v["type"] == ty { return v }
            assert_eq!(v["type"], "file_ack", "{v}");
        } }).await.expect(ty)
    }
    /// Remonta fragmentos de download até `n` chunks novos completos.
    async fn got(rx: &mut Rx, have: &mut HashMap<u32, Vec<u8>>, size: u64, n: usize) {
        let done = |h: &HashMap<u32, Vec<u8>>| h.iter().filter(|(i, c)| c.len() as u32 == files::chunk_len(size, **i)).count();
        let target = done(have) + n;
        tokio::time::timeout(Duration::from_secs(30), async {
            while done(have) < target {
                let (s, b) = rx.recv().await.expect("file-channel fechou");
                assert!(!s, "{}", String::from_utf8_lossy(&b));
                let f = files::parse_frag(&b).unwrap();
                let c = have.entry(f.index).or_default();
                assert_eq!(c.len(), f.offset as usize, "fragmentos em ordem");
                c.extend(&f.data);
            }
        }).await.expect("chunks");
    }
    /// Remetente do cliente dentro da §7 (pausa ≤ 512 KiB, volta abaixo de 128 KiB).
    /// ponytail: pausa em 128 KiB, não 512 KiB: com 512 KiB no loopback UDP do Windows há perdas e RTO de ~1 s do SCTP no eco (N9). Reavaliar em rede real (E7).
    async fn put(fc: &Arc<dyn DataChannel>, fid: [u8; 16], data: &[u8], idx: impl Iterator<Item = u32>) -> R2<()> {
        let e = |e: webrtc::error::Error| e.to_string();
        for i in idx {
            let c = &data[i as usize * 65536..((i as usize + 1) * 65536).min(data.len())];
            for f in files::frags(fid, i, c) {
                if fc.outstanding_bytes().await.map_err(e)? > FRESUME as usize {
                    while fc.outstanding_bytes().await.map_err(e)? > FRESUME as usize { tokio::time::sleep(Duration::from_millis(2)).await; }
                }
                fc.send(f[..].into()).await.map_err(e)?;
            }
        }
        Ok(())
    }
    fn blob(n: usize) -> Vec<u8> { (0..n as u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect() }
    fn upload_init(id: &str, fid: &[u8; 16], name: &str, dst: &Path, data: &[u8]) -> String {
        fctl(id, json!({"type":"file_upload_init","file_id":proto::uuid_str(fid),"session_id":null,"filename":name,"target_dir":dst.to_str(),
            "size_bytes":data.len().to_string(),"chunk_size":65536,"total_chunks":files::total_chunks(data.len() as u64),"sha256":files::hex(&files::sha(data)),"overwrite":false}), true)
    }
    fn ranges(v: &Value) -> Vec<(u32, u32)> { v.as_array().unwrap().iter().map(|r| (r[0].as_u64().unwrap() as u32, r[1].as_u64().unwrap() as u32)).collect() }
    /// A1: upload cai no meio (fim da PeerConnection), volta com `file_resume`, termina com hash certo e `file_complete`.
    #[tokio::test(flavor = "multi_thread")]
    async fn d4c_a1_upload_drop_resume() {
        let _heavy = HEAVY.lock().await;
        fdir();
        let (host, _, tmp) = store("d4c-a1");
        let dst = tmp.0.join("dst"); std::fs::create_dir_all(&dst).unwrap();
        let sk = SigningKey::from_bytes(&[31; 32]);
        let otk = host.issue_otk(Instant::now()).unwrap();
        let data = blob((6 << 20) + 123);
        let total = files::total_chunks(data.len() as u64) as u32;
        let fid = new_fid(0xa1);
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), sk.clone(), host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        fc.send_text(&upload_init(&l.id, &fid, "a1.bin", &dst, &data)).await.unwrap();
        assert_eq!(until(&mut rx, "file_upload_init_ok").await["missing"]["ranges"], json!([[0, total]]));
        put(&fc, fid, &data, 0..total / 2).await.unwrap();
        until(&mut rx, "file_ack").await;
        finish(pc, l).await;
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), None, sk, host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        fc.send_text(&fctl(&l.id, json!({"type":"file_resume","file_id":proto::uuid_str(&fid)}), true)).await.unwrap();
        let r = until(&mut rx, "file_resume_ok").await;
        assert_eq!(r["direction"], "upload");
        let miss = ranges(&r["missing"]["ranges"]);
        assert!(miss.iter().map(|r| r.1 - r.0).sum::<u32>() < total, "o já confirmado não volta: {miss:?}");
        for (a, b) in miss { put(&fc, fid, &data, a..b).await.unwrap(); }
        until(&mut rx, "file_complete").await;
        assert!(std::fs::read(dst.join("a1.bin")).unwrap() == data, "hash do destino");
        assert!(!fdir().join("staging").join(proto::uuid_str(&fid)).exists(), "staging limpo");
        finish(pc, l).await;
    }
    /// A2: download com a mesma queda; `file_get` em janelas, `file_ack`, retomada e `file_complete`.
    #[tokio::test(flavor = "multi_thread")]
    async fn d4c_a2_download_drop_resume() {
        let _heavy = HEAVY.lock().await;
        fdir();
        let (host, _, tmp) = store("d4c-a2");
        let src = tmp.0.join("src.bin");
        let data = blob((3 << 20) + 77);
        std::fs::write(&src, &data).unwrap();
        let size = data.len() as u64;
        let sk = SigningKey::from_bytes(&[32; 32]);
        let otk = host.issue_otk(Instant::now()).unwrap();
        let fid = new_fid(0xa2);
        let s = proto::uuid_str(&fid);
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), sk.clone(), host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        fc.send_text(&fctl(&l.id, json!({"type":"file_download_init","file_id":s,"path":src.to_str()}), true)).await.unwrap();
        let ok = until(&mut rx, "file_download_init_ok").await;
        let total = ok["total_chunks"].as_u64().unwrap() as u32;
        assert_eq!((total, ok["sha256"].as_str().unwrap()), (49, files::hex(&files::sha(&data)).as_str()));
        let mut have = HashMap::new();
        // 16 chunks = 1 MiB: passa do limite de 512 KiB e exercita a pausa/retomada do bufferedAmount.
        fc.send_text(&fctl(&l.id, json!({"type":"file_get","file_id":s,"ranges":[[0,16]]}), true)).await.unwrap();
        got(&mut rx, &mut have, size, 16).await;
        fc.send_text(&fctl(&l.id, json!({"type":"file_ack","file_id":s,"ranges":[[0,16]]}), false)).await.unwrap();
        finish(pc, l).await;
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), None, sk, host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        fc.send_text(&fctl(&l.id, json!({"type":"file_resume","file_id":s}), true)).await.unwrap();
        assert_eq!(until(&mut rx, "file_resume_ok").await["direction"], "download");
        for (a, b) in [(16, 32), (32, total)] {
            fc.send_text(&fctl(&l.id, json!({"type":"file_get","file_id":s,"ranges":[[a,b]]}), true)).await.unwrap();
            got(&mut rx, &mut have, size, (b - a) as usize).await;
            fc.send_text(&fctl(&l.id, json!({"type":"file_ack","file_id":s,"ranges":[[a,b]]}), false)).await.unwrap();
        }
        fc.send_text(&fctl(&l.id, json!({"type":"file_complete","file_id":s}), false)).await.unwrap();
        let out: Vec<u8> = (0..total).flat_map(|i| have.remove(&i).unwrap()).collect();
        assert!(out == data, "conteúdo remontado");
        // `file_complete` apaga o manifesto: novo resume não acha a transferência.
        fc.send_text(&fctl(&l.id, json!({"type":"file_resume","file_id":s}), true)).await.unwrap();
        assert_eq!(until(&mut rx, "error").await["code"], "NOT_FOUND");
        finish(pc, l).await;
    }
    /// A7 / C3 N9: digitação durante upload a toda velocidade; eco p95 < 150 ms.
    #[tokio::test(flavor = "multi_thread")]
    async fn d4c_a7_typing_during_upload() {
        let _heavy = HEAVY.lock().await;
        fdir();
        hub().profiles.push(("test-d4c".into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false }));
        let (host, _, tmp) = store("d4c-a7");
        let dst = tmp.0.join("dst"); std::fs::create_dir_all(&dst).unwrap();
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[33; 32]), host.pk()).await.unwrap();
        let ctl = |t: Value| fctl(&l.id, t, true);
        l.dc.send_text(&ctl(json!({"type":"session_create","profile_id":"test-d4c","cols":80,"rows":24}))).await.unwrap();
        let v: Value = serde_json::from_str(&next_text(&l.dc).await.unwrap()).unwrap();
        let sid_s = v["session_id"].as_str().unwrap().to_owned();
        let sid = auth::uuid_bytes(&sid_s).unwrap();
        l.dc.send_text(&ctl(json!({"type":"session_attach","session_id":sid_s,"from_offset":null}))).await.unwrap();
        let mut out = Vec::new();
        while !out.contains(&b'>') { if let (false, b) = next_msg(&l.dc).await.unwrap() && let Op::Output(d) = proto::parse_frame(&b).unwrap().op { out.extend(d); } }
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        let data = blob(48 << 20);
        let fid = new_fid(0xa7);
        fc.send_text(&upload_init(&l.id, &fid, "a7.bin", &dst, &data)).await.unwrap();
        until(&mut rx, "file_upload_init_ok").await;
        let total = files::total_chunks(data.len() as u64) as u32;
        let up = tokio::spawn({ let fc = fc.clone(); async move { put(&fc, fid, &data, 0..total).await } });
        until(&mut rx, "file_ack").await;
        let mut lat = vec![];
        for seq in 1..=100u64 {
            assert!(!up.is_finished(), "upload acabou antes da amostra {seq}; aumentar o arquivo");
            let t = Instant::now();
            l.dc.send(proto::encode_frame(&proto::Frame { session_id: sid, seq, op: Op::Input(b"q".to_vec()) })[..].into()).await.unwrap();
            loop { if let (false, b) = next_msg(&l.dc).await.unwrap() && let Op::Output(d) = proto::parse_frame(&b).unwrap().op && d.contains(&b'q') { break } }
            lat.push(t.elapsed());
        }
        lat.sort();
        // p95 sobre 100 amostras: com 20, duas pausas isoladas (~1 s, ConPTY/agendador) já derrubavam o p95.
        let p95 = lat[lat.len() * 95 / 100 - 1];
        assert!(p95 < Duration::from_millis(150), "eco p95 {p95:?} (todas {lat:?})");
        up.abort();
        l.dc.send_text(&ctl(json!({"type":"session_close","session_id":sid_s}))).await.unwrap();
        finish(pc, l).await;
    }
    /// N12: revogação durante upload fecha em ≤ 1 s, cancela e apaga o staging; retomar fica impossível.
    #[tokio::test(flavor = "multi_thread")]
    async fn d4c_n12_revoke_during_upload() {
        let _heavy = HEAVY.lock().await;
        let staging = fdir().join("staging");
        let (host, path, tmp) = store("d4c-n12");
        let dst = tmp.0.join("dst"); std::fs::create_dir_all(&dst).unwrap();
        let sk = SigningKey::from_bytes(&[34; 32]);
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), sk.clone(), host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let mut rx = drain(fc.clone());
        let data = blob(32 << 20);
        let fid = new_fid(0x12);
        fc.send_text(&upload_init(&l.id, &fid, "n12.bin", &dst, &data)).await.unwrap();
        until(&mut rx, "file_upload_init_ok").await;
        let total = files::total_chunks(data.len() as u64) as u32;
        let up = tokio::spawn({ let fc = fc.clone(); async move { put(&fc, fid, &data, 0..total).await } });
        until(&mut rx, "file_ack").await;
        assert!(staging.join(proto::uuid_str(&fid)).exists());
        // Prazo conta de quando a revogação está gravada; abrir/gravar o store sob upload pesado não é do host.
        Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path).unwrap().revoke(sk.verifying_key().to_bytes()).unwrap();
        let started = Instant::now();
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "REVOKED");
        assert!(started.elapsed() <= Duration::from_millis(1100), "fechou em {:?}", started.elapsed());
        up.abort();
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
        assert!(!staging.join(proto::uuid_str(&fid)).exists(), "staging apagado na revogação");
        assert!(!dst.join("n12.bin").exists());
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        assert_eq!(authenticate(&l, l.b.clone(), None, sk, host.pk()).await, Err("REVOKED".into()));
        finish(pc, l).await;
    }
    /// §7: controles dos dois canais somam no mesmo balde (50/s, rajada 100); nenhum dos dois sozinho estoura.
    #[tokio::test(flavor = "multi_thread")]
    async fn d4c_joint_control_limit() {
        fdir();
        let (host, _, tmp) = store("d4c-rate");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[35; 32]), host.pk()).await.unwrap();
        let fc = l.fc.clone().unwrap(); let _rx = drain(fc.clone());
        let pty = json!({"v":1,"type":"session_list","connection_id":l.id,"request_id":RID}).to_string();
        let file = fctl(&l.id, json!({"type":"fs_list","path":tmp.0.to_str()}), true);
        for _ in 0..80 { if l.dc.send_text(&pty).await.is_err() || fc.send_text(&file).await.is_err() { break } }
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "LIMIT_EXCEEDED");
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    /// Controle inválido no file-channel após a auth: `INVALID_MESSAGE` fatal (§1).
    #[tokio::test]
    async fn d4c_invalid_control_after_auth() {
        fdir();
        let (host, _, _tmp) = store("d4c-inv");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), true).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[36; 32]), host.pk()).await.unwrap();
        l.fc.as_ref().unwrap().send_text(&fctl(&l.id, json!({"type":"file_get"}), true)).await.unwrap();
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "INVALID_MESSAGE");
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    /// Pergunta de capacidades pelo canal real: `caps_ok` com o mesmo `request_id`, conexão continua viva.
    /// (Host 0.1.8 responde NOT_FOUND para esta sessão: ver `extras::CAPS_PROBE` e a fixture C3.)
    #[tokio::test]
    async fn caps_probe_responde_sem_derrubar() {
        let (host, _, _tmp) = store("caps");
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), SigningKey::from_bytes(&[37; 32]), host.pk()).await.unwrap();
        let rid = "00000000-0000-4000-8000-0000000000c1";
        l.dc.send_text(&json!({"v":1,"type":"session_attach","connection_id":l.id,"request_id":rid,"session_id":extras::CAPS_PROBE,"from_offset":null}).to_string()).await.unwrap();
        let v: Value = serde_json::from_str(&next_text(&l.dc).await.unwrap()).unwrap();
        assert_eq!((v["type"].as_str(), v["request_id"].as_str(), v["caps"].clone()), (Some("caps_ok"), Some(rid), json!(extras::CAPS)), "{v}");
        echo(&l.dc, &l.id).await.unwrap();
        finish(pc, l).await;
    }
    #[tokio::test]
    async fn d2b_n12_auth_timeout_30s() {
        let (host, _, _tmp) = store("n12");
        let started = Instant::now();
        let (pc, l) = loopback(host, false).await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(40), async {
            loop { if let Some(DataChannelEvent::OnMessage(m)) = l.dc.poll().await { return String::from_utf8_lossy(&m.data).into_owned(); } }
        }).await.unwrap();
        assert!(auth::parse_msg(&got, &l.id) == Ok(Msg::Fail(auth::Error::Expired)), "{got}");
        let Loopback { host: h, keep, .. } = l;
        assert_eq!(host_error(h, 5).await, "EXPIRED");
        assert!(started.elapsed() >= auth::AUTH_TIMEOUT);
        keep.abort();
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    #[test]
    fn n15_no_fingerprint_bypass_or_secret_logs() {
        let bypass = concat!("with_disable_certificate", "_fingerprint_verification");
        for src in [include_str!("main.rs"), include_str!("lib.rs"), include_str!("auth.rs")] {
            assert!(!src.contains(bypass));
            // Corta no módulo de testes, não no primeiro `#[cfg(test)]` (há helpers só de teste antes dele).
            let prod = src.split(concat!("mod ", "tests {")).next().unwrap();
            // Allowlist exata: qualquer print/log novo em produção exige revisão (C2 §1). O QR só vai ao terminal no --pair.
            let allowed = [
                r#"println!("{}", qrcode::QrCode::new(qr.as_bytes())?.render::<qrcode::render::unicode::Dense1x2>().build());"#,
                r#"if result.is_err() { eprintln!("Echo attempt failed"); std::process::exit(1); }"#,
                // host.log: só fase, connection_id, URL do Worker e texto do erro; nunca chave, OTK, SDP ou terminal.
                // git_info: lê a saída do git (branch/status), não imprime nada.
                r#"let mut ch = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;"#,
                r#"let mut out = ch.stdout.take()?;"#,
                r#"if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) { let _ = writeln!(f, "{ts} {msg}"); }"#,
            ];
            for line in prod.lines().filter(|l| ["print!", "println!", "eprint", "log::", "tracing::", "dbg!", "write!(", "writeln!(", "stdout", "stderr"].iter().any(|m| l.contains(m))) {
                assert!(allowed.contains(&line.trim()), "print/log não revisado: {line}");
            }
        }
    }
}

#[cfg(test)]
mod dpapi_tests {
    #[test]
    fn dpapi_ida_e_volta() {
        let c = super::dpapi(b"senha 123", true).unwrap();
        assert!(!c.windows(5).any(|w| w == b"senha"));
        assert_eq!(super::dpapi(&c, false).unwrap(), b"senha 123");
        assert!(super::dpapi(b"texto puro antigo", false).is_none());
    }
}

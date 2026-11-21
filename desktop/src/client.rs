//! Cliente C3 do Terminal Desktop (D5c): local (hub em processo, sem rede, P5) ou remoto
//! (offerer WebRTC + C2 + C3 contra o host de outro PC, pela sala de presença).
use super::*;
use std::sync::mpsc as smpsc;

/// O que o cliente entrega ao app. Nunca contém URI, OTK nem chaves.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEvent {
    /// Controle host→cliente que não é resposta (`replay_*`, `screen_snapshot`, `session_exited`, `cwd_changed`…).
    Control(Value),
    Output { session_id: String, offset: u64, data: Vec<u8> },
    Exited { session_id: String, code: i32 },
    /// Remoto: `attempt` 0 = primeira tentativa; > 0 = reconectando.
    Connecting { attempt: u32 },
    Ready,
    /// Fim sem nova tentativa; `code` C2 §2.2 quando o host recusou.
    Closed { code: Option<String> },
}
type OnEvent = Box<dyn Fn(ClientEvent) + Send + Sync>;

/// Pin de um host remoto, guardado após o pareamento. Nada aqui é segredo.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RemotePin { pub host_pk: [u8; 32], pub room: String, pub server: String }

/// Lê o link de pareamento (mesmo URI do QR, C2 §1) e confere o servidor na allowlist.
/// Devolve o pin e a OTK; a OTK só vai para [`Client::remote`]. Nunca logar.
pub fn parse_pair_link(uri: &str, allowlist: &[&str]) -> std::result::Result<(RemotePin, [u8; 32]), String> {
    let uri = uri.trim();
    let p = auth::parse_qr(uri.find("remoteterm://").map_or(uri, |i| &uri[i..])).map_err(|e| e.code().to_owned())?;
    auth::check_server(&p.server, allowlist).map_err(|e| e.code().to_owned())?;
    Ok((RemotePin { host_pk: p.host_pk, room: p.session, server: p.server }, p.otk))
}

enum Outbound { Ctl(Value), Frame(proto::Frame), Upload(Upload) }
/// Envio de arquivo (C4 §4) pelo `file-channel`: `init` pronto, bytes em memória, resposta final em `done`.
struct Upload { fid: [u8; 16], init: Value, data: Vec<u8>, done: smpsc::Sender<std::result::Result<Option<String>, String>> }
enum Wire { Local { conn: ConnId, pk: Pk, _reg: Reg }, Remote(mpsc::UnboundedSender<Outbound>), Pipe(ipc::PipeLink) }
struct Shared {
    on: OnEvent,
    pending: Mutex<HashMap<String, smpsc::Sender<Value>>>,
    /// Último `seq` de input por sessão (C3 §4), vindo do `last_input_seq` do attach.
    seq: Mutex<HashMap<Sid, u64>>,
}
impl Shared {
    /// `session_attach_ok` ajusta o `seq`. `reset` só no primeiro attach da sessão nesta conexão;
    /// nos demais o contador nunca desce, senão frames em voo repetem número e viram duplicata.
    fn attached(&self, v: &Value, reset: bool) {
        if v["type"] == "session_attach_ok"
            && let (Some(sid), Ok(n)) = (v["session_id"].as_str().and_then(auth::uuid_bytes), proto::parse_u64(&v["last_input_seq"])) {
            let mut m = self.seq.lock().unwrap();
            let c = m.entry(sid).or_insert(n);
            *c = if reset { n } else { (*c).max(n) };
        }
    }
    fn next_seq(&self, sid: Sid) -> u64 {
        let mut m = self.seq.lock().unwrap();
        let n = m.entry(sid).or_insert(0);
        *n += 1;
        *n
    }
    /// Resposta vai para quem pediu; o resto vira evento.
    fn control(&self, v: Value) {
        self.attached(&v, false);
        let waiter = v["request_id"].as_str().and_then(|r| self.pending.lock().unwrap().remove(r));
        match waiter { Some(w) => { let _ = w.send(v); } None => (self.on)(ClientEvent::Control(v)) }
    }
    fn frame(&self, f: proto::Frame) {
        let session_id = proto::uuid_str(&f.session_id);
        match f.op {
            Op::Output(data) => (self.on)(ClientEvent::Output { session_id, offset: f.seq, data }),
            Op::Exited(code) => (self.on)(ClientEvent::Exited { session_id, code }),
            _ => {}
        }
    }
}

/// Uma conexão C3. Pedidos bloqueiam até a resposta (≤ 10 s): chamar fora da thread de UI.
pub struct Client { wire: Wire, shared: Arc<Shared> }
const REPLY: Duration = Duration::from_secs(10);

impl Client {
    /// Terminal desta máquina: fala direto com o hub do [`Service`], sem WebRTC nem auth (P5).
    /// Não conta como aparelho conectado.
    pub fn local(svc: &Service, on: impl Fn(ClientEvent) + Send + Sync + 'static) -> Client {
        let shared = Arc::new(Shared { on: Box::new(on), pending: Mutex::default(), seq: Mutex::default() });
        let conn = NEXT_CONN.fetch_add(1, Relaxed);
        let (tx, mut rx) = mpsc::unbounded_channel();
        hub().conns.insert(conn, tx);
        let s = shared.clone();
        std::thread::spawn(move || while let Some(o) = rx.blocking_recv() {
            match o {
                Out::Ctl(v) => s.control(v),
                Out::Frame(f) => {
                    let (sid, n) = (f.session_id, if let Op::Output(d) = &f.op { d.len() } else { 0 });
                    s.frame(f);
                    if n > 0 { let mut h = hub(); let outs = h.s.sent(conn, sid, n); h.deliver(outs.into_iter().map(|o| (conn, o))); }
                }
                Out::Snapshot { .. } => {}
            }
        });
        (shared.on)(ClientEvent::Ready);
        Client { wire: Wire::Local { conn, pk: svc.0.host.pk(), _reg: Reg(conn) }, shared }
    }

    /// Terminal desta máquina servido pelo daemon `live-cmd-termd` (named pipe, [`ipc`]). Mesmas mensagens do
    /// [`Client::local`]; as sessões continuam no daemon quando o app fecha. A primeira conexão é feita aqui;
    /// se cair, reconecta sozinho (`Connecting { attempt ≥ 1 }` e depois `Ready`, para a tela reanexar as abas).
    pub fn pipe(ep: ipc::Endpoint, on: impl Fn(ClientEvent) + Send + Sync + 'static) -> std::result::Result<Client, ipc::Fail> {
        let first = { let ep = ep.clone(); ipc::block(async move { ipc::connect(&ep, false).await })? };
        let shared = Arc::new(Shared { on: Box::new(on), pending: Mutex::default(), seq: Mutex::default() });
        let (tx, rx) = mpsc::unbounded_channel();
        let up = Arc::new(std::sync::atomic::AtomicBool::new(true));
        (shared.on)(ClientEvent::Ready);
        ipc::rt().spawn(pipe_loop(ep, first, shared.clone(), rx, up.clone()));
        Ok(Client { wire: Wire::Pipe(ipc::PipeLink { tx, up }), shared })
    }

    /// Outro PC: offerer na sala `pin.room`. `otk = Some` pareia (uma tentativa só);
    /// `None` reconecta, com backoff de 1 s a 30 s enquanto a falha for de rede.
    pub fn remote(pin: RemotePin, otk: Option<[u8; 32]>, on: impl Fn(ClientEvent) + Send + Sync + 'static) -> std::result::Result<Client, String> {
        Self::remote_with(stored_key("client-ed25519").map_err(|_| "IO_ERROR")?, pin, otk, on)
    }
    fn remote_with(sk: SigningKey, pin: RemotePin, otk: Option<[u8; 32]>, on: impl Fn(ClientEvent) + Send + Sync + 'static) -> std::result::Result<Client, String> {
        let shared = Arc::new(Shared { on: Box::new(on), pending: Mutex::default(), seq: Mutex::default() });
        let (tx, rx) = mpsc::unbounded_channel();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().map_err(|_| "IO_ERROR")?;
        let s = shared.clone();
        std::thread::Builder::new().name("live-cmd-client".into()).spawn(move || rt.block_on(remote_loop(pin, otk, sk, s, rx))).map_err(|_| "IO_ERROR")?;
        Ok(Client { wire: Wire::Remote(tx), shared })
    }

    /// Pedido C3 (`body` com `type` e campos, sem `v`/`connection_id`/`request_id`). Resposta `error` vira `Err(code)`.
    pub fn request(&self, body: Value) -> std::result::Result<Value, String> {
        let rid = uuid_v4()?;
        let reply = match &self.wire {
            Wire::Local { conn, pk, .. } => {
                let mut m = body;
                m["v"] = 1.into(); m["connection_id"] = "local".into(); m["request_id"] = rid.as_str().into();
                let Control::Request { req, .. } = proto::parse_control(&m.to_string(), "local").map_err(|e| e.code())? else { return Err("INVALID_MESSAGE".into()) };
                if matches!(req, Req::Focus(_)) { return Ok(json!({"type":"client_focus_ok"})); }
                let mut v = match hub().request(*conn, *pk, req, now_ms()) { Ok(v) => v, Err(e) => proto::error(e) };
                v["request_id"] = rid.into();
                self.shared.attached(&v, false);
                v
            }
            Wire::Remote(tx) => {
                let (wtx, wrx) = smpsc::channel();
                self.shared.pending.lock().unwrap().insert(rid.clone(), wtx);
                let mut m = body;
                m["request_id"] = rid.as_str().into();
                if tx.send(Outbound::Ctl(m)).is_err() { self.shared.pending.lock().unwrap().remove(&rid); return Err("NOT_CONNECTED".into()); }
                let r = wrx.recv_timeout(REPLY);
                self.shared.pending.lock().unwrap().remove(&rid);
                r.map_err(|_| "EXPIRED")?
            }
            Wire::Pipe(l) => {
                if !l.up.load(Relaxed) { return Err("NOT_CONNECTED".into()); }
                let (wtx, wrx) = smpsc::channel();
                self.shared.pending.lock().unwrap().insert(rid.clone(), wtx);
                let mut m = body;
                m["request_id"] = rid.as_str().into();
                if l.tx.send(ipc::pack_json(&json!({"t":"req","body":m}))).is_err() { self.shared.pending.lock().unwrap().remove(&rid); return Err("NOT_CONNECTED".into()); }
                let r = wrx.recv_timeout(REPLY);
                self.shared.pending.lock().unwrap().remove(&rid);
                r.map_err(|e| if matches!(e, smpsc::RecvTimeoutError::Disconnected) { "NOT_CONNECTED" } else { "EXPIRED" })?
            }
        };
        if reply["type"] == "error" { return Err(reply["code"].as_str().unwrap_or("INVALID_MESSAGE").to_owned()); }
        Ok(reply)
    }

    /// Bytes digitados (frame `01`), em pedaços de até 16 KiB, cada um com o próximo `seq`.
    pub fn input(&self, session_id: &str, data: &[u8]) -> std::result::Result<(), String> {
        let sid = auth::uuid_bytes(session_id).ok_or("INVALID_MESSAGE")?;
        for chunk in data.chunks(proto::MAX_IO) {
            // `seq` é dado na hora do envio (`next_seq`): frame descartado antes do attach não gasta número.
            self.send(proto::Frame { session_id: sid, seq: 0, op: Op::Input(chunk.to_vec()) })?;
        }
        Ok(())
    }

    /// Frame `03` (C3 §3, "último resize vence").
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> std::result::Result<(), String> {
        let sid = auth::uuid_bytes(session_id).ok_or("INVALID_MESSAGE")?;
        if !(1..=1000).contains(&cols) || !(1..=1000).contains(&rows) { return Err("INVALID_MESSAGE".into()); }
        self.send(proto::Frame { session_id: sid, seq: 0, op: Op::Resize { cols, rows } })
    }

    /// Envia `path` deste PC para `target_dir` do host (`None` = pasta padrão do host). Bloqueia até `file_complete`; devolve o caminho gravado (host antigo: `None`).
    /// ponytail: arquivo inteiro em memória e sem `file_resume`; queda no meio = erro. Ler em blocos e retomar se arquivos grandes virarem uso comum.
    pub fn upload(&self, path: &Path, target_dir: Option<&str>, session_id: Option<&str>, kind: Option<&str>) -> std::result::Result<Option<String>, String> {
        let Wire::Remote(tx) = &self.wire else { return Err("INVALID_MESSAGE".into()) };
        let filename = path.file_name().and_then(|n| n.to_str()).ok_or("INVALID_MESSAGE")?.to_owned();
        let data = std::fs::read(path).map_err(|_| "IO_ERROR")?;
        let fid = auth::uuid_bytes(&uuid_v4()?).ok_or("IO_ERROR")?;
        let init = json!({"type":"file_upload_init","file_id":proto::uuid_str(&fid),"session_id":session_id,"filename":filename,"target_dir":target_dir,
            "size_bytes":data.len().to_string(),"chunk_size":files::CHUNK,"total_chunks":files::total_chunks(data.len() as u64),"sha256":files::hex(&files::sha(&data)),"overwrite":false,"kind":kind});
        let (done, wait) = smpsc::channel();
        tx.send(Outbound::Upload(Upload { fid, init, data, done })).map_err(|_| "NOT_CONNECTED")?;
        wait.recv().map_err(|_| "NOT_CONNECTED")?
    }

    fn send(&self, f: proto::Frame) -> std::result::Result<(), String> {
        match &self.wire {
            Wire::Local { conn, .. } => {
                let mut h = hub();
                let r = match &f.op {
                    Op::Input(d) => h.input(*conn, f.session_id, self.shared.next_seq(f.session_id), d, now_ms()).map(|a| a.map(Out::Ctl)),
                    Op::Resize { cols, rows } => h.s.resize(*conn, f.session_id, *cols, *rows).map(|_| None),
                    _ => Ok(None),
                };
                h.sync(&f.session_id);
                match r { Ok(Some(o)) => { h.deliver([(*conn, o)]); Ok(()) } Ok(None) => Ok(()), Err(e) => Err(e.code().into()) }
            }
            Wire::Remote(tx) => tx.send(Outbound::Frame(f)).map_err(|_| "NOT_CONNECTED".into()),
            // O `seq` do input é dado no daemon, pelo cliente local de lá.
            Wire::Pipe(l) if l.up.load(Relaxed) => l.tx.send(ipc::pack_frame(&f)).map_err(|_| "NOT_CONNECTED".into()),
            Wire::Pipe(_) => Err("NOT_CONNECTED".into()),
        }
    }
}

/// Laço do [`Client::pipe`]: escreve a fila no pipe, entrega o que chega e reconecta (250 ms a 2 s) se cair.
/// Termina quando o `Client` é solto.
async fn pipe_loop(ep: ipc::Endpoint, first: ipc::Conn, s: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<Vec<u8>>, up: Arc<std::sync::atomic::AtomicBool>) {
    use tokio::io::AsyncWriteExt;
    let mut cur = Some(first);
    let mut attempt = 0u32;
    loop {
        let conn = match cur.take() {
            Some(c) => c,
            None => {
                attempt += 1;
                (s.on)(ClientEvent::Connecting { attempt });
                let mut wait = Duration::from_millis(250);
                loop {
                    if rx.is_closed() { return; }
                    if let Ok(c) = ipc::connect(&ep, false).await { break c; }
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(Duration::from_secs(2));
                }
            }
        };
        // O que foi enfileirado durante a queda era para a conexão anterior.
        loop { match rx.try_recv() { Ok(_) => {} Err(mpsc::error::TryRecvError::Empty) => break, Err(_) => return } }
        if attempt > 0 { up.store(true, Relaxed); (s.on)(ClientEvent::Ready); }
        let (mut r, mut w) = tokio::io::split(conn.pipe);
        let sr = s.clone();
        let mut reader = tokio::spawn(async move {
            while let Ok(m) = ipc::read_msg(&mut r).await {
                match m {
                    ipc::Msg::Json(v) if v["t"] == "ctl" => sr.control(v["v"].clone()),
                    ipc::Msg::Frame(f) => sr.frame(f),
                    _ => {}
                }
            }
        });
        let gone = loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Some(b) => if w.write_all(&b).await.is_err() { break false },
                    None => break true,
                },
                _ = &mut reader => break false,
            }
        };
        reader.abort();
        up.store(false, Relaxed);
        // Quem espera resposta recebe NOT_CONNECTED em vez de esperar o prazo.
        s.pending.lock().unwrap().clear();
        if gone { return; }
    }
}

/// `host[:porta]` do QR → URL do Worker. Loopback usa `ws://` (sinalização local); o resto `wss://`.
fn signal_url(server: &str) -> String {
    let host = server.rsplit_once(':').map_or(server, |(h, _)| h);
    if host == "127.0.0.1" || host == "localhost" { format!("ws://{server}") } else { format!("wss://{server}") }
}

/// Falha de uma tentativa: `Auth` encerra (o host recusou); `Net` tenta de novo com backoff.
enum Stop { Auth(String), Net }
impl From<Box<dyn std::error::Error + Send + Sync>> for Stop { fn from(_: Box<dyn std::error::Error + Send + Sync>) -> Self { Stop::Net } }

async fn remote_loop(pin: RemotePin, mut otk: Option<[u8; 32]>, sk: SigningKey, s: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<Outbound>) {
    let mut backoff = Duration::from_secs(1);
    for attempt in 0u32.. {
        (s.on)(ClientEvent::Connecting { attempt });
        let mut ready = false;
        let r = attempt_once(&pin, otk, &sk, &s, &mut rx, &mut ready).await;
        // Respostas pendentes da tentativa que caiu não chegam mais.
        s.pending.lock().unwrap().clear();
        if ready { otk = None; backoff = Duration::from_secs(1); }
        match r {
            // `Ok`: o app largou o `Client` (canal fechado).
            Ok(()) => return,
            Err(Stop::Auth(code)) if code != "LIMIT_EXCEEDED" => { (s.on)(ClientEvent::Closed { code: Some(code) }); return }
            // OTK é de uso único: pareamento que não chegou ao fim não se repete.
            _ if otk.is_some() => { (s.on)(ClientEvent::Closed { code: Some("EXPIRED".into()) }); return }
            _ => {}
        }
        if rx.is_closed() { return; }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

/// Uma tentativa (SPEC-C1-presenca §3): `hello` com `connection_id` novo, offer, ICE, C2, depois C3 até cair.
async fn attempt_once(pin: &RemotePin, otk: Option<[u8; 32]>, sk: &SigningKey, s: &Shared, rx: &mut mpsc::UnboundedReceiver<Outbound>, ready: &mut bool) -> std::result::Result<(), Stop> {
    let id = uuid_v4().map_err(|_| Stop::Net)?;
    let (cid, rid) = (auth::uuid_bytes(&id).ok_or(Stop::Net)?, auth::uuid_bytes(&pin.room).ok_or(Stop::Net)?);
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(65536)).max_frame_size(Some(65536));
    let url = format!("{}/v1/ws/{}", signal_url(&pin.server), pin.room);
    let Ok(Ok((mut ws, _))) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async_with_config(url, Some(config), false)).await else { return Err(Stop::Net) };
    ws.send(json!({"v":1,"type":"hello","connection_id":id,"request_id":"client","payload":{"role":"mobile"}}).to_string().into()).await.map_err(|_| Stop::Net)?;
    // Espera o host chegar à sala (30 s, prazo do Worker para socket sem par).
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let tokio_tungstenite::tungstenite::Message::Text(raw) = ws.next().await.ok_or(Stop::Net)?.map_err(|_| Stop::Net)? else { return Err(Stop::Net) };
            let m = envelope_any(&raw)?;
            match (m["type"].as_str(), m["payload"]["status"].as_str()) {
                (Some("hello"), Some("waiting")) => continue,
                (Some("hello"), Some("ready")) if m["connection_id"] == id.as_str() => return Ok(()),
                _ => return Err(Stop::Net),
            }
        }
    }).await.map_err(|_| Stop::Net)??;
    let (cert, local_fp) = certificate()?;
    let (etx, mut erx) = mpsc::channel(260);
    let handler = Arc::new(Handler { tx: etx, failed: tokio::sync::Notify::new() });
    // ponytail: `peer()` só escuta UDP em 127.0.0.1 (sinalização e host locais, deploy vetado); abrir para a LAN junto com o deploy.
    let pc = peer(cert).with_handler(handler.clone()).build().await?;
    let r = async {
        let dc = pc.create_data_channel("pty-channel", None).await?;
        let fc = pc.create_data_channel("file-channel", None).await?;
        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;
        ws.send(message("offer", json!({"sdp":offer.sdp}), &id).into()).await?;
        let (mut sent, mut open, mut fopen) = (0, false, false);
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                tokio::select! {
                    f = ws.next() => {
                        let tokio_tungstenite::tungstenite::Message::Text(raw) = f.ok_or(Stop::Net)?.map_err(|_| Stop::Net)? else { return Err(Stop::Net) };
                        let m = envelope_any(&raw)?;
                        if m["connection_id"] != id.as_str() { return Err(Stop::Net); }
                        match m["type"].as_str() {
                            Some("answer") => pc.set_remote_description(RTCSessionDescription::answer(m["payload"]["sdp"].as_str().ok_or(Stop::Net)?.into())?).await?,
                            Some("ice") => pc.add_ice_candidate(serde_json::from_value(m["payload"].clone()).map_err(|_| Stop::Net)?).await?,
                            _ => return Err(Stop::Net),
                        }
                    }
                    e = erx.recv() => match e {
                        Some(Event::Ice(e)) => {
                            sent += 1;
                            if sent > 256 { return Err(Stop::Net); }
                            ws.send(message("ice", serde_json::to_value(e.candidate.to_json()?).map_err(|_| Stop::Net)?, &id).into()).await.map_err(|_| Stop::Net)?;
                        }
                        Some(Event::Failed) | None => return Err(Stop::Net),
                        _ => {}
                    },
                    e = dc.poll(), if !open => match e {
                        Some(DataChannelEvent::OnOpen) => open = true,
                        Some(DataChannelEvent::OnClose) | None => return Err(Stop::Net),
                        _ => {}
                    },
                    e = fc.poll(), if !fopen => match e {
                        Some(DataChannelEvent::OnOpen) => fopen = true,
                        Some(DataChannelEvent::OnClose) | None => return Err(Stop::Net),
                        _ => {}
                    },
                }
                if open && fopen { return Ok(()) }
            }
        }).await.map_err(|_| Stop::Net)??;
        // C2: binding com os fingerprints reais do DTLS; o pin do host decide (pair ou reconnect).
        let b = Binding { connection_id: cid, room_id: rid, fp_offerer: local_fp, fp_answerer: remote_fingerprint(&pc).await? };
        let (mut o, hello) = auth::Offerer::new(otk, sk.clone(), pin.host_pk, b, Instant::now()).map_err(|e| Stop::Auth(e.code().into()))?;
        dc.send_text(&hello.to_json(&id)).await?;
        tokio::time::timeout(auth::AUTH_TIMEOUT, async {
            loop {
                let m = match dc.poll().await { Some(DataChannelEvent::OnMessage(m)) => m, Some(DataChannelEvent::OnClose) | None => return Err(Stop::Net), _ => continue };
                let t = std::str::from_utf8(&m.data).map_err(|_| Stop::Net)?;
                let msg = auth::parse_msg(t, &id).map_err(|e| Stop::Auth(e.code().into()))?;
                if let Msg::Fail(e) = msg { return Err(Stop::Auth(e.code().into())); }
                match o.handle(msg, Instant::now()).map_err(|e| Stop::Auth(e.code().into()))? {
                    Some(p) => dc.send_text(&p.to_json(&id)).await?,
                    None => return Ok(()),
                }
            }
        }).await.map_err(|_| Stop::Auth("EXPIRED".into()))??;
        *ready = true;
        (s.on)(ClientEvent::Ready);
        // Sessões anexadas nesta conexão: frame de outra sessão (ex.: digitado durante a reconexão,
        // antes do reattach) seria erro fatal no host (C3 §2.2), então é descartado.
        // ponytail: detach/close não tiram a sessão do conjunto; o host responde com erro não fatal.
        let mut attached = std::collections::HashSet::new();
        let (utx, urx) = mpsc::unbounded_channel();
        let pump = tokio::spawn(file_pump(fc.clone(), id.clone(), urx));
        // O Worker fecha o WS do cliente aos 300 s (room.ts); depois disso a sessão vive só pelo WebRTC.
        let mut ws_open = true;
        let r: std::result::Result<(), Stop> = async { loop {
            tokio::select! {
                o = rx.recv() => match o {
                    None => return Ok(()),
                    Some(Outbound::Ctl(mut v)) => { v["v"] = 1.into(); v["connection_id"] = id.as_str().into(); dc.send_text(&v.to_string()).await?; }
                    Some(Outbound::Upload(u)) => { let _ = utx.send(u); }
                    Some(Outbound::Frame(mut f)) => if attached.contains(&f.session_id) {
                        if matches!(f.op, Op::Input(_)) { f.seq = s.next_seq(f.session_id); }
                        dc.send(proto::encode_frame(&f)[..].into()).await?;
                    },
                },
                e = dc.poll() => match e {
                    Some(DataChannelEvent::OnMessage(m)) if m.is_string => {
                        let v: Value = serde_json::from_slice(&m.data).map_err(|_| Stop::Net)?;
                        // `error` sem `request_id` é fatal (C3 §2.2): o host fecha em seguida.
                        if v["type"] == "error" && v.get("request_id").is_none() { return Err(Stop::Auth(v["code"].as_str().unwrap_or("INVALID_MESSAGE").into())); }
                        if v["type"] == "session_attach_ok" && let Some(sid) = v["session_id"].as_str().and_then(auth::uuid_bytes) && attached.insert(sid) { s.attached(&v, true); }
                        s.control(v);
                    }
                    Some(DataChannelEvent::OnMessage(m)) => {
                        let f = proto::parse_frame(&m.data).map_err(|_| Stop::Net)?;
                        // Ping do host: devolve o mesmo frame (C3 §3); é o que mantém a conexão viva.
                        if f.op == Op::Heartbeat { dc.send(m.data[..].into()).await?; } else { s.frame(f); }
                    }
                    Some(DataChannelEvent::OnClose) | None => return Err(Stop::Net),
                    _ => {}
                },
                e = erx.recv() => if matches!(e, Some(Event::Failed) | None) { return Err(Stop::Net) },
                f = ws.next(), if ws_open => if !matches!(f, Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))) { ws_open = false },
            }
        } }.await;
        pump.abort();
        r
    }.await;
    let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    r
}
/// Uploads em fila, um por vez, no `file-channel` (C4 §4/§7): init, fragmentos com pausa no `bufferedAmount`, espera `file_complete`.
/// Upload que falha não derruba a conexão do terminal: o erro volta para quem pediu.
async fn file_pump(fc: Arc<dyn DataChannel>, id: String, mut rx: mpsc::UnboundedReceiver<Upload>) {
    while let Some(u) = rx.recv().await {
        let r = upload_one(&fc, &id, &u).await;
        let _ = u.done.send(r);
    }
}
async fn upload_one(fc: &Arc<dyn DataChannel>, id: &str, u: &Upload) -> std::result::Result<Option<String>, String> {
    let e = |_: webrtc::error::Error| "NOT_CONNECTED".to_string();
    let mut init = u.init.clone();
    let rid = uuid_v4()?;
    (init["v"], init["connection_id"], init["request_id"]) = (1.into(), id.into(), rid.as_str().into());
    fc.send_text(&init.to_string()).await.map_err(e)?;
    let fid = proto::uuid_str(&u.fid);
    // Lê controles deste arquivo; `file_ack` é ignorado. ponytail: 60 s sem resposta = falha; ajustar se o host demorar em disco lento.
    let next = || async {
        tokio::time::timeout(Duration::from_secs(60), async { loop {
            match fc.poll().await {
                Some(DataChannelEvent::OnMessage(m)) if m.is_string => {
                    let v: Value = serde_json::from_slice(&m.data).map_err(|_| "INVALID_MESSAGE".to_string())?;
                    if v["file_id"] != fid.as_str() && v["request_id"] != rid.as_str() { continue }
                    if v["type"] == "error" { return Err(v["code"].as_str().unwrap_or("INVALID_MESSAGE").to_owned()) }
                    if v["type"] != "file_ack" { return Ok(v) }
                }
                Some(DataChannelEvent::OnClose) | None => return Err("NOT_CONNECTED".into()),
                _ => {}
            }
        } }).await.map_err(|_| "EXPIRED".to_string())?
    };
    let ok = next().await?;
    let saved = |v: &Value| v["path"].as_str().map(str::to_owned);
    if ok["type"] == "file_complete" { return Ok(saved(&ok)) }
    for r in ok["missing"]["ranges"].as_array().ok_or("INVALID_MESSAGE")? {
        let (a, b) = (r[0].as_u64().ok_or("INVALID_MESSAGE")? as usize, r[1].as_u64().ok_or("INVALID_MESSAGE")? as usize);
        for i in a..b {
            let c = &u.data[i * files::CHUNK as usize..((i + 1) * files::CHUNK as usize).min(u.data.len())];
            for f in files::frags(u.fid, i as u32, c) {
                // §7: pausa enquanto o buffer passa de 128 KiB (mesmo limite do teste de vazão no loopback).
                while fc.outstanding_bytes().await.map_err(e)? > (128 << 10) { tokio::time::sleep(Duration::from_millis(2)).await; }
                fc.send(f[..].into()).await.map_err(e)?;
            }
        }
    }
    loop { let v = next().await?; if v["type"] == "file_complete" { return Ok(saved(&v)) } }
}
impl From<webrtc::error::Error> for Stop { fn from(_: webrtc::error::Error) -> Self { Stop::Net } }
impl From<tokio_tungstenite::tungstenite::Error> for Stop { fn from(_: tokio_tungstenite::tungstenite::Error) -> Self { Stop::Net } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    const ROOM: &str = "20212223-2425-4627-a829-2a2b2c2d2e2f";

    fn events() -> (impl Fn(ClientEvent) + Send + Sync + 'static, Receiver<ClientEvent>) {
        let (tx, rx) = smpsc::channel();
        let tx = Mutex::new(tx);
        (move |e| { let _ = tx.lock().unwrap().send(e); }, rx)
    }
    /// Lê eventos até a saída da sessão conter `marker` duas vezes (eco + resultado).
    fn wait_output(rx: &Receiver<ClientEvent>, sid: &str, marker: &str) {
        let mut out = Vec::new();
        while String::from_utf8_lossy(&out).matches(marker).count() < 2 {
            if let ClientEvent::Output { session_id, data, .. } = rx.recv_timeout(Duration::from_secs(10)).expect("saída") && session_id == sid { out.extend(data); }
        }
    }
    fn cmd_profile(id: &str) { hub().profiles.push((id.into(), Profile { label: "cmd".into(), command: "cmd.exe".into(), osc: false })); }
    fn host(name: &str) -> (Arc<Host>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("live-cmd-d5c-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (Host::with_store(SigningKey::from_bytes(&[0x61; 32]), dir.join("trust.json")).unwrap(), dir)
    }
    /// Sobe o Worker falso e o host real (uma tentativa) no loopback; devolve o `server` do pin.
    fn host_on_loopback(rt: &tokio::runtime::Runtime, h: &Arc<Host>) -> String {
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(relay(listener));
            let hh = h.clone();
            tokio::spawn(async move { let _ = run(&format!("ws://127.0.0.1:{port}"), ROOM, hh, &Link::default(), tokio::sync::watch::channel(false).1, &mut Vec::new(), true).await; });
            format!("127.0.0.1:{port}")
        })
    }

    #[test]
    fn pair_link_checks_server_allowlist() {
        let uri = format!("remoteterm://pair?v=1&session={ROOM}&otk={}&host_pk={}&server=127.0.0.1:8787", B64.encode([7u8; 32]), B64.encode([8u8; 32]));
        let (pin, otk) = parse_pair_link(&format!("  {uri}\n"), &["127.0.0.1:8787"]).unwrap();
        assert_eq!((pin.host_pk, pin.room.as_str(), pin.server.as_str(), otk), ([8; 32], ROOM, "127.0.0.1:8787", [7; 32]));
        assert_eq!(parse_pair_link(&uri, &["sig.example.com"]).unwrap_err(), "SERVER_NOT_ALLOWED");
        let wrapped = format!("https://github.com/yagoriccomi/live-cmd/releases/latest#{uri}");
        assert_eq!(parse_pair_link(&wrapped, &["127.0.0.1:8787"]).unwrap().1, [7; 32]);
        assert_eq!(parse_pair_link("remoteterm://pair?v=2", &[]).unwrap_err(), "VERSION_UNSUPPORTED");
        assert_eq!(parse_pair_link("lixo", &[]).unwrap_err(), "INVALID_MESSAGE");
        assert_eq!(signal_url("127.0.0.1:8787"), "ws://127.0.0.1:8787");
        assert_eq!(signal_url("sig.example.com"), "wss://sig.example.com");
    }

    /// Daemon (D-A): sessão criada pelo pipe continua viva depois que o cliente sai; outro cliente
    /// a encontra na lista, reanexa e digita. Também: segundo servidor no mesmo pipe é recusado,
    /// token errado não passa do handshake e o `shutdown` pelo pipe encerra o servidor.
    #[test]
    fn pipe_sessao_sobrevive_a_reconexao() {
        let _heavy = crate::tests::HEAVY.blocking_lock();
        cmd_profile("termd-ipc");
        crate::tests::fdir();
        let (h, dir) = host("ipc");
        let svc = Service::with_host(Config { data_dir: dir.clone(), server: "ws://127.0.0.1:1".into(), room: ROOM.into() }, h).unwrap();
        let ep = ipc::Endpoint { pipe: format!(r"\\.\pipe\live-cmd-termd-test-{}", std::process::id()), token_file: dir.join("termd").join("token") };
        let srv = ipc::Server::bind(&ep).unwrap();
        assert_eq!(ipc::Server::bind(&ep).err().as_deref(), Some("ALREADY_RUNNING"));
        let d = Arc::new(ipc::ServiceDaemon::new(Some(svc), || Err("IO_ERROR".into())));
        let dd: Arc<dyn ipc::Daemon> = d.clone();
        let t = std::thread::spawn(move || srv.run(dd, ipc::Events::default()));

        let (on, rx) = events();
        let c = Client::pipe(ep.clone(), on).unwrap();
        assert_eq!(rx.recv().unwrap(), ClientEvent::Ready);
        let sid = c.request(json!({"type":"session_create","profile_id":"termd-ipc","cols":80,"rows":24})).unwrap()["session_id"].as_str().unwrap().to_owned();
        c.request(json!({"type":"session_attach","session_id":sid,"from_offset":null})).unwrap();
        c.resize(&sid, 100, 30).unwrap();
        c.input(&sid, b"echo termd-um\r").unwrap();
        wait_output(&rx, &sid, "termd-um");
        drop(c);

        // App fechado e aberto de novo: outra conexão, mesma sessão, mesmo processo.
        let (on, rx) = events();
        let c = Client::pipe(ep.clone(), on).unwrap();
        assert_eq!(rx.recv().unwrap(), ClientEvent::Ready);
        let list = c.request(json!({"type":"session_list"})).unwrap();
        let s = list["sessions"].as_array().unwrap().iter().find(|s| s["session_id"] == sid.as_str()).expect("sessão viva no daemon").clone();
        assert_eq!((s["state"].as_str(), s["attached_count"].as_u64(), s["cols"].as_u64()), (Some("running"), Some(0), Some(100)));
        c.request(json!({"type":"session_attach","session_id":sid,"from_offset":null})).unwrap();
        c.input(&sid, b"echo termd-dois\r").unwrap();
        wait_output(&rx, &sid, "termd-dois");

        let sc = ipc::ServiceClient::new(ep.clone());
        assert!(sc.rpc("status", Value::Null).is_ok());
        assert!(sc.devices().is_empty());
        assert_eq!(sc.rpc("nada", Value::Null).unwrap_err(), "INVALID_MESSAGE");
        let good = std::fs::read_to_string(&ep.token_file).unwrap();
        assert_eq!(good.len(), 64);
        std::fs::write(&ep.token_file, "0".repeat(64)).unwrap();
        assert_eq!(ipc::probe(&ep).unwrap_err(), ipc::Fail::Denied);
        std::fs::write(&ep.token_file, &good).unwrap();
        assert_eq!(ipc::probe(&ep).unwrap().ipc_version, ipc::IPC_VERSION);

        c.request(json!({"type":"session_close","session_id":sid})).unwrap();
        sc.shutdown().unwrap();
        t.join().unwrap();
        d.stop();
        assert!(!ep.token_file.exists());
        assert_eq!(ipc::probe(&ep).unwrap_err(), ipc::Fail::Absent);
    }

    /// Cliente local: create, attach, input, saída, close; no hub do processo, sem rede.
    #[test]
    fn local_create_attach_input_close() {
        let _heavy = crate::tests::HEAVY.blocking_lock();
        cmd_profile("d5c-local");
        crate::tests::fdir();
        let (h, dir) = host("local");
        let svc = Service::with_host(Config { data_dir: dir.clone(), server: "ws://127.0.0.1:1".into(), room: ROOM.into() }, h).unwrap();
        let (on, rx) = events();
        let c = Client::local(&svc, on);
        assert_eq!(rx.recv().unwrap(), ClientEvent::Ready);
        let v = c.request(json!({"type":"session_create","profile_id":"d5c-local","cols":80,"rows":24})).unwrap();
        let sid = v["session_id"].as_str().unwrap().to_owned();
        assert!(c.request(json!({"type":"session_list"})).unwrap()["sessions"].to_string().contains(&sid));
        c.request(json!({"type":"session_attach","session_id":sid,"from_offset":null})).unwrap();
        c.resize(&sid, 100, 30).unwrap();
        c.input(&sid, b"echo d5c-local-ok\r").unwrap();
        wait_output(&rx, &sid, "d5c-local-ok");
        // Segundo input usa seq 2: o dedup do host não descarta.
        c.input(&sid, b"echo d5c-second\r").unwrap();
        wait_output(&rx, &sid, "d5c-second");
        assert_eq!(c.request(json!({"type":"session_attach","session_id":"00000000-0000-4000-8000-000000000000","from_offset":null})).unwrap_err(), "NOT_FOUND");
        c.request(json!({"type":"session_close","session_id":sid})).unwrap();
        loop { if let ClientEvent::Exited { session_id, .. } = rx.recv_timeout(Duration::from_secs(10)).unwrap() && session_id == sid { break; } }
        drop(c);
        svc.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Worker falso de presença: host entra, cliente entra com o seu `connection_id`, os dois recebem `ready`;
    /// depois repassa as mensagens.
    async fn relay(listener: tokio::net::TcpListener) {
        // Host e cliente conectam em paralelo: a ordem do accept não é garantida, então separa pelo `role` do hello.
        let mut conns = vec![];
        for _ in 0..2 {
            let mut ws = tokio_tungstenite::accept_async(listener.accept().await.unwrap().0).await.unwrap();
            let hello = envelope_any(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            conns.push((ws, hello));
        }
        if conns[0].1["payload"]["role"] != "host" { conns.swap(0, 1); }
        let ((mut h, host_hello), (mut c, hello)) = (conns.remove(0), conns.remove(0));
        assert_eq!(host_hello["payload"]["role"], "host");
        h.send(json!({"v":1,"type":"hello","connection_id":null,"request_id":"w","payload":{"status":"waiting"}}).to_string().into()).await.unwrap();
        assert_eq!(hello["payload"]["role"], "mobile");
        let id = hello["connection_id"].as_str().unwrap().to_owned();
        assert!(auth::uuid_v4_lower(&id).is_some(), "connection_id v4 sorteado pelo cliente");
        for ws in [&mut h, &mut c] { ws.send(message("hello", json!({"status":"ready"}), &id).into()).await.unwrap(); }
        loop {
            tokio::select! {
                m = h.next() => match m { Some(Ok(m)) if m.is_text() => { if c.send(m).await.is_err() { return } } _ => return },
                m = c.next() => match m { Some(Ok(m)) if m.is_text() => { if h.send(m).await.is_err() { return } } _ => return },
            }
        }
    }

    /// Cliente remoto (offerer) contra o host real no loopback: pareia por OTK, abre sessão, digita, lê a saída.
    #[test]
    fn remote_pair_create_input() {
        let _heavy = crate::tests::HEAVY.blocking_lock();
        cmd_profile("d5c-remote");
        let (h, dir) = host("remote");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = host_on_loopback(&rt, &h);
        let otk = h.issue_otk(Instant::now()).unwrap();
        let (on, rx) = events();
        let sk = SigningKey::from_bytes(&[33; 32]);
        let c = Client::remote_with(sk.clone(), RemotePin { host_pk: h.pk(), room: ROOM.into(), server }, Some(otk), on).unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), ClientEvent::Connecting { attempt: 0 });
        assert_eq!(rx.recv_timeout(Duration::from_secs(20)).unwrap(), ClientEvent::Ready);
        assert!(h.clients().iter().any(|d| d.0 == sk.verifying_key().to_bytes()), "pareado na trust store");
        let sid = c.request(json!({"type":"session_create","profile_id":"d5c-remote","cols":80,"rows":24})).unwrap()["session_id"].as_str().unwrap().to_owned();
        // Regressão: digitar/redimensionar antes do attach (como na reconexão) não derruba a conexão.
        c.input(&sid, b"x").unwrap();
        c.resize(&sid, 80, 24).unwrap();
        c.request(json!({"type":"session_attach","session_id":sid,"from_offset":null})).unwrap();
        c.input(&sid, b"echo d5c-remote-ok\r").unwrap();
        wait_output(&rx, &sid, "d5c-remote-ok");
        c.request(json!({"type":"session_close","session_id":sid})).unwrap();
        drop(c);
        drop(rt);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Upload pelo `file-channel` do cliente remoto: arquivo de 3 chunks chega inteiro no `target_dir`.
    #[test]
    fn remote_upload_file() {
        let _heavy = crate::tests::HEAVY.blocking_lock();
        crate::tests::fdir();
        let (h, dir) = host("upload");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = host_on_loopback(&rt, &h);
        let otk = h.issue_otk(Instant::now()).unwrap();
        let (on, rx) = events();
        let c = Client::remote_with(SigningKey::from_bytes(&[34; 32]), RemotePin { host_pk: h.pk(), room: ROOM.into(), server }, Some(otk), on).unwrap();
        while rx.recv_timeout(Duration::from_secs(20)).unwrap() != ClientEvent::Ready {}
        let (src, dst) = (dir.join("src.bin"), dir.join("dst"));
        std::fs::create_dir_all(&dst).unwrap();
        let data: Vec<u8> = (0..150_000u32).map(|i| (i * 7) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        c.upload(&src, dst.to_str(), None, None).unwrap();
        assert!(std::fs::read(dst.join("src.bin")).unwrap() == data);
        assert_eq!(c.upload(&src, dst.to_str(), None, None).unwrap_err(), "ALREADY_EXISTS");
        drop(c);
        drop(rt);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// OTK errada: o host recusa e o cliente para sem nova tentativa, com o código C2.
    #[test]
    fn remote_bad_otk_stops_with_code() {
        let _heavy = crate::tests::HEAVY.blocking_lock();
        let (h, dir) = host("bad");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = host_on_loopback(&rt, &h);
        h.issue_otk(Instant::now()).unwrap();
        let (on, rx) = events();
        let _c = Client::remote_with(SigningKey::from_bytes(&[34; 32]), RemotePin { host_pk: h.pk(), room: ROOM.into(), server }, Some([0; 32]), on).unwrap();
        let last = loop { if let ClientEvent::Closed { code } = rx.recv_timeout(Duration::from_secs(20)).unwrap() { break code } };
        assert_eq!(last.as_deref(), Some("UNAUTHORIZED"));
        drop(rt);
        let _ = std::fs::remove_dir_all(dir);
    }
}

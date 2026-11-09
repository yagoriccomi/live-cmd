mod auth;
mod proto;
use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};
use auth::{Binding, Host, HostHandshake, Msg, State};
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
fn uuid(s: &str) -> bool { s.len() == 36 && s.bytes().enumerate().all(|(i,b)| if [8,13,18,23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }) }
fn envelope(text: &str, id: &str) -> Result<Value> {
    if text.len() > 65536 { return Err("signal limit".into()); }
    let m: Value = serde_json::from_str(text)?;
    let request = m["request_id"].as_str().ok_or("request")?;
    if m["v"] != 1 || m["connection_id"] != id || !m["payload"].is_object() || request.is_empty() || request.len()>128 || !request.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) { return Err("envelope".into()); }
    Ok(m)
}
fn message(kind: &str, payload: Value, id: &str) -> String { json!({"v":1,"type":kind,"connection_id":id,"request_id":"host","payload":payload}).to_string() }
fn text(data: &[u8], is_string: bool) -> Result<&str> {
    if !is_string || data.len()>16384 { return Err("message limit/type".into()); }
    Ok(std::str::from_utf8(data)?)
}
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
    PeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().with_certificates(vec![cert]).build())
        .with_udp_addrs(vec!["127.0.0.1:0"]).with_data_channel_send_buffer_limit(32768)
}
async fn run(room: &str, id: &str, port: u16, host: Arc<Host>) -> Result<()> {
    let (cid, rid) = (auth::uuid_bytes(id).ok_or("connection UUID")?, auth::uuid_bytes(room).ok_or("room UUID")?);
    let (cert, local_fp) = certificate()?;
    let (tx, mut rx) = mpsc::channel(260);
    let handler = Arc::new(Handler { tx, failed: tokio::sync::Notify::new() });
    let pc = peer(cert).with_handler(handler.clone()).build().await?;
    let channel_open = tokio::sync::Notify::new();
    let attempt = async {
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(65536)).max_frame_size(Some(65536));
        let (mut ws, _) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async_with_config(format!("ws://127.0.0.1:{port}/v1/ws/{room}"), Some(config), false)).await??;
        ws.send(message("hello", json!({"role":"host"}), id).into()).await?;
        let deadline = tokio::time::sleep(Duration::from_secs(30)); tokio::pin!(deadline);
        let mut ready = false; let mut offered = false; let mut ended = false; let mut candidates = 0;
        let mut channel: Option<Arc<dyn DataChannel>> = None; let mut opened = false;
        let mut file: Option<Arc<dyn DataChannel>> = None;
        let mut hs: Option<HostHandshake> = None;
        let mut client_pk: Option<[u8; 32]> = None;
        // C2 §2.1: os 30 s contam do DTLS connected, com ou sem pty-channel aberto.
        let mut connected_at: Option<Instant> = None;
        let mut revocation = tokio::time::interval(Duration::from_secs(1));
        loop {
            // Erro de auth: envia o código ao peer e encerra; o chamador fecha a PeerConnection inteira.
            let auth_error: auth::Error = tokio::select! {
                _ = &mut deadline, if !opened => return Err("handshake timeout".into()),
                _ = tokio::signal::ctrl_c() => return Ok(()),
                _ = handler.failed.notified() => return Err("callback overflow".into()),
                event = rx.recv() => match event.ok_or("events closed")? {
                    Event::Failed => return Err("connection closed".into()),
                    Event::Connected => { connected_at.get_or_insert_with(Instant::now); continue }
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
                        tokio::time::timeout(Duration::from_secs(2), ws.send(wire.into())).await??;
                        continue;
                    }
                },
                _ = async { tokio::time::sleep_until((connected_at.unwrap() + auth::AUTH_TIMEOUT).into()).await }, if hs.is_none() && connected_at.is_some() => auth::Error::Expired,
                _ = async { tokio::time::sleep_until(hs.as_ref().unwrap().deadline().into()).await }, if hs.as_ref().is_some_and(|h| h.state() != State::Authenticated) => {
                    match hs.as_mut().unwrap().tick(Instant::now()) { Err(e) => e, Ok(()) => continue }
                }
                // ponytail: revogação vista ao reler o arquivo a cada 1 s; trocar por IPC se precisar de menos.
                _ = revocation.tick(), if client_pk.is_some() => {
                    match host.reload().map_err(|_| auth::Error::IoError).and_then(|_| host.check_trusted(&client_pk.unwrap())) { Err(e) => e, Ok(()) => continue }
                }
                // ponytail: C4 não implementado; qualquer mensagem no file-channel fecha (antes da auth: C2 N11).
                event = async { file.as_ref().unwrap().poll().await }, if file.is_some() => match event {
                    Some(DataChannelEvent::OnMessage(_)) => return Err("file-channel".into()),
                    Some(DataChannelEvent::OnClose) | None => { file = None; continue }
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
                            Some(h) if h.state() == State::Authenticated => { let t = text(&m.data, m.is_string)?; tokio::time::timeout(Duration::from_secs(2), ch.send_text(t)).await??; continue }
                            Some(h) => {
                                let parsed = if m.is_string { std::str::from_utf8(&m.data).map_err(|_| auth::Error::InvalidMessage).and_then(|t| auth::parse_msg(t, id)) } else { Err(auth::Error::InvalidMessage) };
                                match parsed.and_then(|msg| h.handle(msg, Instant::now())) {
                                    Ok(reply) => {
                                        tokio::time::timeout(Duration::from_secs(2), ch.send_text(&reply.to_json(id))).await??;
                                        client_pk = h.client_pk();
                                        continue;
                                    }
                                    Err(e) => e,
                                }
                            }
                            None => return Err("message before open".into()),
                        },
                        Some(DataChannelEvent::OnClose) | None => return Ok(()),
                        _ => continue,
                    }
                },
                frame = ws.next() => {
                    let frame = frame.ok_or("websocket closed")??;
                    let raw = match frame { tokio_tungstenite::tungstenite::Message::Text(t) => t, tokio_tungstenite::tungstenite::Message::Close(_) => return Ok(()), _ => return Err("websocket type".into()) };
                    let m = envelope(&raw, id)?; let p = &m["payload"];
                    match m["type"].as_str() {
                        Some("hello") if !ready && !offered => match p["status"].as_str() { Some("ready") => ready=true, Some("waiting") => {}, _ => return Err("hello".into()) },
                        Some("offer") if ready && !offered => {
                            let sdp = p["sdp"].as_str().filter(|s| !s.trim().is_empty()).ok_or("sdp")?;
                            pc.set_remote_description(RTCSessionDescription::offer(sdp.to_owned())?).await?;
                            let answer = pc.create_answer(None).await?;
                            // O certificado gerado precisa ser o anunciado (C2 §4).
                            if !answer.sdp.to_ascii_lowercase().contains(&sdp_fingerprint(&local_fp)) { return Err("local certificate not in SDP".into()); }
                            pc.set_local_description(answer.clone()).await?;
                            let wire = message("answer", json!({"sdp":answer.sdp}), id);
                            if wire.len() > 65536 { return Err("signal limit".into()); }
                            tokio::time::timeout(Duration::from_secs(2), ws.send(wire.into())).await??; offered=true;
                        }
                        Some("ice") if offered && !ended => {
                            let c = p["candidate"].as_str().ok_or("candidate")?;
                            if p.get("sdpMid").is_none() || p.get("usernameFragment").is_none() || p.get("sdpMLineIndex").is_none() || c.len()>4096 || !(p["sdpMid"].is_null() || p["sdpMid"].is_string()) || !(p["usernameFragment"].is_null() || p["usernameFragment"].is_string()) || !(p["sdpMLineIndex"].is_null() || p["sdpMLineIndex"].as_u64().is_some_and(|n| n<=65535)) { return Err("ice".into()); }
                            ended=c.is_empty(); if !ended { candidates+=1; if candidates>256 { return Err("ice limit".into()); } }
                            pc.add_ice_candidate(serde_json::from_value(p.clone())?).await?;
                        }
                        Some("close") if matches!(p["reason"].as_str(), Some("done" | "timeout" | "peer_left" | "protocol_error")) => return Ok(()),
                        Some("error") if p["code"] == "INVALID_MESSAGE" && p["message"].is_string() => return Err("signaling error".into()),
                        _ => return Err("signal phase".into())
                    }
                    continue;
                }
            };
            if let Some(ch) = &channel {
                // Drena o erro antes do close da PeerConnection; sem isso o peer pode ver só o OnClose.
                let _ = tokio::time::timeout(Duration::from_secs(2), async {
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
        result = tokio::time::timeout(Duration::from_secs(300), attempt) => result.unwrap_or_else(|_| Err("attempt timeout".into())),
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    let close_result = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    result?;
    close_result??;
    Ok(())
}
fn store_path() -> Result<PathBuf> {
    // ponytail: APPDATA (Windows) ou HOME; usar um crate de diretórios se precisar seguir XDG/macOS à risca.
    let dir = std::env::var_os("LIVE_CMD_DATA_DIR").map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("live-cmd")))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".live-cmd")))
        .ok_or("data dir")?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("trust.json"))
}
/// Chave Ed25519 do host no cofre do SO, criada na primeira execução (C2 §6).
// ponytail: só o backend Windows (Credential Manager) habilitado; ativar apple-native/linux-native ao portar.
fn host_key() -> Result<SigningKey> {
    let entry = keyring::Entry::new("live-cmd", "host-ed25519")?;
    let seed: [u8; 32] = match entry.get_secret() {
        Ok(s) => s.try_into().map_err(|_| "host key")?,
        Err(keyring::Error::NoEntry) => { let mut s = [0; 32]; getrandom::getrandom(&mut s).map_err(|_| "random")?; entry.set_secret(&s)?; s }
        Err(e) => return Err(e.into()),
    };
    Ok(SigningKey::from_bytes(&seed))
}
#[tokio::main]
async fn main() {
    let result: Result<()> = async {
        let mut room=None; let mut id=None; let mut port=8787; let mut pair=None; let mut revoke=None;
        let mut args=std::env::args().skip(1);
        while let Some(key)=args.next() { let value=args.next().ok_or("argument value")?; match key.as_str() { "--room-id" => room=Some(value), "--connection-id" => id=Some(value), "--port" => port=value.parse::<u16>()?, "--pair" => pair=Some(value), "--revoke" => revoke=Some(value), _ => return Err("argument".into()) } }
        let host = Host::with_store(host_key()?, store_path()?)?;
        if let Some(pk) = revoke {
            let pk: [u8; 32] = B64.decode(pk)?.try_into().map_err(|_| "client_pk")?;
            return host.revoke(pk).map_err(|e| e.code().into());
        }
        let room=room.filter(|s| uuid(s)).ok_or("room UUID")?; let id=id.filter(|s| uuid(s)).ok_or("connection UUID")?;
        if port==0 { return Err("port".into()); }
        if let Some(server) = pair {
            let otk = host.issue_otk(Instant::now()).map_err(|e| e.code())?;
            let qr = format!("remoteterm://pair?v=1&session={room}&otk={}&host_pk={}&server={server}", B64.encode(otk), B64.encode(host.pk()));
            auth::parse_qr(&qr).map_err(|e| e.code())?;
            // Só o desenho do QR vai ao terminal do usuário; o texto do URI nunca é impresso (C2 §1).
            println!("{}", qrcode::QrCode::new(qr.as_bytes())?.render::<qrcode::render::unicode::Dense1x2>().build());
        }
        run(&room,&id,port,host).await
    }.await;
    if result.is_err() { eprintln!("Echo attempt failed"); std::process::exit(1); }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        const MESSAGE_BYTES: usize = 16384;
        const MAX_MESSAGES: usize = 1024;
        const BUFFER_LIMIT: usize = 32768;
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
                    tokio::time::timeout(Duration::from_secs(5), &mut pending).await??;
                    let elapsed = resumed_at.elapsed();
                    if elapsed > Duration::from_secs(5) { return Err("retomada excedeu 5s".into()); }
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
                    tokio::time::timeout(Duration::from_secs(10), drain).await?
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
    // Run Wrangler with fresh ROOM_ID/CONNECTION_ID; pass the same values here.
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_echo() { wrangler_channel("reliable").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_retransmits_zero() { wrangler_channel("retransmits-zero").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_retransmits_one() { wrangler_channel("retransmits-one").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_lifetime_zero() { wrangler_channel("lifetime-zero").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_lifetime_one() { wrangler_channel("lifetime-one").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_unordered() { wrangler_channel("unordered").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_wrong_label() { wrangler_channel("wrong-label").await; }
    #[tokio::test]
    #[ignore = "requires local Wrangler and fresh ROOM_ID/CONNECTION_ID"]
    async fn wrangler_second_channel() { wrangler_channel("second-channel").await; }

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
        let id = std::env::var("CONNECTION_ID").unwrap();
        assert!(uuid(&room) && uuid(&id));
        let port = std::env::var("PORT").unwrap_or("8787".into()).parse::<u16>().unwrap();
        let r = room.clone(); let i = id.clone();
        let mut host = tokio::spawn(async move { run(&r, &i, port, Host::new(SigningKey::from_bytes(&[0x60; 32]))).await });
        let (tx, mut rx) = mpsc::channel(260);
        let pc = PeerConnectionBuilder::new()
            .with_handler(Arc::new(Handler { tx, failed: tokio::sync::Notify::new() }))
            .with_udp_addrs(vec!["127.0.0.1:0"])
            .with_data_channel_send_buffer_limit(32768).build().await.unwrap();
        let mut host_result = None;
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(30), async {
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/v1/ws/{room}")).await?;
            ws.send(message("hello", json!({"role":"mobile"}), &id).into()).await?;
            let dc = pc.create_data_channel(if case == "wrong-label" { "wrong-channel" } else { "pty-channel" }, Some(init)).await?;
            let samples = [String::new(), "ação 日本語".into(), "repeated".into(), "repeated".into(), "a".repeat(16384)];
            let mut received = 0;
            loop {
                tokio::select! {
                    outcome = &mut host => {
                        host_result = Some(outcome);
                        return Err("host exited".into());
                    }
                    frame = ws.next() => {
                        let frame = frame.ok_or("ws ended")??;
                        let m = envelope(frame.to_text()?, &id)?;
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
                            dc.send_text(&samples[0]).await?;
                        },
                        Some(DataChannelEvent::OnMessage(m)) => {
                            if received >= samples.len() || text(&m.data,m.is_string)? != samples[received] { return Err("echo mismatch/duplicate".into()); }
                            received += 1;
                            if reject {
                                if case != "second-channel" { return Err("host echoed forbidden channel".into()); }
                                pc.create_data_channel("pty-channel", None).await?;
                                // The first channel was accepted (echo above); now require host rejection.
                                host_result = Some(tokio::time::timeout(Duration::from_secs(5), &mut host).await?);
                                return Ok(());
                            }
                            if received == samples.len() {
                                if tokio::time::timeout(Duration::from_millis(250), async {
                                    loop { if matches!(dc.poll().await, Some(DataChannelEvent::OnMessage(_))) { break; } }
                                }).await.is_ok() { return Err("duplicate echo".into()); }
                                ws.close(None).await?;
                                host_result = Some(tokio::time::timeout(Duration::from_secs(5), &mut host).await?);
                                return Ok(());
                            }
                            dc.send_text(&samples[received]).await?;
                        }
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
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let id = format!("00000000-0000-4000-8000-0000000000{n:02x}");
        let (r, i) = (ROOM.to_owned(), id.clone());
        let task = tokio::spawn(async move { run(&r, &i, port, host).await });
        let (stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let mut ws = tokio_tungstenite::accept_async(stream).await?;
        let first = ws.next().await.ok_or("ws")??;
        if envelope(first.to_text()?, &id)?["type"] != "hello" { return Err("host hello".into()); }
        ws.send(message("hello", json!({"status":"ready"}), &id).into()).await?;
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
        let (mut o, hello) = auth::Offerer::new(otk, sk, pin, b, Instant::now()).map_err(|e| e.code().to_owned())?;
        l.dc.send_text(&hello.to_json(&l.id)).await.map_err(|e| e.to_string())?;
        loop {
            let m = auth::parse_msg(&next_text(&l.dc).await?, &l.id).map_err(|e| e.code().to_owned())?;
            if let Msg::Fail(e) = m { return Err(e.code().to_owned()); }
            match o.handle(m, Instant::now()).map_err(|e| e.code().to_owned())? {
                Some(p) => l.dc.send_text(&p.to_json(&l.id)).await.map_err(|e| e.to_string())?,
                None => return Ok(()),
            }
        }
    }
    async fn host_error(h: tokio::task::JoinHandle<Result<()>>, secs: u64) -> String {
        match tokio::time::timeout(Duration::from_secs(secs), h).await {
            Ok(Ok(Err(e))) => e.to_string(),
            other => format!("{other:?}"),
        }
    }
    async fn echo(dc: &Arc<dyn DataChannel>) -> R2<()> {
        dc.send_text("ping ação").await.map_err(|e| e.to_string())?;
        if next_text(dc).await? == "ping ação" { Ok(()) } else { Err("echo".into()) }
    }
    async fn finish(pc: impl PeerConnection, l: Loopback) {
        l.keep.abort();
        if !l.host.is_finished() { l.host.abort(); }
        let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
    }
    #[tokio::test]
    async fn d2b_pair_reconnect_revoke_during_session() {
        let (host, path, _tmp) = store("revoke");
        let c_sk = SigningKey::from_bytes(&[7; 32]);
        let c_pk = c_sk.verifying_key().to_bytes();
        let otk = host.issue_otk(Instant::now()).unwrap();
        let (pc, l) = loopback(host.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), Some(otk), c_sk.clone(), host.pk()).await.unwrap();
        echo(&l.dc).await.unwrap();
        finish(pc, l).await;
        assert!(std::fs::read_to_string(&path).unwrap().contains(&B64.encode(c_pk)), "pair persistido");
        // Reconexão: nova instância do host lê a store do disco.
        let host2 = Host::with_store(SigningKey::from_bytes(&[0x60; 32]), path.clone()).unwrap();
        let (pc, l) = loopback(host2.clone(), false).await.unwrap();
        authenticate(&l, l.b.clone(), None, c_sk.clone(), host2.pk()).await.unwrap();
        echo(&l.dc).await.unwrap();
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
        for case in ["binary", "control", "file"] {
            let (host, _, _tmp) = store(case);
            let (pc, l) = loopback(host, case == "file").await.unwrap();
            match case {
                "binary" => l.dc.send(b"\x01\x02"[..].into()).await.unwrap(),
                "control" => l.dc.send_text(&json!({"v":1,"type":"session_list","connection_id":l.id}).to_string()).await.unwrap(),
                _ => l.fc.as_ref().unwrap().send_text("x").await.unwrap(),
            }
            let Loopback { host: h, keep, .. } = l;
            let want = if case == "file" { "file-channel" } else { "INVALID_MESSAGE" };
            assert_eq!(host_error(h, 5).await, want, "{case}");
            keep.abort();
            let _ = tokio::time::timeout(Duration::from_secs(2), pc.close()).await;
        }
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
        for src in [include_str!("main.rs"), include_str!("auth.rs")] {
            assert!(!src.contains(bypass));
            let prod = src.split("#[cfg(test)]").next().unwrap();
            // Allowlist exata: qualquer print/log novo em produção exige revisão (C2 §1). O QR só vai ao terminal no --pair.
            let allowed = [
                r#"println!("{}", qrcode::QrCode::new(qr.as_bytes())?.render::<qrcode::render::unicode::Dense1x2>().build());"#,
                r#"if result.is_err() { eprintln!("Echo attempt failed"); std::process::exit(1); }"#,
            ];
            for line in prod.lines().filter(|l| ["print!", "println!", "eprint", "log::", "tracing::", "dbg!", "write!(", "writeln!(", "stdout", "stderr"].iter().any(|m| l.contains(m))) {
                assert!(allowed.contains(&line.trim()), "print/log não revisado: {line}");
            }
        }
    }
}

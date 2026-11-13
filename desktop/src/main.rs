//! CLI do host: `[--room-id <uuid>] [--port <n>] [--pair <server>] | --revoke <client_pk>`. Roda até Ctrl+C.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use live_cmd_echo::{Config, Service, default_data_dir, revoke_offline, room_id};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn main() {
    let result: Result<()> = (|| {
        let mut room=None; let mut port=8787; let mut pair=None; let mut revoke=None;
        let mut args=std::env::args().skip(1);
        while let Some(key)=args.next() { let value=args.next().ok_or("argument value")?; match key.as_str() { "--room-id" => room=Some(value), "--port" => port=value.parse::<u16>()?, "--pair" => pair=Some(value), "--revoke" => revoke=Some(value), _ => return Err("argument".into()) } }
        let data_dir = default_data_dir()?;
        if let Some(pk) = revoke { return Ok(revoke_offline(&data_dir, B64.decode(pk)?.try_into().map_err(|_| "client_pk")?)?); }
        if port==0 { return Err("port".into()); }
        let room = match room { Some(r) => r, None => room_id(&data_dir)? };
        let service = Service::start(Config { data_dir, server: format!("ws://127.0.0.1:{port}"), room })?;
        if let Some(server) = pair {
            let qr = service.pair_with(&server)?;
            // Só o desenho do QR vai ao terminal do usuário; o texto do URI nunca é impresso (C2 §1).
            println!("{}", qrcode::QrCode::new(qr.as_bytes())?.render::<qrcode::render::unicode::Dense1x2>().build());
        }
        tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(tokio::signal::ctrl_c())?;
        service.stop();
        Ok(())
    })();
    if result.is_err() { eprintln!("Echo attempt failed"); std::process::exit(1); }
}

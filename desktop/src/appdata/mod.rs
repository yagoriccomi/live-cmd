//! Arquivos de dados do app desktop (pasta `%APPDATA%\live-cmd`) que o app e o daemon `live-cmd-termd`
//! leem e gravam: configurações, registro de agentes, histórico e o provedor das telas do celular.
//! Vieram de `desktop/app/src`: o daemon atende o celular e grava o histórico sem o app aberto.
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

pub mod agents;
pub mod hist;
mod provider;
pub mod settings;

pub use provider::{AppProvider, Notify, Probe};

/// Conexão das sessões deste PC (aba "Este PC" do terminal desktop e registro de agentes).
pub const LOCAL: &str = "local";

pub fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64) }

/// Abre `porta` no host em até `timeout`; `{ok, ms}` ou `{ok:false, code}` (`DNS`, `TIMEOUT`, `OFFLINE`).
pub fn probe(host: &str, port: u16, timeout: Duration) -> Value {
    if !crate::ssh::valid_host(host) || port == 0 { return json!({ "ok": false, "code": "INVALID_SSH_HOST" }); }
    let Ok(addrs) = (host, port).to_socket_addrs() else { return json!({ "ok": false, "code": "DNS" }) };
    let mut code = "DNS";
    for a in addrs {
        let t = Instant::now();
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(_) => return json!({ "ok": true, "ms": t.elapsed().as_millis() as u64 }),
            Err(e) => code = if e.kind() == std::io::ErrorKind::TimedOut { "TIMEOUT" } else { "OFFLINE" },
        }
    }
    json!({ "ok": false, "code": code })
}

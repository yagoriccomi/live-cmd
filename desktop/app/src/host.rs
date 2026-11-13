//! Adaptador do app para o host. A UI só conhece este arquivo.
//!
//! ponytail: stub até D5a; trocar por live_cmd_echo::Service. A troca deve mexer só aqui:
//! manter os nomes e assinaturas públicas abaixo e delegar para a biblioteca do host.
//! Limite do stub: nada de rede; pareamento nunca conclui; dois aparelhos fictícios na lista.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;

/// Validade da OTK (C2 §1).
const PAIR_TTL_MS: u64 = 300_000;

pub struct Config { pub data_dir: PathBuf, pub server: String, pub room: String }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { Running, Connecting, Error }

/// `pairing_expires_ms`: instante (ms desde a época Unix) em que a OTK ativa expira; `None` sem OTK ativa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status { pub state: State, pub connected: usize, pub pairing_expires_ms: Option<u64> }

#[derive(Clone, Debug)]
pub struct Device { pub client_pk: [u8; 32], pub label: String, pub paired_at_ms: u64, pub connected: bool }

pub struct Service { cfg: Config, inner: Mutex<Inner> }
struct Inner { devices: Vec<Device>, pairing: Option<u64> }

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    getrandom::getrandom(&mut b).expect("random");
    b
}

impl Service {
    pub fn start(cfg: Config) -> Result<Service, String> {
        let day = 86_400_000;
        let devices = vec![
            Device { client_pk: [1; 32], label: "Pixel de Yago".into(), paired_at_ms: now_ms() - 5 * day, connected: true },
            Device { client_pk: [2; 32], label: "iPhone da Ana".into(), paired_at_ms: now_ms() - 4 * day, connected: false },
        ];
        Ok(Service { cfg, inner: Mutex::new(Inner { devices, pairing: None }) })
    }

    pub fn status(&self) -> Status {
        let mut i = self.inner.lock().unwrap();
        if i.pairing.is_some_and(|t| t <= now_ms()) { i.pairing = None; }
        Status { state: State::Running, connected: i.devices.iter().filter(|d| d.connected).count(), pairing_expires_ms: i.pairing }
    }

    /// Gera uma OTK nova (invalida a anterior) e devolve o URI só para desenhar o QR.
    pub fn pair(&self) -> Result<String, String> {
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        self.inner.lock().unwrap().pairing = Some(now_ms() + PAIR_TTL_MS);
        let _ = &self.cfg.data_dir;
        Ok(format!("remoteterm://pair?v=1&session={}&otk={}&host_pk={}&server={}",
            self.cfg.room, b64.encode(random::<32>()), b64.encode([0u8; 32]), self.cfg.server))
    }

    pub fn cancel_pair(&self) { self.inner.lock().unwrap().pairing = None; }

    pub fn devices(&self) -> Vec<Device> { self.inner.lock().unwrap().devices.clone() }

    pub fn revoke(&self, client_pk: [u8; 32]) -> Result<(), String> {
        let mut i = self.inner.lock().unwrap();
        let n = i.devices.len();
        i.devices.retain(|d| d.client_pk != client_pk);
        if i.devices.len() == n { Err("NOT_FOUND".into()) } else { Ok(()) }
    }

    pub fn stop(&self) { self.cancel_pair(); }
}

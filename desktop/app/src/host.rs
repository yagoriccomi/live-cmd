//! Adaptador do app para o host. A UI só conhece este arquivo; o host é a biblioteca `live_cmd_echo` (D5a).
use std::time::{SystemTime, UNIX_EPOCH};

pub use live_cmd_echo::{Client, ClientEvent, RemotePin, State, parse_pair_link};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

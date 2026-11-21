//! `live-cmd-termd`: daemon de terminais do live-cmd. Dono das sessões PTY e do host do celular;
//! segue rodando quando o app fecha ou atualiza. O app fala com ele pelo named pipe (`live_cmd_echo::ipc`).
//!
//! `live-cmd-termd` sobe o daemon (sai calado se outro já tem o pipe deste usuário).
//! `live-cmd-termd --shutdown` encerra o daemon em execução (usado pelo desinstalador).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;

use live_cmd_echo::appdata::{AppProvider, settings};
use live_cmd_echo::{Config, Service, extras, hlog, ipc, room_id};

fn main() {
    let ep = match ipc::Endpoint::user() {
        Ok(e) => e,
        Err(e) => { hlog(&format!("termd: sem endpoint ({e})")); std::process::exit(1) }
    };
    if std::env::args().any(|a| a == "--shutdown") {
        let r = ipc::ServiceClient::new(ep).shutdown();
        std::process::exit(match r { Ok(()) => 0, Err(e) if e == "NOT_RUNNING" => 0, Err(_) => 1 });
    }
    let dir = match settings::data_dir() {
        Ok(d) => d,
        Err(e) => { hlog(&format!("termd: sem pasta de dados ({e})")); std::process::exit(1) }
    };
    let srv = match ipc::Server::bind(&ep) {
        Ok(s) => s,
        Err(e) if e == "ALREADY_RUNNING" => return,
        Err(e) => { hlog(&format!("termd: pipe não abriu ({e})")); std::process::exit(1) }
    };
    // Programas abertos pelo terminal herdam a pasta atual: a do usuário, não a do instalador.
    if let Some(home) = std::env::var_os("USERPROFILE") { let _ = std::env::set_current_dir(home); }
    hlog(&format!("termd: iniciado (build {}, ipc {})", ipc::BUILD, ipc::IPC_VERSION));

    // Avisos do provedor do celular (aparência mudou, agente esperando) vão para o app, se estiver aberto.
    let events = ipc::Events::default();
    let ev = events.clone();
    extras::set_provider(Arc::new(AppProvider::new(dir.clone()).with_notify(Arc::new(move |e: &str, v| ev.send(e, v)))));

    let start = move || -> Result<Service, String> {
        let s = settings::load(&dir)?;
        Service::start(Config { data_dir: dir.clone(), server: s.server, room: room_id(&dir)? })
    };
    let svc = start().map_err(|e| hlog(&format!("termd: host não subiu ({e})"))).ok();
    let d = Arc::new(ipc::ServiceDaemon::new(svc, start));
    srv.run(d.clone(), events);
    d.stop();
    hlog("termd: encerrado");
}

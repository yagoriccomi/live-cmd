//! CPU e memória do PC do host, para a barra "Desempenho" (local ou remoto via `sys_stats`). Só Win32:
//! a CPU sai da diferença entre duas leituras de GetSystemTimes (a anterior fica guardada aqui).
use std::sync::Mutex;

use serde_json::{Value, json};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

static LAST: Mutex<Option<(u64, u64)>> = Mutex::new(None); // (ocioso, total) da leitura anterior

const fn ft(f: FILETIME) -> u64 { ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64 }

/// Uso de CPU (%) desde a chamada anterior; a primeira chamada mede desde o boot.
fn cpu() -> Option<f64> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    // kernel já inclui o tempo ocioso.
    let (idle, total) = (ft(idle), ft(kernel) + ft(user));
    let mut last = LAST.lock().unwrap();
    let (i0, t0) = last.replace((idle, total)).unwrap_or((0, 0));
    let (di, dt) = (idle.saturating_sub(i0), total.saturating_sub(t0));
    Some(if dt == 0 { 0.0 } else { 100.0 * (dt - di.min(dt)) as f64 / dt as f64 })
}

/// `{cpu, cores, mem_used, mem_total, host}`; bytes em mem_*. Campos ausentes viram null.
pub fn sys_stats() -> Value {
    let mut m = MEMORYSTATUSEX { dwLength: size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    let mem = unsafe { GlobalMemoryStatusEx(&mut m) }.ok().map(|_| (m.ullTotalPhys - m.ullAvailPhys, m.ullTotalPhys));
    json!({
        "cpu": cpu(),
        "cores": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "mem_used": mem.map(|x| x.0), "mem_total": mem.map(|x| x.1),
        "host": std::env::var("COMPUTERNAME").ok(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn stats_tem_campos() {
        let v = super::sys_stats();
        let cpu = super::sys_stats()["cpu"].as_f64().unwrap();
        assert!((0.0..=100.0).contains(&cpu));
        assert!(v["mem_total"].as_u64().unwrap() >= v["mem_used"].as_u64().unwrap());
    }
}

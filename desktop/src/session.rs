//! C3 §4–§7: tabela de sessões PTY, dedup de input, ring buffer, replay, TTL, limites e revogação.
//! Lógica pura: PTY injetável (`Pty`) e relógio injetável (`now` em ms em cada chamada).
#![allow(dead_code)]
use crate::auth::Error::{self, *};
use crate::proto::{u64s, uuid_str, Frame, Op, MAX_IO};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

type R<T> = Result<T, Error>;
pub type Sid = [u8; 16];
pub type Pk = [u8; 32];
/// Identifica uma PeerConnection autenticada (atribuído por quem chama).
pub type ConnId = u64;

pub const MAX_SESSIONS: usize = 16;
pub const MAX_ATTACH: usize = 4;
pub const RING_BYTES: usize = 8 << 20;
pub const RING_LINES: usize = 10_000;
pub const QUEUE: usize = 1 << 20;
pub const TTL_MS: u64 = 24 * 3600 * 1000;

pub trait Pty {
    fn write(&mut self, data: &[u8]) -> std::io::Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> std::io::Result<()>;
    fn kill(&mut self) -> std::io::Result<()>;
}

/// O que enviar a uma conexão, na ordem.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    /// Corpo do controle (sem `v`/`connection_id`); u64 já como string.
    Ctl(Value),
    Frame(Frame),
    /// Ponto exato onde vai o `screen_snapshot` (D3d), com `offset` = `available_from`.
    Snapshot { session_id: Sid, offset: u64 },
}

/// Saída retida: até `RING_BYTES` bytes e `RING_LINES` caracteres `\n`, o que vier primeiro.
struct Ring { buf: VecDeque<u8>, start: u64, nl: VecDeque<u64> }
impl Ring {
    fn end(&self) -> u64 { self.start + self.buf.len() as u64 }
    /// Devolve os bytes que saíram do início (alimentam a tela-base do snapshot).
    fn push(&mut self, data: &[u8]) -> Vec<u8> {
        let end = self.end();
        self.nl.extend(data.iter().enumerate().filter(|(_, b)| **b == b'\n').map(|(i, _)| end + i as u64));
        self.buf.extend(data);
        let mut cut = self.start;
        if self.buf.len() > RING_BYTES { cut = self.end() - RING_BYTES as u64; }
        if self.nl.len() > RING_LINES { cut = cut.max(self.nl[self.nl.len() - RING_LINES - 1] + 1); }
        let gone = self.buf.drain(..(cut - self.start) as usize).collect();
        self.start = cut;
        while self.nl.front().is_some_and(|o| *o < cut) { self.nl.pop_front(); }
        gone
    }
    fn read(&self, from: u64, n: usize) -> Vec<u8> {
        let a = (from - self.start) as usize;
        self.buf.range(a..a + n).copied().collect()
    }
}

struct Attach { pk: Pk, next: u64, queued: usize, replay_to: Option<u64>, exit_sent: bool, size: Option<(u16, u16)> }

struct Session {
    pty: Box<dyn Pty + Send>,
    label: Option<String>,
    created: u64,
    activity: u64,
    cols: u16,
    rows: u16,
    exit: Option<i32>,
    /// `session_close` pedido: sai da tabela assim que o `05` for entregue.
    closed: bool,
    ring: Ring,
    /// Bytes que saíram do ring no último `on_output`.
    dropped: Vec<u8>,
    input_seq: HashMap<Pk, u64>,
    att: HashMap<ConnId, Attach>,
}

impl Session {
    /// Aplica o tamanho de `conn` ao PTY (regra: vence o último que digitou ou redimensionou).
    fn apply_size(&mut self, conn: ConnId) -> R<()> {
        let Some((c, r)) = self.att.get(&conn).and_then(|a| a.size) else { return Ok(()) };
        if (c, r) != (self.cols, self.rows) && self.exit.is_none() {
            self.pty.resize(c, r).map_err(|_| IoError)?;
            (self.cols, self.rows) = (c, r);
        }
        Ok(())
    }

    /// Envia à conexão o que couber na fila (1 MiB), mantendo a ordem gap → snapshot → frames → replay_end → 05.
    fn fill(&mut self, sid: Sid, conn: ConnId) -> Vec<Out> {
        let (start, end, exit) = (self.ring.start, self.ring.end(), self.exit);
        let Some(a) = self.att.get_mut(&conn) else { return vec![] };
        let s = uuid_str(&sid);
        let mut out = vec![];
        loop {
            // Fila cheia: para de enfileirar; o gap só é anunciado ao recuperar (§6).
            if a.next < start && a.queued < QUEUE {
                out.push(Out::Ctl(json!({"type":"replay_gap","session_id":s,"requested":u64s(a.next),"available_from":u64s(start)})));
                out.push(Out::Snapshot { session_id: sid, offset: start });
                a.next = start;
                a.replay_to = a.replay_to.map(|t| t.max(start));
            }
            if a.replay_to == Some(a.next) {
                out.push(Out::Ctl(json!({"type":"replay_end","session_id":s,"offset":u64s(a.next)})));
                a.replay_to = None;
            }
            let limit = a.replay_to.unwrap_or(end);
            let n = ((limit - a.next) as usize).min(QUEUE - a.queued).min(MAX_IO);
            if n == 0 { break; }
            out.push(Out::Frame(Frame { session_id: sid, seq: a.next, op: Op::Output(self.ring.read(a.next, n)) }));
            a.next += n as u64;
            a.queued += n;
        }
        if let (Some(code), None, false) = (exit, a.replay_to, a.exit_sent)
            && a.next == end {
                a.exit_sent = true;
                out.push(Out::Ctl(json!({"type":"session_exited","session_id":s,"exit_code":code,"final_offset":u64s(end)})));
                out.push(Out::Frame(Frame { session_id: sid, seq: end, op: Op::Exited(code) }));
            }
        out
    }
}

#[derive(Default)]
pub struct Sessions { s: HashMap<Sid, Session> }

fn new_sid() -> R<Sid> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|_| IoError)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(b)
}

impl Sessions {
    /// `session_create`: o perfil já foi resolvido e o PTY aberto por quem chama.
    pub fn create(&mut self, pty: Box<dyn Pty + Send>, label: Option<String>, cols: u16, rows: u16, now: u64) -> R<Sid> {
        self.create_as(new_sid()?, pty, label, cols, rows, now)
    }

    /// Como `create`, com o id escolhido (restauração do disco mantém o id que a UI conhece).
    pub fn create_as(&mut self, sid: Sid, pty: Box<dyn Pty + Send>, label: Option<String>, cols: u16, rows: u16, now: u64) -> R<Sid> {
        if self.s.len() >= MAX_SESSIONS { return Err(LimitExceeded); }
        if self.s.contains_key(&sid) { return Err(InvalidMessage); }
        let ring = Ring { buf: VecDeque::new(), start: 0, nl: VecDeque::new() };
        self.s.insert(sid, Session { pty, label, created: now, activity: now, cols, rows, exit: None, closed: false, ring, dropped: vec![], input_seq: HashMap::new(), att: HashMap::new() });
        Ok(sid)
    }

    /// Entradas de `session_list_ok.sessions` (§2.3).
    pub fn list(&self) -> Vec<Value> {
        self.s.iter().map(|(sid, x)| json!({
            "session_id": uuid_str(sid), "label": x.label.clone().unwrap_or_default(),
            "state": if x.exit.is_some() { "exited" } else { "running" },
            "created_at_ms": u64s(x.created), "last_activity_ms": u64s(x.activity),
            "cols": x.cols, "rows": x.rows, "attached_count": x.att.len(),
        })).collect()
    }

    /// `session_attach`: devolve o corpo de `session_attach_ok` e o que segue (gap, snapshot, replay, ao vivo).
    pub fn attach(&mut self, conn: ConnId, pk: Pk, sid: Sid, from: Option<u64>, size: Option<(u16, u16)>, now: u64) -> R<(Value, Vec<Out>)> {
        let x = self.s.get_mut(&sid).ok_or(NotFound)?;
        let (start, end) = (x.ring.start, x.ring.end());
        if from.is_some_and(|f| f > end) { return Err(InvalidMessage); }
        if !x.att.contains_key(&conn) && x.att.len() >= MAX_ATTACH { return Err(LimitExceeded); }
        let from = from.unwrap_or(start);
        x.att.insert(conn, Attach { pk, next: from.max(start), queued: 0, replay_to: Some(end), exit_sent: false, size });
        x.activity = now;
        x.apply_size(conn)?;
        let s = uuid_str(&sid);
        let mut ok = json!({"type":"session_attach_ok","session_id":s,"state":if x.exit.is_some() {"exited"} else {"running"},
            "buffer_start":u64s(start),"output_end":u64s(end),"last_input_seq":u64s(x.input_seq.get(&pk).copied().unwrap_or(0))});
        if let Some(c) = x.exit { ok["exit_code"] = c.into(); }
        let mut out = vec![];
        if from < start {
            out.push(Out::Ctl(json!({"type":"replay_gap","session_id":s,"requested":u64s(from),"available_from":u64s(start)})));
            out.push(Out::Snapshot { session_id: sid, offset: start });
        }
        out.push(Out::Ctl(json!({"type":"replay_begin","session_id":s,"from_offset":u64s(from.max(start)),"to_offset":u64s(end)})));
        out.extend(x.fill(sid, conn));
        Ok((ok, out))
    }

    pub fn detach(&mut self, conn: ConnId, sid: Sid, now: u64) -> R<Value> {
        let x = self.s.get_mut(&sid).ok_or(NotFound)?;
        x.att.remove(&conn).ok_or(NotFound)?;
        x.activity = now;
        Ok(json!({"type":"session_detach_ok"}))
    }

    /// `session_rename`: o rótulo vai para o disco junto com a sessão (`sessions.json`, ver `lib.rs`).
    pub fn rename(&mut self, sid: Sid, label: String) -> R<Value> {
        self.s.get_mut(&sid).ok_or(NotFound)?.label = Some(label);
        Ok(json!({"type":"session_rename_ok"}))
    }

    /// `session_close`: encerra o processo; `session_exited`/`05` seguem por `on_exit`.
    pub fn close(&mut self, sid: Sid) -> R<Value> {
        let x = self.s.get_mut(&sid).ok_or(NotFound)?;
        if x.exit.is_none() { x.pty.kill().map_err(|_| IoError)?; x.closed = true; } else { self.s.remove(&sid); }
        Ok(json!({"type":"session_close_ok"}))
    }

    /// Frame `01`. `Ok(None)`: sessão `exited` ou já fechada (corrida com outro cliente), descartado sem ACK.
    pub fn input(&mut self, conn: ConnId, sid: Sid, seq: u64, data: &[u8], now: u64) -> R<Option<Value>> {
        let Some(x) = self.s.get_mut(&sid) else { return Ok(None) };
        let pk = x.att.get(&conn).ok_or(InvalidMessage)?.pk;
        if x.exit.is_some() { return Ok(None); }
        let last = x.input_seq.get(&pk).copied().unwrap_or(0);
        if seq > last.saturating_add(1) || seq == 0 && last == 0 { return Err(InvalidMessage); }
        if seq == last + 1 {
            x.apply_size(conn)?;
            x.pty.write(data).map_err(|_| IoError)?;
            x.input_seq.insert(pk, seq);
            x.activity = now;
        }
        Ok(Some(json!({"type":"input_ack","session_id":uuid_str(&sid),"seq":u64s(x.input_seq[&pk])})))
    }

    /// Frame `03` de uma conexão anexada.
    pub fn resize(&mut self, conn: ConnId, sid: Sid, cols: u16, rows: u16) -> R<()> {
        let Some(x) = self.s.get_mut(&sid) else { return Ok(()) };
        x.att.get_mut(&conn).ok_or(InvalidMessage)?.size = Some((cols, rows));
        x.apply_size(conn)
    }

    /// Bytes lidos do PTY (sempre lidos, nunca bloqueados pela rede).
    pub fn on_output(&mut self, sid: Sid, data: &[u8], now: u64) -> Vec<(ConnId, Out)> {
        let Some(x) = self.s.get_mut(&sid) else { return vec![] };
        x.dropped = x.ring.push(data);
        x.activity = now;
        Self::fill_all(x, sid)
    }

    pub fn on_exit(&mut self, sid: Sid, code: i32) -> Vec<(ConnId, Out)> {
        let Some(x) = self.s.get_mut(&sid) else { return vec![] };
        x.exit.get_or_insert(code);
        let mut outs = Self::fill_all(x, sid);
        if x.closed {
            // Fechada: todo anexo recebe o fim já, mesmo pausado; a saída que não coube na fila se perde.
            let (end, s) = (x.ring.end(), uuid_str(&sid));
            for c in x.att.iter().filter(|(_, a)| !a.exit_sent).map(|(c, _)| *c) {
                outs.push((c, Out::Ctl(json!({"type":"session_exited","session_id":s,"exit_code":code,"final_offset":u64s(end)}))));
                outs.push((c, Out::Frame(Frame { session_id: sid, seq: end, op: Op::Exited(code) })));
            }
            self.s.remove(&sid);
        }
        outs
    }

    fn fill_all(x: &mut Session, sid: Sid) -> Vec<(ConnId, Out)> {
        let conns: Vec<ConnId> = x.att.keys().copied().collect();
        conns.into_iter().flat_map(|c| x.fill(sid, c).into_iter().map(move |o| (c, o))).collect()
    }

    /// A fila de envio da conexão drenou `n` bytes de saída: retoma, com `replay_gap` se ficou para trás.
    pub fn sent(&mut self, conn: ConnId, sid: Sid, n: usize) -> Vec<Out> {
        let Some(x) = self.s.get_mut(&sid) else { return vec![] };
        let Some(a) = x.att.get_mut(&conn) else { return vec![] };
        a.queued = a.queued.saturating_sub(n);
        x.fill(sid, conn)
    }

    /// Sessões para o disco: (id, rótulo, cols, rows, últimos `tail` bytes de saída). Fica de fora só a fechada de propósito:
    /// no desligamento o Windows mata os shells antes do app, e essas saídas não podem apagar o arquivo.
    /// Escreve direto no PTY (comando inicial do agente).
    pub fn type_in(&mut self, sid: &Sid, data: &[u8]) { if let Some(x) = self.s.get_mut(sid) { let _ = x.pty.write(data); } }
    pub fn saved(&self, tail: usize) -> Vec<(Sid, String, u16, u16, Vec<u8>)> {
        self.s.iter().filter(|(_, x)| !x.closed).map(|(sid, x)| {
            let n = x.ring.buf.len().min(tail);
            (*sid, x.label.clone().unwrap_or_default(), x.cols, x.rows, x.ring.read(x.ring.end() - n as u64, n))
        }).collect()
    }

    /// Bytes descartados do ring pelo último `on_output` (a tela no `buffer_start` avança com eles).
    pub fn dropped(&self, sid: &Sid) -> &[u8] { self.s.get(sid).map_or(&[], |x| &x.dropped) }
    /// Conexões anexadas (destino de `command_completed`/`cwd_changed`).
    pub fn attached(&self, sid: &Sid) -> Vec<ConnId> { self.s.get(sid).map(|x| x.att.keys().copied().collect()).unwrap_or_default() }
    /// Tamanho atual do PTY (a tela do host acompanha).
    pub fn size(&self, sid: &Sid) -> Option<(u16, u16)> { self.s.get(sid).map(|x| (x.cols, x.rows)) }

    /// PeerConnection fechada: remove os anexos; a contagem de input sobrevive (§4).
    pub fn disconnect(&mut self, conn: ConnId, now: u64) {
        for x in self.s.values_mut() { if x.att.remove(&conn).is_some() { x.activity = now; } }
    }

    /// Revogação passos 2–3 (§7); o passo 1 (fechar as PeerConnections) é de quem chama.
    pub fn revoke(&mut self, pk: &Pk, now: u64) {
        for x in self.s.values_mut() {
            let n = x.att.len();
            x.att.retain(|_, a| a.pk != *pk);
            if x.att.len() != n { x.activity = now; }
            x.input_seq.remove(pk);
        }
    }

    /// TTL: encerra sessões sem anexo e sem I/O há 24 h. Devolve as removidas.
    pub fn tick(&mut self, now: u64) -> Vec<Sid> {
        let dead: Vec<Sid> = self.s.iter().filter(|(_, x)| x.att.is_empty() && now.saturating_sub(x.activity) >= TTL_MS).map(|(k, _)| *k).collect();
        for sid in &dead { if let Some(mut x) = self.s.remove(sid) && x.exit.is_none() { let _ = x.pty.kill(); } }
        dead
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Log { input: Vec<u8>, sizes: Vec<(u16, u16)>, killed: bool }
    struct Fake(Arc<Mutex<Log>>);
    impl Pty for Fake {
        fn write(&mut self, d: &[u8]) -> std::io::Result<()> { self.0.lock().unwrap().input.extend(d); Ok(()) }
        fn resize(&mut self, c: u16, r: u16) -> std::io::Result<()> { self.0.lock().unwrap().sizes.push((c, r)); Ok(()) }
        fn kill(&mut self) -> std::io::Result<()> { self.0.lock().unwrap().killed = true; Ok(()) }
    }
    const A: Pk = [1; 32];
    const B: Pk = [2; 32];

    fn new() -> (Sessions, Sid, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let mut t = Sessions::default();
        let sid = t.create(Box::new(Fake(log.clone())), None, 80, 24, 0).unwrap();
        (t, sid, log)
    }
    fn frames(o: &[Out]) -> Vec<(u64, usize)> {
        o.iter().filter_map(|o| match o { Out::Frame(Frame { seq, op: Op::Output(d), .. }) => Some((*seq, d.len())), _ => None }).collect()
    }
    fn types(o: &[Out]) -> Vec<String> {
        o.iter().map(|o| match o { Out::Ctl(v) => v["type"].as_str().unwrap().into(), Out::Frame(f) => match f.op { Op::Output(_) => "02", Op::Exited(_) => "05", _ => "?" }.into(), Out::Snapshot { .. } => "snapshot".into() }).collect()
    }

    #[test]
    fn a3_n4_input_dedup() {
        let (mut t, sid, log) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        assert_eq!(t.input(1, sid, 0, b"x", 0), Err(InvalidMessage));
        assert_eq!(t.input(1, sid, 1, b"a", 0).unwrap().unwrap()["seq"], "1");
        assert_eq!(t.input(1, sid, 2, b"b", 0).unwrap().unwrap()["seq"], "2");
        // A3: reenvio após reconexão → descartado e re-ACK; contagem sobrevive.
        t.disconnect(1, 0);
        let (ok, _) = t.attach(2, A, sid, None, None, 0).unwrap();
        assert_eq!(ok["last_input_seq"], "2");
        assert_eq!(t.input(2, sid, 2, b"b", 0).unwrap().unwrap()["seq"], "2");
        assert_eq!(log.lock().unwrap().input, b"ab");
        // N4: pulo.
        assert_eq!(t.input(2, sid, 4, b"d", 0), Err(InvalidMessage));
        // Não anexado (conexão antiga) → INVALID_MESSAGE; seq é por client_pk.
        assert_eq!(t.input(1, sid, 3, b"c", 0), Err(InvalidMessage));
        t.attach(3, B, sid, None, None, 0).unwrap();
        assert_eq!(t.input(3, sid, 1, b"z", 0).unwrap().unwrap()["seq"], "1");
        // Exited: descartado sem ACK.
        t.on_exit(sid, 0);
        assert_eq!(t.input(2, sid, 3, b"c", 0), Ok(None));
        assert_eq!(log.lock().unwrap().input, b"abz");
    }

    #[test]
    fn a5_two_clients_same_output() {
        let (mut t, sid, _) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        t.attach(2, B, sid, None, None, 0).unwrap();
        let out = t.on_output(sid, &[0xff, 0x03, b'\n'], 1);
        let by = |c| out.iter().filter(|(k, _)| *k == c).map(|(_, o)| o.clone()).collect::<Vec<_>>();
        assert_eq!(by(1), by(2));
        assert_eq!(by(1), vec![Out::Frame(Frame { session_id: sid, seq: 0, op: Op::Output(vec![0xff, 0x03, b'\n']) })]);
    }

    #[test]
    fn a2_a4_replay_order_and_gap() {
        let (mut t, sid, _) = new();
        let chunk = vec![b'x'; MAX_IO];
        for _ in 0..(RING_BYTES / MAX_IO + 3) { t.on_output(sid, &chunk, 0); }
        let end = ((RING_BYTES / MAX_IO + 3) * MAX_IO) as u64;
        let start = end - RING_BYTES as u64;
        let (ok, out) = t.attach(1, A, sid, Some(0), None, 0).unwrap();
        assert_eq!((ok["buffer_start"].as_str().unwrap(), ok["output_end"].as_str().unwrap()), (start.to_string().as_str(), end.to_string().as_str()));
        let ty = types(&out);
        assert_eq!(&ty[..3], ["replay_gap", "snapshot", "replay_begin"]);
        assert_eq!(out[0], Out::Ctl(json!({"type":"replay_gap","session_id":uuid_str(&sid),"requested":"0","available_from":start.to_string()})));
        assert_eq!(out[1], Out::Snapshot { session_id: sid, offset: start });
        // 1 MiB de fila: o resto do replay vem conforme a fila drena; offsets contíguos, sem duplicata.
        let mut got = frames(&out);
        let mut all = out;
        while all.iter().all(|o| !matches!(o, Out::Ctl(v) if v["type"] == "replay_end")) {
            let o = t.sent(1, sid, QUEUE);
            got.extend(frames(&o));
            all = o;
        }
        let mut at = start;
        for (s, n) in &got { assert_eq!(*s, at); at += *n as u64; }
        assert_eq!(at, end);
        // Ao vivo segue sem buraco (depois que a fila drena).
        assert!(t.sent(1, sid, QUEUE).is_empty());
        let live = t.on_output(sid, b"y", 0);
        assert_eq!(frames(&live.into_iter().map(|x| x.1).collect::<Vec<_>>()), vec![(end, 1)]);
        // A2: attach de um ponto dentro do buffer, sem gap.
        let (_, out) = t.attach(2, B, sid, Some(end - 5), None, 0).unwrap();
        assert_eq!(types(&out), ["replay_begin", "02", "replay_end"]);
        assert_eq!(frames(&out), vec![(end - 5, 6)]);
        assert_eq!(t.attach(3, B, sid, Some(end + 2), None, 0).unwrap_err(), InvalidMessage);
    }

    #[test]
    fn ring_line_limit() {
        let (mut t, sid, _) = new();
        t.on_output(sid, &b"a\n".repeat(RING_LINES + 5), 0);
        let (ok, out) = t.attach(1, A, sid, None, None, 0).unwrap();
        assert_eq!(ok["buffer_start"], "10");
        assert_eq!(types(&out)[0], "replay_begin");
        assert_eq!(frames(&out).iter().map(|f| f.1).sum::<usize>(), 2 * RING_LINES);
    }

    #[test]
    fn a6_close_libera_vaga() {
        let (mut t, sid, log) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        t.close(sid).unwrap();
        assert!(log.lock().unwrap().killed);
        assert_eq!(t.list().len(), 1, "fica até o 05 sair");
        assert!(types(&t.on_exit(sid, 1).into_iter().map(|(_, o)| o).collect::<Vec<_>>()).contains(&"05".to_string()));
        assert!(t.list().is_empty());
        // já `exited`: sai na hora
        let s2 = t.create(Box::new(Fake(Default::default())), None, 80, 24, 0).unwrap();
        t.on_exit(s2, 0);
        t.close(s2).unwrap();
        assert!(t.list().is_empty());
        for _ in 0..MAX_SESSIONS { let s = t.create(Box::new(Fake(Default::default())), None, 80, 24, 0).unwrap(); t.on_exit(s, 0); t.close(s).unwrap(); }
    }

    #[test]
    fn close_com_cliente_pausado_ainda_entrega_05() {
        let (mut t, sid, _) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        t.attach(2, B, sid, None, None, 0).unwrap();
        t.on_output(sid, &vec![b'x'; QUEUE + 10], 0); // ambos pausados em 1 MiB
        t.close(sid).unwrap();
        let out = t.on_exit(sid, 1);
        for c in [1, 2] {
            let mine: Vec<Out> = out.iter().filter(|(k, _)| *k == c).map(|(_, o)| o.clone()).collect();
            let ty = types(&mine);
            assert!(ty.contains(&"session_exited".to_string()) && ty.contains(&"05".to_string()), "conn {c}: {ty:?}");
        }
        assert!(t.list().is_empty());
        // input/resize atrasados de quem não viu o close: descartados, conexão não cai
        assert!(matches!(t.input(1, sid, 1, b"a", 0), Ok(None)));
        assert!(t.resize(1, sid, 10, 10).is_ok());
    }

    #[test]
    fn a7_ttl() {
        let (mut t, sid, _) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        assert!(t.tick(TTL_MS * 2).is_empty(), "anexado não expira");
        t.disconnect(1, 1000);
        t.on_output(sid, b"x", 5000);
        assert!(t.tick(5000 + TTL_MS - 1).is_empty());
        assert_eq!(t.tick(5000 + TTL_MS), vec![sid]);
        assert_eq!(t.attach(1, A, sid, None, None, 0).unwrap_err(), NotFound);
    }

    #[test]
    fn n5_n6_not_found_and_limits() {
        let (mut t, sid, _) = new();
        assert_eq!(t.attach(1, A, [9; 16], None, None, 0).unwrap_err(), NotFound);
        assert_eq!(t.detach(1, sid, 0).unwrap_err(), NotFound);
        assert_eq!(t.close([9; 16]).unwrap_err(), NotFound);
        for c in 1..=4 { t.attach(c, A, sid, None, None, 0).unwrap(); }
        assert_eq!(t.attach(5, A, sid, None, None, 0).unwrap_err(), LimitExceeded);
        t.attach(4, A, sid, None, None, 0).unwrap();
        for _ in 1..MAX_SESSIONS { t.create(Box::new(Fake(Default::default())), None, 80, 24, 0).unwrap(); }
        assert_eq!(t.create(Box::new(Fake(Default::default())), None, 80, 24, 0).unwrap_err(), LimitExceeded);
        assert_eq!(t.list().len(), MAX_SESSIONS);
    }

    #[test]
    fn n8_slow_client_bounded() {
        let (mut t, sid, _) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        let chunk = vec![b'z'; MAX_IO];
        let mut queued = 0;
        for _ in 0..(50 << 20) / MAX_IO { queued += frames(&t.on_output(sid, &chunk, 0).into_iter().map(|x| x.1).collect::<Vec<_>>()).iter().map(|f| f.1).sum::<usize>(); }
        assert_eq!(queued, QUEUE);
        let x = &t.s[&sid];
        assert_eq!(x.ring.buf.len(), RING_BYTES);
        let out = t.sent(1, sid, QUEUE);
        assert_eq!(&types(&out)[..2], ["replay_gap", "snapshot"]);
        assert_eq!(frames(&out)[0].0, (50u64 << 20) - RING_BYTES as u64);
    }

    #[test]
    fn resize_last_typer_or_resizer_wins() {
        let (mut t, sid, log) = new();
        t.attach(1, A, sid, None, Some((80, 24)), 0).unwrap();
        t.attach(2, B, sid, None, Some((100, 30)), 0).unwrap();
        t.input(1, sid, 1, b"a", 0).unwrap();
        t.resize(2, sid, 120, 40).unwrap();
        t.input(2, sid, 1, b"b", 0).unwrap();
        assert_eq!(log.lock().unwrap().sizes, [(100, 30), (80, 24), (120, 40)]);
        assert_eq!(t.resize(3, sid, 1, 1), Err(InvalidMessage));
    }

    #[test]
    fn revoke_drops_attach_and_input_state() {
        let (mut t, sid, log) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        t.attach(2, B, sid, None, None, 0).unwrap();
        t.input(1, sid, 1, b"a", 0).unwrap();
        t.revoke(&A, 0);
        assert_eq!(t.input(1, sid, 2, b"b", 0), Err(InvalidMessage));
        assert!(t.on_output(sid, b"o", 0).iter().all(|(c, _)| *c == 2));
        assert_eq!(t.list()[0]["attached_count"], 1);
        assert!(!log.lock().unwrap().killed, "sessão continua viva");
    }

    #[test]
    fn rename_aparece_na_lista() {
        let (mut t, sid, _) = new();
        assert_eq!(t.rename(sid, "x".into()).unwrap()["type"], "session_rename_ok");
        assert_eq!(t.list()[0]["label"], "x");
        assert!(t.rename([9; 16], "y".into()).is_err());
    }

    #[test]
    fn exit_after_output_and_close() {
        let (mut t, sid, log) = new();
        t.attach(1, A, sid, None, None, 0).unwrap();
        t.on_output(sid, b"bye", 0);
        assert_eq!(t.close(sid).unwrap()["type"], "session_close_ok");
        assert!(log.lock().unwrap().killed);
        let out: Vec<Out> = t.on_exit(sid, 7).into_iter().map(|x| x.1).collect();
        assert_eq!(types(&out), ["session_exited", "05"]);
        assert_eq!(out[0], Out::Ctl(json!({"type":"session_exited","session_id":uuid_str(&sid),"exit_code":7,"final_offset":"3"})));
        // decisão 7: após session_close o buffer some
        assert_eq!(t.attach(2, B, sid, Some(0), None, 0).unwrap_err(), NotFound);
    }

    #[test]
    fn exited_sem_close_continua_anexavel() {
        let (mut t, sid, _) = new();
        t.on_output(sid, b"bye", 0);
        t.on_exit(sid, 7);
        let (ok, out) = t.attach(2, B, sid, Some(0), None, 0).unwrap();
        assert_eq!((ok["state"].as_str(), ok["exit_code"].as_i64()), (Some("exited"), Some(7)));
        assert_eq!(types(&out), ["replay_begin", "02", "replay_end", "session_exited", "05"]);
        assert_eq!(t.list()[0]["state"], "exited");
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::screen::Screen;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    struct P;
    impl Pty for P {
        fn write(&mut self, _: &[u8]) -> std::io::Result<()> { Ok(()) }
        fn resize(&mut self, _: u16, _: u16) -> std::io::Result<()> { Ok(()) }
        fn kill(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    /// E7 (PLANO-M3 §5) no PC: vim real no ConPTY, gap forçado, snapshot + replay = tela real.
    #[test]
    #[ignore = "precisa de vim (LIVE_CMD_VIM=caminho)"]
    fn e7_vim_apos_gap() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (cols, rows) = (80u16, 24u16);
        let (tx, rx) = mpsc::channel();
        let mut pty = crate::conpty::ConPty::spawn("cmd.exe", None, cols, rows, move |e| { if let crate::conpty::Ev::Output(d) = e { let _ = tx.send(d); } }).unwrap();
        let mut t = Sessions::default();
        let sid = t.create(Box::new(P), None, cols, rows, 0).unwrap();
        let mut base = Screen::new(cols, rows, false);
        let mut truth = vt100::Parser::new(rows.try_into().unwrap(), cols.try_into().unwrap(), 0);
        // lê até `done` na tela real e 1 s sem saída
        let mut pump = |t: &mut Sessions, done: &dyn Fn(&str) -> bool| loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(d) => { t.on_output(sid, &d, 0); base.feed(t.dropped(&sid)); truth.process(&d); }
                Err(_) if done(&truth.screen().contents()) => return,
                Err(e) => assert!(e == mpsc::RecvTimeoutError::Timeout, "pty fechou"),
            }
        };
        pty.write(b"for /L %i in (1,1,11000) do @echo linha-%i\r").unwrap();
        pump(&mut t, &|s| s.contains("linha-11000"));
        let vim = std::env::var("LIVE_CMD_VIM").unwrap();
        pty.write(format!("\"{vim}\" -u NONE -N -n\r").as_bytes()).unwrap();
        pump(&mut t, &|s| s.contains('~'));
        pty.write(b"ie7-vim-ok\x1b:set number\r").unwrap();
        pump(&mut t, &|s| s.contains("1 e7-vim-ok"));
        drop(pump);
        assert!(t.s[&sid].ring.start > 0, "precisa de gap");
        let (_, out) = t.attach(1, [1; 32], sid, Some(0), None, 0).unwrap();
        let mut client = vt100::Parser::new(rows.try_into().unwrap(), cols.try_into().unwrap(), 0);
        let mut snap = false;
        let mut apply = |o: Out| match o {
            Out::Snapshot { offset, .. } => { snap = true; client.process(&B64.decode(base.snapshot_body(&sid, offset)["data"].as_str().unwrap()).unwrap()); }
            Out::Frame(Frame { op: Op::Output(d), .. }) => client.process(&d),
            _ => {}
        };
        for o in out { apply(o); }
        loop {
            let more = t.sent(1, sid, QUEUE);
            if more.is_empty() { break; }
            for o in more { apply(o); }
        }
        drop(apply);
        assert!(snap);
        assert_eq!(client.screen().contents_formatted(), truth.screen().contents_formatted());
        assert_eq!(client.screen().cursor_position(), truth.screen().cursor_position());
        pty.write(b"\x1b:q!\rexit\r").unwrap();
    }

    /// A4 com snapshot: tela-base (bytes que saíram do ring) + replay = tela de quem viu tudo.
    #[test]
    fn snapshot_mais_replay_igual_a_tela_real() {
        let mut t = Sessions::default();
        let sid = t.create(Box::new(P), None, 40, 10, 0).unwrap();
        let mut base = Screen::new(40, 10, false);
        let mut truth = vt100::Parser::new(10.try_into().unwrap(), 40.try_into().unwrap(), 0);
        // poucas linhas e muita barra de progresso (\r): o ring corta por bytes e a tela antiga ainda aparece
        let mut feed = |d: &[u8]| { t.on_output(sid, d, 0); base.feed(t.dropped(&sid)); truth.process(d); };
        feed(b"\x1b[31mtopo\x1b[m\r\nsegunda\r\n");
        for i in 0..(RING_BYTES / 4000 + 300) { feed(format!("\r{:>30}\x1b[1;{}H\x1b[4{}m \x1b[m\x1b[3;1H", i % 997, i % 40 + 1, i % 8).as_bytes().repeat(100).as_slice()); }
        feed(b"\x1b[10;1H\r\nfim1\r\nfim2\r\nfim3\x1b[2;8r\x1b[8;1H\nr\x1b[r\x1b[5;1H\x1b[2L");
        assert!(t.s[&sid].ring.start > 0, "precisa de gap");
        let (_, out) = t.attach(1, [1; 32], sid, Some(0), None, 0).unwrap();
        let mut client = vt100::Parser::new(10.try_into().unwrap(), 40.try_into().unwrap(), 0);
        let mut snap = false;
        for o in out {
            match o {
                Out::Snapshot { offset, .. } => {
                    snap = true;
                    let b = base.snapshot_body(&sid, offset);
                    client.process(&B64.decode(b["data"].as_str().unwrap()).unwrap());
                }
                Out::Frame(Frame { op: Op::Output(d), .. }) => client.process(&d),
                _ => {}
            }
        }
        // a fila de 1 MiB corta o replay; drena o resto
        loop {
            let more = t.sent(1, sid, QUEUE);
            if more.is_empty() { break; }
            for o in more { if let Out::Frame(Frame { op: Op::Output(d), .. }) = o { client.process(&d); } }
        }
        assert!(snap);
        assert_eq!(client.screen().contents_formatted(), truth.screen().contents_formatted());
        assert_eq!(client.screen().cursor_position(), truth.screen().cursor_position());
    }
}

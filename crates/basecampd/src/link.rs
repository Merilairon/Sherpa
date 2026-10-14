//! Request/response engine for the Everest command family (spec 04 §2, 02 §2.2).
//!
//! One reader thread splits input reports into events (`01`..`0A`) and replies. A
//! per-device lock is held for a whole command, including every chunk, so packets of two
//! commands never interleave (C-1). Each packet waits for its acknowledgement (C-2).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use basecamp_core::everest::{self, timeout};
use basecamp_core::multipart::{Assembler, Form};
use basecamp_core::{hex, Packet};
use basecamp_hid::HidIo;

use crate::error::{Error, Result};

/// An opened HID endpoint: the device node plus the report ids of its collection.
#[derive(Clone)]
pub struct Endpoint {
    pub io: Arc<dyn HidIo>,
    /// Report id written before each output/feature payload (0 when unused).
    pub out_id: u8,
    /// Input report ids of this collection; empty when the descriptor declares none.
    pub in_ids: Vec<u8>,
}

impl Endpoint {
    pub fn new(io: Arc<dyn HidIo>, out_id: u8, in_ids: Vec<u8>) -> Self {
        Endpoint { io, out_id, in_ids }
    }

    /// Write `reportId ‖ payload`.
    pub fn write_payload(&self, payload: &[u8]) -> Result<()> {
        let mut buf = Vec::with_capacity(payload.len() + 1);
        buf.push(self.out_id);
        buf.extend_from_slice(payload);
        self.io.write(&buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::BrokenPipe || e.raw_os_error() == Some(19) {
                Error::Disconnected
            } else {
                Error::Io(e.to_string())
            }
        })
    }

    /// Strip the report id of an input report; `None` if it belongs to another collection.
    pub fn strip(&self, mut r: Vec<u8>) -> Option<Vec<u8>> {
        if self.in_ids.is_empty() {
            return Some(r);
        }
        if r.is_empty() || !self.in_ids.contains(&r[0]) {
            return None;
        }
        r.remove(0);
        Some(r)
    }
}

pub fn to_packet(v: &[u8]) -> Packet {
    let mut p = [0u8; 64];
    let n = v.len().min(64);
    p[..n].copy_from_slice(&v[..n]);
    p
}

pub struct Link {
    ep: Endpoint,
    lock: Mutex<Receiver<Packet>>,
    alive: Arc<AtomicBool>,
}

/// Exclusive access to the device for one command (C-1).
pub struct Session<'a> {
    link: &'a Link,
    replies: MutexGuard<'a, Receiver<Packet>>,
}

impl Link {
    /// Start the reader thread. Returns the link and the event receiver.
    pub fn start(ep: Endpoint, name: &str) -> (Arc<Link>, Receiver<Packet>) {
        let (reply_tx, reply_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let link = Arc::new(Link { ep: ep.clone(), lock: Mutex::new(reply_rx), alive: alive.clone() });
        let tname = format!("reader-{name}");
        thread::Builder::new().name(tname).spawn(move || reader(ep, alive, reply_tx, event_tx)).expect("spawn reader");
        (link, event_rx)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Stop the reader thread (detach).
    pub fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.ep
    }

    pub fn session(&self) -> Result<Session<'_>> {
        if !self.is_alive() {
            return Err(Error::Disconnected);
        }
        let replies = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        Ok(Session { link: self, replies })
    }

    /// For timer ticks: `None` if another command is in flight (C-3).
    pub fn try_session(&self) -> Option<Session<'_>> {
        if !self.is_alive() {
            return None;
        }
        self.lock.try_lock().ok().map(|replies| Session { link: self, replies })
    }

    pub fn transact(&self, p: &Packet, timeout_ms: u64) -> Result<Packet> {
        self.session()?.transact(p, timeout_ms)
    }

    pub fn transact_all(&self, ps: &[Packet], timeout_ms: u64) -> Result<()> {
        self.session()?.transact_all(ps, timeout_ms)
    }

    pub fn multipart(&self, p: &Packet, form: Form, timeout_ms: u64) -> Result<Vec<u8>> {
        self.session()?.multipart(p, form, timeout_ms)
    }

    /// Feature-channel command (`AA 55` family): SetFeature then GetFeature.
    pub fn feature(&self, p: &Packet) -> Result<Packet> {
        let _s = self.session()?;
        let mut buf = vec![self.ep.out_id];
        buf.extend_from_slice(p);
        self.ep.io.set_feature(&buf)?;
        let r = self.ep.io.get_feature(self.ep.out_id, 65)?;
        Ok(to_packet(r.get(1..).unwrap_or(&[])))
    }
}

fn reader(ep: Endpoint, alive: Arc<AtomicBool>, replies: Sender<Packet>, events: Sender<Packet>) {
    while alive.load(Ordering::SeqCst) {
        match ep.io.read(Duration::from_millis(200)) {
            Ok(Some(r)) => {
                let Some(r) = ep.strip(r) else { continue };
                if r.is_empty() {
                    continue;
                }
                let p = to_packet(&r);
                if everest::is_event(&p) {
                    let _ = events.send(p);
                } else {
                    let _ = replies.send(p);
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::info!("device read ended: {e}");
                break;
            }
        }
    }
    alive.store(false, Ordering::SeqCst);
}

impl<'a> Session<'a> {
    pub fn endpoint(&self) -> &Endpoint {
        &self.link.ep
    }

    fn drain(&self) {
        while self.replies.try_recv().is_ok() {}
    }

    pub fn send(&self, p: &Packet) -> Result<()> {
        log::trace!("→ {}", hex(&p[..16]));
        self.link.ep.write_payload(p)
    }

    /// Wait for a reply accepted by `pick`, which returns `Some(result)` for a matching
    /// report and `None` to skip it.
    pub fn wait<T>(&self, timeout_ms: u64, what: &str, mut pick: impl FnMut(&Packet) -> Option<Result<T>>) -> Result<T> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout(what.to_string()));
            }
            match self.replies.recv_timeout(deadline - now) {
                Ok(r) => {
                    log::trace!("← {}", hex(&r[..16]));
                    if let Some(res) = pick(&r) {
                        return res;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(Error::Timeout(what.to_string())),
                Err(RecvTimeoutError::Disconnected) => return Err(Error::Disconnected),
            }
        }
    }

    fn ack(p: &Packet, r: &Packet) -> Option<Result<Packet>> {
        if everest::is_nak(r) {
            return Some(Err(Error::Nak(hex(&p[..8]))));
        }
        if everest::matches(p, r) {
            return Some(Ok(*r));
        }
        None
    }

    /// Send one packet and wait for its acknowledgement. A timeout is retried once (02 §2.3).
    pub fn transact(&self, p: &Packet, timeout_ms: u64) -> Result<Packet> {
        let what = hex(&p[..8]);
        for attempt in 0..2 {
            self.drain();
            self.send(p)?;
            match self.wait(timeout_ms, &what, |r| Self::ack(p, r)) {
                Err(Error::Timeout(_)) if attempt == 0 => {
                    log::debug!("timeout on {what}, retrying once");
                    continue;
                }
                other => return other,
            }
        }
        Err(Error::Timeout(what))
    }

    /// Multi-packet write: each chunk waits for the previous acknowledgement; the first
    /// failure aborts the rest.
    pub fn transact_all(&self, ps: &[Packet], timeout_ms: u64) -> Result<()> {
        for p in ps {
            self.transact(p, timeout_ms)?;
        }
        Ok(())
    }

    /// Multi-part read (§9). Returns the reassembled payload.
    pub fn multipart(&self, p: &Packet, form: Form, timeout_ms: u64) -> Result<Vec<u8>> {
        let what = hex(&p[..8]);
        self.drain();
        self.send(p)?;
        let mut asm = Assembler::new(form);
        loop {
            let done = self.wait(timeout_ms, &what, |r| {
                if everest::is_nak(r) {
                    return Some(Err(Error::Nak(what.clone())));
                }
                if everest::matches(p, r) {
                    return Some(Ok(asm.push(r)));
                }
                None
            })?;
            if done {
                return Ok(asm.finish());
            }
        }
    }
}

/// Default timeout for a packet (spec 04 tables).
pub fn timeout_for(p: &Packet) -> u64 {
    match (p[0], p[1]) {
        (0x14, 0x00) => timeout::SWITCH_PROFILE,
        (0x13, 0x55) => timeout::SAVE,
        (0x13, 0x40) => timeout::RESET_FLASH,
        (0x13, 0x41) | (0x13, 0x42) => timeout::RESET_PICTURES,
        (0x13, 0x60) => timeout::RESET_KEYS,
        (0x13, 0x61) => timeout::RESET_EFFECTS,
        (0x11, 0x14) => timeout::EXT_INFO,
        (0x11, 0x02) => timeout::KEY_CONTENT,
        (0x12, 0x00) | (0x12, 0x01) => timeout::SYNC_EFFECT,
        _ => timeout::DEFAULT,
    }
}

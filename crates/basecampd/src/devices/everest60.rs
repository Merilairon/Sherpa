//! Everest 60 driver (spec 10): SetFeature, per-command delay, GetFeature.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use basecamp_core::binding::KeyOp;
use basecamp_core::everest60::{self as e6, cmd};
use basecamp_core::model::Lighting;
use basecamp_core::{hex, Packet};

use crate::error::{Error, Result};
use crate::link::{to_packet, Endpoint};

pub struct Everest60Dev {
    ep: Endpoint,
    lock: Mutex<()>,
    alive: AtomicBool,
}

impl Everest60Dev {
    pub fn new(ep: Endpoint) -> Self {
        Everest60Dev { ep, lock: Mutex::new(()), alive: AtomicBool::new(true) }
    }

    pub fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn exec_locked(&self, p: &Packet) -> Result<Packet> {
        let mut buf = vec![0u8];
        buf.extend_from_slice(p);
        if let Err(e) = self.ep.io.set_feature(&buf) {
            if e.kind() == std::io::ErrorKind::BrokenPipe || e.raw_os_error() == Some(19) {
                self.close();
                return Err(Error::Disconnected);
            }
            return Err(e.into());
        }
        thread::sleep(Duration::from_millis(e6::delay_for(p[0])));
        let r = self.ep.io.get_feature(0, 65)?;
        let reply = to_packet(r.get(1..).unwrap_or(&[]));
        if !e6::reply_ok(p, &reply) {
            return Err(Error::Invalid(format!("unexpected reply to {}: {}", hex(&p[..8]), hex(&reply[..8]))));
        }
        Ok(reply)
    }

    /// One command, in order with all others (one worker queue per device).
    pub fn exec(&self, p: &Packet) -> Result<Packet> {
        if !self.is_alive() {
            return Err(Error::Disconnected);
        }
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.exec_locked(p)
    }

    /// Several commands as one atomic transfer.
    pub fn exec_all(&self, ps: &[Packet]) -> Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        for p in ps {
            self.exec_locked(p)?;
        }
        Ok(())
    }

    /// Setter returning r0 = 1 on success.
    pub fn set(&self, p: &Packet) -> Result<()> {
        let r = self.exec(p)?;
        if e6::result(&r, 0) != 1 {
            return Err(Error::Nak(hex(&p[..8])));
        }
        Ok(())
    }

    pub fn fw_info(&self) -> Result<e6::FwInfo> {
        Ok(e6::parse_fw_info(&self.exec(&e6::command(cmd::FW_INFO, &[]))?))
    }

    pub fn layout(&self) -> Result<u32> {
        Ok(e6::result(&self.exec(&e6::command(cmd::GET_LAYOUT, &[]))?, 0))
    }

    pub fn poll(&self) -> Result<e6::Poll> {
        Ok(e6::parse_poll(&self.exec(&e6::command(cmd::POLL, &[]))?))
    }

    pub fn switch_profile(&self, slot: u8, menu: u8) -> Result<()> {
        self.set(&e6::command(cmd::SWITCH_PROFILE, &[slot as u32, menu as u32]))
    }

    pub fn apply_lighting(&self, l: &Lighting, save: bool) -> Result<()> {
        let mut ps = vec![e6::lighting_packet(l)?];
        if save {
            ps.push(e6::command(cmd::SAVE, &[l.menu_index.value() as u32]));
        }
        self.exec_all(&ps)
    }

    pub fn apply_key(&self, fn_layer: bool, src: u16, op: &KeyOp) -> Result<()> {
        let codes = e6::layer_codes(fn_layer);
        match op {
            KeyOp::Remap(dst) => self.set(&e6::command(codes.remap, &[src as u32, *dst as u32])),
            KeyOp::Shortcut { key, mods } => {
                // TODO(spec): the Fn-layer shortcut command is not specified; 23 is used.
                self.set(&e6::command(cmd::SHORTCUT_REMAP, &[src as u32, *key as u32, *mods as u32]))
            }
            KeyOp::Macro { playback, events } => self.exec_all(&e6::macro_packets(fn_layer, src as u8, *playback, events)?),
            KeyOp::Launch(payload) => self.exec_all(&e6::launch_packets(fn_layer, src as u8, payload)),
            KeyOp::None => Ok(()),
        }
    }
}

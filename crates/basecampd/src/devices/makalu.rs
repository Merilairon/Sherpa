//! Makalu 67 driver (spec 11).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use basecamp_core::hex;
use basecamp_core::makalu67::{self as mk, Command, Status};

use crate::error::{Error, Result};
use crate::link::Endpoint;

pub struct MakaluDev {
    ep: Endpoint,
    lock: Mutex<()>,
    alive: Arc<AtomicBool>,
}

const BUSY_POLLS: u32 = 50;

impl MakaluDev {
    /// Open the driver and start the 9-byte event reader.
    pub fn start(ep: Endpoint) -> (Arc<MakaluDev>, Receiver<mk::Event>) {
        let alive = Arc::new(AtomicBool::new(true));
        let dev = Arc::new(MakaluDev { ep: ep.clone(), lock: Mutex::new(()), alive: alive.clone() });
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("makalu-events".into())
            .spawn(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                while alive.load(Ordering::SeqCst) {
                    match ep.io.read(Duration::from_millis(200)) {
                        Ok(Some(r)) => {
                            // Throttle to one event every 50 ms.
                            if last.elapsed() < Duration::from_millis(50) {
                                continue;
                            }
                            last = Instant::now();
                            if let Some(e) = mk::parse_event(&r) {
                                let _ = tx.send(e);
                            }
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
                alive.store(false, Ordering::SeqCst);
            })
            .expect("spawn");
        (dev, rx)
    }

    pub fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn status_locked(&self) -> Result<Vec<u8>> {
        for _ in 0..BUSY_POLLS {
            let r = self.ep.io.get_feature(mk::REPORT_CMD, mk::CMD_LEN)?;
            match mk::parse_status(&r) {
                Status::Ok => return Ok(r),
                Status::Busy => thread::sleep(Duration::from_millis(20)),
                Status::Failed(v) => return Err(Error::Nak(format!("status {v}"))),
            }
        }
        Err(Error::Timeout("device stayed busy".into()))
    }

    fn write_locked(&self, c: &Command) -> Result<Vec<u8>> {
        let mut tries = 0;
        loop {
            match self.ep.io.set_feature(&c.bytes) {
                Ok(()) => break,
                Err(e) if tries < 3 => {
                    log::debug!("SetFeature failed ({e}), retrying");
                    tries += 1;
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => return Err(e.into()),
            }
        }
        thread::sleep(Duration::from_millis(c.delay_ms));
        self.status_locked().map_err(|e| match e {
            Error::Nak(s) => Error::Nak(format!("{} ({s})", hex(&c.bytes[..8]))),
            other => other,
        })
    }

    pub fn exec(&self, c: &Command) -> Result<Vec<u8>> {
        if !self.is_alive() {
            return Err(Error::Disconnected);
        }
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.write_locked(c)
    }

    /// Button record, then (for macro/program/URL) its bulk block, retrying once.
    pub fn set_button(&self, profile: u8, button: u8, rec: &mk::ButtonRecord) -> Result<()> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.write_locked(&mk::button_command(profile, button, rec)?)?;
        if let Some(data) = &rec.bulk {
            let block = mk::bulk_write(profile, button, data)?;
            let mut attempt = 0;
            loop {
                let r = self.ep.io.set_feature(&block).map_err(Error::from).and_then(|_| {
                    thread::sleep(Duration::from_millis(100));
                    self.status_locked()
                });
                match r {
                    Ok(_) => break,
                    Err(e) if attempt == 0 => {
                        log::debug!("bulk upload failed ({e}), retrying once");
                        attempt += 1;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    /// Read a button's stored path/URL for an event.
    pub fn bulk_text(&self, e4: u8, e5: u8) -> Result<String> {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.write_locked(&mk::bulk_read_request(e4, e5))?;
        let r = self.ep.io.get_feature(mk::REPORT_BULK, mk::BULK_LEN)?;
        Ok(mk::bulk_text(&r))
    }

    pub fn current_profile(&self) -> Result<u8> {
        Ok(self.exec(&mk::get_current_profile())?[16])
    }

    pub fn firmware(&self) -> Result<u16> {
        let r = self.exec(&mk::firmware_version())?;
        Ok(u16::from_le_bytes([r[16], r[17]]))
    }
}

//! DisplayPad driver (spec 09).

use std::sync::Arc;

use basecamp_core::displaypad::{self as dp, Frame};
use basecamp_core::everest::{self as ev, Variant};
use basecamp_core::hex;

use crate::devices::everest::EverestDev;
use crate::error::{Error, Result};
use crate::link::{Endpoint, Link};

pub struct DisplayPadDev {
    /// Shared Everest-family commands on the command channel.
    pub cmd: EverestDev,
    pub bulk: Option<Endpoint>,
}

impl DisplayPadDev {
    pub fn new(link: Arc<Link>, bulk: Option<Endpoint>) -> Self {
        DisplayPadDev { cmd: EverestDev::new(link, Variant::MacroPad), bulk }
    }

    fn upload(&self, header: &basecamp_core::Packet, data: &[u8], stored: bool, timeout_ms: u64) -> Result<()> {
        let bulk = self.bulk.as_ref().ok_or_else(|| Error::Unsupported("DisplayPad image channel not available".into()))?;
        let s = self.cmd.link.session()?;
        let what = hex(&header[..14]);
        // 1. Header, wait for its echo (bytes 2–3 must be 00 00).
        s.send(header)?;
        s.wait(ev::timeout::DEFAULT, &what, |r| {
            if ev::is_nak(r) {
                return Some(Err(Error::Nak(what.clone())));
            }
            if r[0] == 0x21 && r[1] == header[1] {
                if r[2..4] == [0, 0] || r[..4] == header[..4] {
                    return Some(Ok(()));
                }
                if r[2..4] == [0xFF, 0xAA] {
                    return Some(Err(Error::Nak(what.clone())));
                }
                return Some(Err(Error::Invalid("DisplayPad is not ready for an image".into())));
            }
            None
        })?;
        // 2–3. Pixel data in 1024-byte bulk blocks, last one zero-padded, no per-block ack.
        for block in dp::bulk_blocks(data) {
            bulk.write_payload(&block)?;
        }
        // 4. Completion.
        s.wait(timeout_ms, &what, |r| match dp::completion(r, stored) {
            Some(Ok(())) => Some(Ok(())),
            Some(Err(())) => Some(Err(Error::Nak(what.clone()))),
            None => None,
        })
    }

    /// Upload a 102×102 B,G,R icon live (not stored) to key `icon` (0..11).
    pub fn icon_live(&self, icon: u8, bgr: &[u8]) -> Result<()> {
        check_icon(bgr)?;
        self.upload(&dp::full_icon_header(icon, None)?, bgr, false, dp::LIVE_TIMEOUT)
    }

    /// Store an icon into a profile's flash.
    pub fn icon_stored(&self, icon: u8, bgr: &[u8], frame: Frame) -> Result<()> {
        check_icon(bgr)?;
        let t = if frame.index <= 1 { dp::STORED_FIRST_TIMEOUT } else { dp::STORED_TIMEOUT };
        self.upload(&dp::full_icon_header(icon, Some(frame))?, bgr, true, t)
    }

    /// Live panel image of the whole 800×240 surface.
    pub fn panel(&self, bgr: &[u8]) -> Result<()> {
        if bgr.len() != dp::PANEL_BYTES {
            return Err(Error::Invalid(format!("panel image must be {} bytes", dp::PANEL_BYTES)));
        }
        let h = dp::panel_header(bgr.len(), 0, 0, (dp::PANEL_W - 1) as u16, (dp::PANEL_H - 1) as u16);
        self.upload(&h, bgr, false, dp::LIVE_TIMEOUT)
    }

    pub fn set_brightness(&self, level: u8) -> Result<()> {
        self.cmd.cmd(&dp::brightness(level)).map(|_| ())
    }

    pub fn brightness(&self) -> Result<u8> {
        Ok(self.cmd.cmd(&dp::brightness_request())?[4])
    }

    pub fn sleep_timer(&self, enable: bool, secs: u32) -> Result<()> {
        self.cmd.cmd(&dp::sleep_timer(enable, secs)).map(|_| ())
    }
}

fn check_icon(bgr: &[u8]) -> Result<()> {
    if bgr.len() != dp::ICON_BYTES {
        return Err(Error::Invalid(format!("icon must be {} bytes (102×102 B,G,R)", dp::ICON_BYTES)));
    }
    Ok(())
}

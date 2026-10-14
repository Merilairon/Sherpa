//! Everest Max and MacroPad driver (spec 04, 08).

use std::sync::Arc;

use sherpa_core::binding::KeyOp;
use sherpa_core::everest::{self as ev, timeout, ClockTime, ExtendInfo, FwInfo, KeyContent, Variant};
use sherpa_core::model::{CustomLighting, Lighting};
use sherpa_core::types::MenuIndex;
use sherpa_core::Packet;

use crate::error::{Error, Result};
use crate::link::{timeout_for, Link};

pub struct EverestDev {
    pub link: Arc<Link>,
    pub variant: Variant,
}

impl EverestDev {
    pub fn new(link: Arc<Link>, variant: Variant) -> Self {
        EverestDev { link, variant }
    }

    /// Send one command with its table timeout.
    pub fn cmd(&self, p: &Packet) -> Result<Packet> {
        self.link.transact(p, timeout_for(p))
    }

    pub fn fw_info(&self) -> Result<FwInfo> {
        Ok(ev::parse_fw_info(&self.cmd(&ev::fw_info_request())?))
    }

    pub fn layout(&self) -> Result<u8> {
        Ok(ev::parse_layout(&self.cmd(&ev::layout_request())?))
    }

    pub fn device_info(&self) -> Result<ev::DeviceInfo> {
        Ok(ev::parse_device_info(&self.link.feature(&ev::device_info_request())?))
    }

    pub fn switch_profile(&self, slot: u8, menu: MenuIndex) -> Result<()> {
        self.cmd(&ev::switch_profile(slot, menu.value())).map(|_| ())
    }

    /// Apply a lighting entry and save it under its menu index (F-LGT-3, §5.5 order).
    pub fn apply_lighting(&self, l: &Lighting, custom: &CustomLighting, save: bool) -> Result<()> {
        let s = self.link.session()?;
        let t = timeout::DEFAULT;
        if l.menu_index == MenuIndex::Custom {
            s.transact(&ev::custom_mode(l.brightness), t)?;
            s.transact_all(&ev::per_key_colors(self.variant, &custom.leds), t)?;
            s.transact_all(&ev::effect_table(self.variant, &custom.per_led_effect), t)?;
        } else {
            s.transact(&ev::lighting_packet(l)?, t)?;
        }
        if save {
            s.transact(&ev::save(l.menu_index.value()), timeout::SAVE)?;
        }
        Ok(())
    }

    /// Write a key's binding.
    ///
    /// TODO(spec): the spec does not say whether bindings need a Save (`13 55`) to
    /// persist; none is sent, to spare the flash.
    pub fn apply_key(&self, src: u16, op: &KeyOp) -> Result<()> {
        let s = self.link.session()?;
        let t = timeout::DEFAULT;
        match op {
            KeyOp::Remap(dst) => s.transact(&ev::remap(src, *dst), t).map(|_| ()),
            KeyOp::Shortcut { key, mods } => s.transact(&ev::shortcut_remap(src, *key as u8, *mods), t).map(|_| ()),
            KeyOp::Macro { playback, events } => s.transact_all(&ev::macro_packets(src as u8, *playback, events)?, t),
            KeyOp::Launch(payload) => s.transact_all(&ev::launch_packets(src as u8, payload), t),
            KeyOp::None => Ok(()),
        }
    }

    pub fn get_toggle(&self, code: u8) -> Result<u8> {
        Ok(self.cmd(&ev::toggle_get(code))?[4])
    }

    pub fn ext_info(&self) -> Result<ExtendInfo> {
        Ok(ev::parse_ext_info(&self.cmd(&ev::ext_info_request())?))
    }

    pub fn set_ext_info(&self, e: &ExtendInfo) -> Result<()> {
        let p = ev::ext_info_write(self.variant, e)?;
        self.cmd(&p).map(|_| ())
    }

    /// Clock sync: AP enable, read the format, send the time with that format unchanged
    /// (or with `format` when given).
    pub fn clock_sync(&self, now: ClockTime, format: Option<(bool, bool)>) -> Result<()> {
        let s = self.link.session()?;
        s.transact(&ev::ap_enable(true), timeout::DEFAULT)?;
        let fmt = match format {
            Some(f) => f,
            None => ev::parse_clock_format(&s.transact(&ev::clock_format_request(), timeout::DEFAULT)?),
        };
        s.transact(&ev::clock(now, fmt.0, fmt.1), timeout::DEFAULT)?;
        Ok(())
    }

    /// Timer push (volume / PC info): skipped when another command is in flight (C-3).
    pub fn push(&self, p: &Packet) -> Result<bool> {
        match self.link.try_session() {
            Some(s) => s.transact(p, timeout::DEFAULT).map(|_| true),
            None => Ok(false),
        }
    }

    /// Read a key's stored content. `None` when the key has no content.
    pub fn key_content(&self, profile: u8, key: u8) -> Result<Option<KeyContent>> {
        let (p, form) = ev::key_content_request(profile, key, 2);
        match self.link.multipart(&p, form, timeout::KEY_CONTENT) {
            Ok(b) => Ok(ev::parse_key_content(&b)),
            Err(Error::Nak(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

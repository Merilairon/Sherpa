//! Everest Max and MacroPad driver (spec 04, 08).

use std::sync::Arc;

use sherpa_core::binding::{KeyOp, REMAP_DEFAULT};
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
        self.switch_profile_raw(slot, menu.value())
    }

    /// Switch with a raw menu slot (a menu slot may hold another effect than its usual one).
    pub fn switch_profile_raw(&self, slot: u8, menu: u8) -> Result<()> {
        self.cmd(&ev::switch_profile(slot, menu)).map(|_| ())
    }

    /// Effect menu table: effects per menu slot and current menu per profile.
    pub fn menu_table(&self) -> Result<ev::MenuTable> {
        let (p, form) = ev::effect_menu_table_request();
        let data = self.link.multipart(&p, form, timeout::DEFAULT)?;
        ev::MenuTable::parse(&data).ok_or_else(|| Error::Invalid("short effect menu table".into()))
    }

    /// Stored effect data of one menu slot (§5.1 layout, from b2).
    pub fn slot_effect(&self, profile: u8, menu: u8) -> Result<Vec<u8>> {
        Ok(self.cmd(&ev::effect_data_request(profile, menu))?[6..].to_vec())
    }

    /// Stored per-key colours and per-LED effect table of a profile.
    pub fn custom_tables(&self, profile: u8) -> Result<(Vec<u8>, Vec<u8>)> {
        let n = self.variant.led_count();
        let (p, form) = ev::per_key_colors_request(profile);
        let mut colors = self.link.multipart(&p, form, timeout::DEFAULT)?;
        let (p, form) = ev::effect_table_request(profile);
        let mut table = self.link.multipart(&p, form, timeout::DEFAULT)?;
        if colors.len() < 3 * n || table.len() < n {
            return Err(Error::Invalid("short per-key tables".into()));
        }
        colors.truncate(3 * n);
        table.truncate(n);
        Ok((colors, table))
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

    /// Write a key's binding. No Save is needed: bindings survive a power cycle
    /// (hardware, fw 0x57).
    pub fn apply_key(&self, src: u16, op: &KeyOp) -> Result<()> {
        let s = self.link.session()?;
        let t = timeout::DEFAULT;
        // A remap stays in effect under macro and launch content, so the key would send
        // (and report) the remap target. Reset it first (hardware, fw 0x57).
        if matches!(op, KeyOp::Macro { .. } | KeyOp::Launch(_)) {
            s.transact(&ev::remap(src, REMAP_DEFAULT), t)?;
        }
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

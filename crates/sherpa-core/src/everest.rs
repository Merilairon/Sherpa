//! Everest Max protocol (spec 04), also used by the MacroPad (spec 08) and for the shared
//! commands of the DisplayPad (spec 09 §3.1). All builders return 64-byte payloads.

use crate::launch;
use crate::model::{DockSettings, Lighting, LED_COUNT};
use crate::multipart::Form;
use crate::types::{effect, ColorType, MenuIndex, Rgb};
use crate::{err, packet, Packet, ProtoResult};

/// Packet layout variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    EverestMax,
    /// 12-key pad: single-chunk colour/effect tables, 16-byte extended info.
    MacroPad,
}

impl Variant {
    pub fn led_count(self) -> usize {
        match self {
            Variant::EverestMax => LED_COUNT,
            Variant::MacroPad => 12,
        }
    }
}

/// Default reply timeout and per-command overrides (ms).
pub mod timeout {
    pub const DEFAULT: u64 = 600;
    pub const SYNC_EFFECT: u64 = 1000;
    pub const EXT_INFO: u64 = 2800;
    pub const KEY_CONTENT: u64 = 1000;
    pub const SWITCH_PROFILE: u64 = 950;
    pub const SAVE: u64 = 1300;
    pub const RESET_FLASH: u64 = 12000;
    pub const RESET_PICTURES: u64 = 5000;
    pub const RESET_KEYS: u64 = 3000;
    pub const RESET_EFFECTS: u64 = 3000;
}

/// True for an input report that is a NAK (`FF AA`).
pub fn is_nak(p: &[u8]) -> bool {
    p.len() >= 2 && p[0] == 0xFF && p[1] == 0xAA
}

/// True for an unsolicited event report (`01`..`0A`).
pub fn is_event(p: &[u8]) -> bool {
    !p.is_empty() && (0x01..=0x0A).contains(&p[0])
}

/// Does `reply` acknowledge `request` (bytes 0–3 equal)?
pub fn matches(request: &[u8], reply: &[u8]) -> bool {
    request.len() >= 4 && reply.len() >= 4 && request[..4] == reply[..4]
}

// ---------------------------------------------------------------------------------------
// Identity and status (§4)

/// Feature report `AA 55 80`: device info.
pub fn device_info_request() -> Packet {
    packet(&[0xAA, 0x55, 0x80])
}

/// Feature report `AA 55 21 <dev>`: sub-device (dock/numpad) firmware version.
pub fn sub_device_request(dev: u8) -> Packet {
    packet(&[0xAA, 0x55, 0x21, dev])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub fw_version: u16,
    pub bootloader_version: u16,
}

/// Parse the `AA 55 80` reply (offsets inferred, spec 04 §4.1).
pub fn parse_device_info(p: &[u8]) -> DeviceInfo {
    let u = |i: usize| if p.len() > i + 1 { u16::from_le_bytes([p[i], p[i + 1]]) } else { 0 };
    DeviceInfo { vid: u(4), pid: u(6), fw_version: u(8), bootloader_version: u(10) }
}

pub fn fw_info_request() -> Packet {
    packet(&[0x11, 0x00])
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FwInfo {
    pub fw_version: u16,
    /// Hex digits of the version, e.g. `0x0105` → "105".
    pub fw_string: String,
    pub profile_count: u8,
    pub effect_mode: u8,
    /// 1-based current profile.
    pub current_profile: u8,
    pub menu_index: u8,
}

pub fn parse_fw_info(p: &Packet) -> FwInfo {
    let v = u16::from_le_bytes([p[4], p[5]]);
    FwInfo {
        fw_version: v,
        fw_string: format!("{v:x}"),
        profile_count: p[8].saturating_sub(1),
        effect_mode: p[9],
        current_profile: p[10],
        menu_index: p[11],
    }
}

pub fn layout_request() -> Packet {
    packet(&[0x11, 0x12])
}

pub fn parse_layout(p: &Packet) -> u8 {
    p[4]
}

/// Getters of `12 0x 00 00` form: game mode (06), indicator (07), sync across profiles
/// (08), game-mode status (09), YETI (0A). The value is at b4.
pub fn toggle_get(code: u8) -> Packet {
    packet(&[0x12, code, 0x00, 0x00])
}

pub mod toggle {
    pub const GAME_MODE: u8 = 0x06;
    pub const INDICATOR: u8 = 0x07;
    pub const SYNC_PROFILES: u8 = 0x08;
    pub const GAME_MODE_STATUS: u8 = 0x09;
    pub const YETI: u8 = 0x0A;
}

/// Setter `12 <code> 00 01 <v>`.
pub fn toggle_set(code: u8, v: u8) -> Packet {
    packet(&[0x12, code, 0x00, 0x01, v])
}

/// `12 02` / `12 03`: main lighting off / on.
pub fn main_lighting(on: bool) -> Packet {
    packet(&[0x12, if on { 0x03 } else { 0x02 }])
}

/// `12 04` / `12 05`: key functions off / on.
pub fn key_functions(on: bool) -> Packet {
    packet(&[0x12, if on { 0x05 } else { 0x04 }])
}

/// `12 <0/1> 00 00 <delayMs>`: sync effect (live streaming) off / on. Timeout 1000 ms.
pub fn sync_effect(on: bool, delay_ms: u8) -> Packet {
    packet(&[0x12, on as u8, 0x00, 0x00, delay_ms])
}

/// `11 80 00 00 <0/1>`: AP enable.
pub fn ap_enable(on: bool) -> Packet {
    packet(&[0x11, 0x80, 0x00, 0x00, on as u8])
}

// ---------------------------------------------------------------------------------------
// Lighting (§5)

/// Speed remapping of §5.2.
pub fn map_speed(effect_index: u8, ui: u8) -> u8 {
    let bucket = match ui {
        0..=12 => 0,
        13..=37 => 1,
        38..=62 => 2,
        63..=87 => 3,
        _ => 4,
    };
    let table: &[u8; 5] = match effect_index {
        effect::BREATHING | effect::REACTIVE | effect::REACTIVE_B | effect::REACTIVE_C => &[5, 4, 3, 1, 0],
        effect::COLOR_WAVE | effect::TORNADO => &[10, 9, 8, 7, 6],
        effect::MATRIX => &[20, 15, 10, 5, 0],
        effect::YETI => &[10, 7, 5, 3, 0],
        _ => return ui,
    };
    table[bucket]
}

fn has_background(effect_index: u8) -> bool {
    matches!(effect_index, effect::REACTIVE | effect::REACTIVE_B | effect::REACTIVE_C | effect::MATRIX | effect::YETI)
}

/// Device `randColor` byte for a simple effect (§5.1 byte 6, 03 ColorType).
pub fn rand_color(effect_index: u8, ct: ColorType) -> u8 {
    if effect_index == effect::OFF {
        return 0xFF;
    }
    let v = match ct {
        ColorType::Single => 0,
        ColorType::Dual => {
            if has_background(effect_index) {
                0
            } else {
                16
            }
        }
        ColorType::Random => 2,
    };
    match effect_index {
        effect::STATIC if v != 2 => 0,
        effect::YETI => 0,
        _ => v,
    }
}

/// Effect ids sent with the simple EffectData form.
pub fn is_simple_effect(e: u8) -> bool {
    matches!(e, 0 | 1 | 3 | 6 | 9 | 11 | 12)
}

/// `14 2C` + EffectData (§5.1).
pub fn simple_effect(effect_index: u8, l: &Lighting) -> ProtoResult<Packet> {
    if !is_simple_effect(effect_index) {
        return err(format!("effect {effect_index} is not a simple effect"));
    }
    let mut p = packet(&[0x14, 0x2C]);
    let rand = rand_color(effect_index, l.color_type);
    p[2] = effect_index;
    // applyAll is 0 (hardware, fw 0x57): with 1, Static (`14 2C 00 01`) is parsed as a
    // per-key colour chunk and the other effects are ignored or switch to Custom.
    p[3] = 0;
    p[4] = if effect_index == effect::STATIC { 0xFF } else { map_speed(effect_index, l.speed) };
    p[5] = l.brightness.min(100);
    p[6] = rand;
    p[7] = 0xFF;
    p[8] = 0xFF;
    if effect_index != effect::OFF {
        p[9..12].copy_from_slice(&l.color1.bytes());
        if rand == 16 {
            p[12..15].copy_from_slice(&l.color2.bytes());
        }
        if has_background(effect_index) {
            // TODO(spec): the UI's "colour 3" is written to the background field
            // (bytes 18–20); the colour-3 slot (15–17) is left 0.
            p[18..21].copy_from_slice(&l.color3.bytes());
        }
    }
    Ok(p)
}

/// BlockData for Color Wave / Tornado (§5.3), with the library's validation and rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockData {
    pub effect_index: u8,
    pub apply_all: u8,
    pub speed: u8,
    pub brightness: u8,
    pub rand_color: u8,
    pub direction: u8,
    pub width: u8,
    pub stop_count: u8,
    /// `{pos, r, g, b}`; `pos = FF` is unused.
    pub stops: [[u8; 4]; 7],
    pub background: Rgb,
}

impl BlockData {
    pub fn from_lighting(effect_index: u8, l: &Lighting) -> BlockData {
        let mut stops = [[0xFF, 0, 0, 0]; 7];
        let c1 = l.color1.bytes();
        let c2 = l.color2.bytes();
        stops[0] = [0xFF, c1[0], c1[1], c1[2]];
        stops[1] = [0xFF, c2[0], c2[1], c2[2]];
        BlockData {
            effect_index,
            // 0 like the simple effects: with 1 the keyboard ignores the block (fw 0x57).
            apply_all: 0,
            speed: l.speed,
            brightness: l.brightness.min(100),
            rand_color: match l.color_type {
                ColorType::Single => 0,
                ColorType::Dual => 16,
                ColorType::Random => 2,
            },
            direction: l.direction,
            width: 0xFF,
            stop_count: 0,
            stops,
            background: Rgb::BLACK,
        }
    }

    /// Validate and rewrite as the library does, then encode `14 2C` + BlockData.
    pub fn encode(mut self) -> ProtoResult<Packet> {
        match self.effect_index {
            effect::COLOR_WAVE => {
                self.stops[0][0] = 100;
                self.stops[1][0] = 0xFF;
                if ![0, 2, 4, 6].contains(&self.direction) {
                    return err(format!("Color Wave direction {} is invalid (0, 2, 4 or 6)", self.direction));
                }
            }
            effect::REACTIVE_B => {
                self.stops[0][0] = 0xFF;
                self.stops[1][0] = 0xFF;
                self.direction = 0xFF;
            }
            effect::TORNADO => {
                self.stops[0][0] = 100;
                self.stops[1][0] = 0xFF;
                if ![9, 10].contains(&self.direction) {
                    return err(format!("Tornado direction {} is invalid (9 or 10)", self.direction));
                }
            }
            e => return err(format!("effect {e} cannot be sent as a block effect")),
        }
        if self.rand_color == 2 {
            self.stops[0][0] = 0xFF;
            self.stop_count = 0;
            self.width = 2;
        } else if self.effect_index == effect::COLOR_WAVE {
            let dual = self.rand_color == 16;
            self.rand_color = 0;
            if dual {
                let c1 = [self.stops[0][1], self.stops[0][2], self.stops[0][3]];
                let c2 = [self.stops[1][1], self.stops[1][2], self.stops[1][3]];
                for (i, (pos, c)) in [(25u8, c1), (50, c2), (75, c1), (100, c2)].into_iter().enumerate() {
                    self.stops[i] = [pos, c[0], c[1], c[2]];
                }
                self.stop_count = 4;
                self.width = 2;
            } else {
                self.stop_count = 1;
                self.width = 0;
            }
        } else if self.effect_index == effect::TORNADO {
            self.rand_color = 0;
            self.width = 0;
            self.stop_count = 1;
        } else {
            self.width = 2;
        }
        self.speed = map_speed(self.effect_index, self.speed);

        let mut p = packet(&[0x14, 0x2C]);
        p[2] = self.effect_index;
        p[3] = self.apply_all;
        p[4] = self.speed;
        p[5] = self.brightness;
        p[6] = self.rand_color;
        p[7] = self.direction;
        p[8] = self.width;
        p[9] = self.stop_count;
        for (i, s) in self.stops.iter().enumerate() {
            p[10 + 4 * i..14 + 4 * i].copy_from_slice(s);
        }
        p[38..41].copy_from_slice(&self.background.bytes());
        Ok(p)
    }
}

/// Packet(s) that apply a non-Custom lighting entry (without the Save).
pub fn lighting_packet(l: &Lighting) -> ProtoResult<Packet> {
    let e = l.menu_index.effect_index();
    match l.menu_index {
        MenuIndex::ColorWave | MenuIndex::Tornado => BlockData::from_lighting(e, l).encode(),
        MenuIndex::Custom => Ok(custom_mode(l.brightness)),
        _ => simple_effect(e, l),
    }
}

/// `14 2C 0A 00 FF <brightness>` with bytes 6–63 = `FF` (§5.4).
pub fn custom_mode(brightness: u8) -> Packet {
    let mut p = [0xFFu8; 64];
    p[..6].copy_from_slice(&[0x14, 0x2C, 0x0A, 0x00, 0xFF, brightness.min(100)]);
    p
}

/// Per-key static colours (§5.5; MacroPad: one chunk, 08 §3).
pub fn per_key_colors(variant: Variant, leds: &[Rgb]) -> Vec<Packet> {
    let n = variant.led_count();
    let mut bytes = vec![0u8; n * 3];
    for (i, c) in leds.iter().take(n).enumerate() {
        bytes[3 * i..3 * i + 3].copy_from_slice(&c.bytes());
    }
    bytes
        .chunks(57)
        .enumerate()
        .map(|(i, chunk)| {
            let mut p = packet(&[0x14, 0x2C, 0x00, 0x01, i as u8, 0x4B, 0x00]);
            p[7..7 + chunk.len()].copy_from_slice(chunk);
            p
        })
        .collect()
}

/// Per-LED effect table (§5.6; MacroPad: one chunk).
pub fn effect_table(variant: Variant, effects: &[u8]) -> Vec<Packet> {
    let n = variant.led_count();
    let mut bytes = vec![0u8; n];
    for (i, e) in effects.iter().take(n).enumerate() {
        bytes[i] = *e;
    }
    bytes
        .chunks(60)
        .enumerate()
        .map(|(i, chunk)| {
            let mut p = packet(&[0x14, 0xA0, i as u8, 0x01]);
            p[4..4 + chunk.len()].copy_from_slice(chunk);
            p
        })
        .collect()
}

/// `ChangeCustomizeEffect` chunks (§5.8). Entries are `{led, r, g, b}`.
pub fn customize_effect(entries: &[[u8; 4]], preview: bool, brightness: u8, variant: Variant) -> ProtoResult<Vec<Packet>> {
    let max = match variant {
        Variant::EverestMax => 171,
        Variant::MacroPad => 12,
    };
    if entries.len() > max {
        return err(format!("at most {max} per-key effect entries"));
    }
    let total = entries.len();
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let end = (offset + 14).min(total);
        let seq = if total < 15 {
            0
        } else if offset == 0 {
            1
        } else if total <= offset + 14 {
            3
        } else {
            2
        };
        let mut p = [0xFFu8; 64];
        p[..8].copy_from_slice(&[0x14, 0x2C, 0x0A, 0x00, if preview { 0xFF } else { 0x00 }, brightness.min(100), 0x00, seq]);
        for (i, e) in entries[offset..end].iter().enumerate() {
            p[8 + 4 * i..12 + 4 * i].copy_from_slice(e);
        }
        out.push(p);
        offset = end;
        if offset >= total {
            break;
        }
    }
    Ok(out)
}

/// Lighting getters (§5.7) and the multi-part form of their reply.
pub fn effect_menu_table_request() -> (Packet, Form) {
    (packet(&[0x11, 0x01, 0x00, 0x00, 0x02]), Form::A)
}

pub fn effect_data_request(profile: u8, menu: u8) -> Packet {
    packet(&[0x11, 0x01, 0x00, 0x01, profile, menu])
}

pub fn per_key_colors_request(profile: u8) -> (Packet, Form) {
    (packet(&[0x11, 0x01, 0x00, 0x02, profile, 0x02]), Form::B)
}

pub fn effect_table_request(profile: u8) -> (Packet, Form) {
    (packet(&[0x11, 0x01, 0x00, 0x03, profile, 0x02]), Form::B)
}

// ---------------------------------------------------------------------------------------
// Device settings (§6)

/// Dock PC-info slots.
pub mod pc_slot {
    pub const CPU: u16 = 0;
    pub const GPU: u16 = 1;
    pub const DISK: u16 = 2;
    pub const NETWORK: u16 = 3;
    pub const RAM: u16 = 4;
    pub const APM: u16 = 5;
}

/// `11 81 <slot u16> <value u16>`, value clamped to 0..100 (APM: 0..999).
pub fn pc_info(slot: u16, value: u32) -> Packet {
    let max = if slot == pc_slot::APM { 999 } else { 100 };
    let v = value.min(max) as u16;
    let mut p = packet(&[0x11, 0x81]);
    p[2..4].copy_from_slice(&slot.to_le_bytes());
    p[4..6].copy_from_slice(&v.to_le_bytes());
    p
}

/// `11 83 00 00 <volume>`.
pub fn volume(v: u8) -> Packet {
    packet(&[0x11, 0x83, 0x00, 0x00, v.min(100)])
}

pub fn clock_format_request() -> Packet {
    packet(&[0x11, 0x84, 0x00, 0x00])
}

/// `(digital, 12-hour)` from the `11 84 00 00` reply.
pub fn parse_clock_format(p: &Packet) -> (bool, bool) {
    (p[11] != 0, p[12] != 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockTime {
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// `11 84 00 01 00 00 <month> <day> <hour> <minute> <second> <digital> <12h>`.
pub fn clock(t: ClockTime, digital: bool, h12: bool) -> Packet {
    packet(&[0x11, 0x84, 0x00, 0x01, 0x00, 0x00, t.month, t.day, t.hour, t.minute, t.second, digital as u8, h12 as u8])
}

/// `1C 00 00 00 <p1..p4>`: numpad display-key picture selection.
pub fn display_key_pictures(p: [u8; 4]) -> Packet {
    packet(&[0x1C, 0x00, 0x00, 0x00, p[0], p[1], p[2], p[3]])
}

pub fn display_key_pictures_request() -> Packet {
    packet(&[0x1C, 0x00, 0x00, 0x01])
}

/// Extended info (§6.4).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendInfo {
    pub dock_plug: u8,
    pub show_menu: u8,
    pub menu_index: u8,
    pub menu_color: [u8; 3],
    pub screen_setup: u8,
    pub screen_saver: u16,
    pub turn_off: u16,
    pub show_profile: [u8; 5],
    pub numpad_plug: u8,
    pub pixel_shift_time: u8,
    pub dock_double_click: u8,
    /// `{dock, numpad}` × 5 profiles, raw device bytes (`0x80 | level`).
    pub brightness: [u8; 10],
}

/// Quantise a brightness to {0, 25, 50, 75, 100}.
pub fn quantize_brightness(v: u8) -> u8 {
    let v = v.min(100) as u16;
    (((v + 12) / 25) * 25).min(100) as u8
}

impl ExtendInfo {
    /// Build the value to write from dock settings.
    pub fn from_dock(d: &DockSettings) -> ExtendInfo {
        let mut brightness = [0u8; 10];
        for i in 0..5 {
            brightness[2 * i] = 0x80 | quantize_brightness(d.dock_brightness[i]);
            brightness[2 * i + 1] = 0x80 | quantize_brightness(d.numpad_brightness[i]);
        }
        // TODO(spec): the bit positions of the screen-saver type in `screenSetup` are unknown;
        // only the two enable bits are written.
        let screen_setup = (d.screen_saver_enabled as u8) | ((d.turn_off_enabled as u8) << 1);
        ExtendInfo {
            dock_plug: 2,
            show_menu: d.pages_enabled,
            menu_index: 0,
            menu_color: d.menu_color.bytes(),
            screen_setup,
            screen_saver: d.screen_saver_sec,
            turn_off: d.turn_off_sec,
            show_profile: d.profiles_shown,
            numpad_plug: 2,
            pixel_shift_time: d.pixel_shift_time,
            dock_double_click: d.double_click_ms,
            brightness,
        }
    }

    /// Host-side validation as done by the vendor library.
    pub fn validate(&self) -> ProtoResult<()> {
        if self.screen_saver > 1800 {
            return err("screen saver timeout must be ≤ 1800");
        }
        if self.turn_off > 1800 {
            return err("turn-off timeout must be ≤ 1800");
        }
        if self.dock_double_click > 200 {
            return err("dock double-click time must be ≤ 200");
        }
        validate_show_profile(&self.show_profile)
    }

    pub fn to_bytes(&self) -> [u8; 29] {
        let mut b = [0u8; 29];
        b[0] = self.dock_plug;
        b[1] = self.show_menu;
        b[2] = self.menu_index;
        b[3..6].copy_from_slice(&self.menu_color);
        b[6] = self.screen_setup;
        b[7..9].copy_from_slice(&self.screen_saver.to_le_bytes());
        b[9..11].copy_from_slice(&self.turn_off.to_le_bytes());
        b[11..16].copy_from_slice(&self.show_profile);
        b[16] = self.numpad_plug;
        b[17] = self.pixel_shift_time;
        b[18] = self.dock_double_click;
        // Raw: `0x80 | level` sets a level, a byte without `0x80` keeps the current one
        // (hardware, fw 0x57). `from_dock` adds the flag.
        b[19..29].copy_from_slice(&self.brightness);
        b
    }

    /// The value to write back after a read: same settings, with the write rules of §6.4
    /// (dockPlug 2, menuIndex 0, numpadPlug 2). Brightness bytes read back as 0, i.e.
    /// without `0x80`, so the device keeps its levels.
    pub fn rewrite(&self) -> ExtendInfo {
        ExtendInfo { dock_plug: 2, menu_index: 0, numpad_plug: 2, ..self.clone() }
    }

    /// Is firmware profile `slot` enabled? A profile missing from `showProfile` counts as
    /// enabled.
    pub fn profile_enabled(&self, slot: u8) -> bool {
        self.show_profile.iter().find(|&&v| v & 0x0F == slot).is_none_or(|v| v & PROFILE_DISABLED == 0)
    }

    pub fn disabled_profiles(&self) -> Vec<u8> {
        (1..=5).filter(|&s| !self.profile_enabled(s)).collect()
    }

    /// Set or clear the disabled flag of profile `slot`. Returns false when the profile is
    /// not in `showProfile`.
    pub fn set_profile_enabled(&mut self, slot: u8, enabled: bool) -> bool {
        match self.show_profile.iter_mut().find(|v| **v & 0x0F == slot) {
            Some(v) => {
                *v = if enabled { *v & !PROFILE_DISABLED } else { *v | PROFILE_DISABLED };
                true
            }
            None => false,
        }
    }

    /// Copy the disabled flags of `device` onto this value's `showProfile` entries.
    pub fn keep_profile_flags(&mut self, device: &ExtendInfo) {
        for v in &mut self.show_profile {
            let off = !device.profile_enabled(*v & 0x0F);
            *v = if off { *v | PROFILE_DISABLED } else { *v & !PROFILE_DISABLED };
        }
    }

    pub fn from_bytes(b: &[u8]) -> ExtendInfo {
        let mut x = [0u8; 29];
        let n = b.len().min(29);
        x[..n].copy_from_slice(&b[..n]);
        let mut e = ExtendInfo {
            dock_plug: x[0],
            show_menu: x[1],
            menu_index: x[2],
            menu_color: [x[3], x[4], x[5]],
            screen_setup: x[6],
            screen_saver: u16::from_le_bytes([x[7], x[8]]),
            turn_off: u16::from_le_bytes([x[9], x[10]]),
            show_profile: [0; 5],
            numpad_plug: x[16],
            pixel_shift_time: x[17],
            dock_double_click: x[18],
            brightness: [0; 10],
        };
        e.show_profile.copy_from_slice(&x[11..16]);
        e.brightness.copy_from_slice(&x[19..29]);
        e
    }

    /// Dock side from `dockPlug`: bit0 right, bit1 left.
    pub fn dock_side(&self) -> &'static str {
        side(self.dock_plug)
    }

    pub fn numpad_side(&self) -> &'static str {
        side(self.numpad_plug)
    }
}

fn side(v: u8) -> &'static str {
    if v & 1 != 0 {
        "right"
    } else if v & 2 != 0 {
        "left"
    } else {
        "none"
    }
}

/// `showProfile` flag: the profile is disabled on the keyboard. Switching to it is ACKed
/// but nothing is loaded (FW info reports effect and menu `FF`), and writes are dropped
/// (hardware, fw 0x57).
pub const PROFILE_DISABLED: u8 = 0x10;

/// Each `showProfile` entry must be non-zero with a distinct low nibble.
pub fn validate_show_profile(sp: &[u8; 5]) -> ProtoResult<()> {
    let mut seen = 0u16;
    for &v in sp {
        if v == 0 {
            return err("showProfile entries must be non-zero");
        }
        let bit = 1u16 << (v & 0x0F);
        if seen & bit != 0 {
            return err("showProfile entries must have distinct low nibbles");
        }
        seen |= bit;
    }
    Ok(())
}

/// `11 14 00 01` + ExtendInfo (Everest Max: 29 bytes; MacroPad: 11 unused bytes then
/// `showProfile[5]`). Validates first.
pub fn ext_info_write(variant: Variant, e: &ExtendInfo) -> ProtoResult<Packet> {
    let mut p = packet(&[0x11, 0x14, 0x00, 0x01]);
    match variant {
        Variant::EverestMax => {
            e.validate()?;
            p[4..33].copy_from_slice(&e.to_bytes());
        }
        Variant::MacroPad => {
            validate_show_profile(&e.show_profile)?;
            p[15..20].copy_from_slice(&e.show_profile);
        }
    }
    Ok(p)
}

pub fn ext_info_request() -> Packet {
    packet(&[0x11, 0x14, 0x00, 0x00])
}

/// Parse the `11 14 00 00` reply (ExtendInfo at b4..b32).
pub fn parse_ext_info(p: &Packet) -> ExtendInfo {
    ExtendInfo::from_bytes(&p[4..33])
}

/// Dock menu index values (§6.5).
pub mod dock_menu {
    pub const VOLUME: u8 = 65;
    pub const CPU: u8 = 97;
    pub const GPU: u8 = 98;
    pub const DISK: u8 = 99;
    pub const NETWORK: u8 = 100;
    pub const RAM: u8 = 101;
    pub const APM: u8 = 113;
    pub const MEDIA_PASSTHROUGH: u8 = 130;
}

// ---------------------------------------------------------------------------------------
// Keys (§7)

/// `14 20 <src u16> <dst u16>`.
pub fn remap(src: u16, dst: u16) -> Packet {
    let mut p = packet(&[0x14, 0x20]);
    p[2..4].copy_from_slice(&src.to_le_bytes());
    p[4..6].copy_from_slice(&dst.to_le_bytes());
    p
}

/// `14 21 <src u16> <dstKey> <mods>`.
pub fn shortcut_remap(src: u16, key: u8, mods: u8) -> Packet {
    let mut p = packet(&[0x14, 0x21]);
    p[2..4].copy_from_slice(&src.to_le_bytes());
    p[4] = key;
    p[5] = mods;
    p
}

/// Macro write: header `14 18 <playback> 00 01`, then `15 <src> <count> <0|1> <events>`.
/// `events` must already include the terminator.
pub fn macro_packets(src: u8, playback: u8, events: &[[u8; 4]]) -> ProtoResult<Vec<Packet>> {
    if events.last() != Some(&crate::macros::TERMINATOR) {
        return err("macro events must end with the terminator");
    }
    if events.len() > crate::macros::MAX_EVENTS {
        return err("too many macro events");
    }
    let mut out = vec![packet(&[0x14, 0x18, playback, 0x00, 0x01])];
    for (i, chunk) in events.chunks(15).enumerate() {
        let mut p = packet(&[0x15, src, chunk.len() as u8, (i > 0) as u8]);
        for (j, e) in chunk.iter().enumerate() {
            p[4 + 4 * j..8 + 4 * j].copy_from_slice(e);
        }
        out.push(p);
    }
    Ok(out)
}

/// Launch content write chunks `17 <src> <len> <seq> <data>`.
pub fn launch_packets(src: u8, payload: &[u8]) -> Vec<Packet> {
    launch::chunks(payload)
        .into_iter()
        .map(|c| {
            let mut p = packet(&[0x17, src, c.data.len() as u8, c.seq]);
            p[4..4 + c.data.len()].copy_from_slice(c.data);
            p
        })
        .collect()
}

/// `11 02 00 01 <profile> <key> <interval>` (form B reply, timeout 1000 ms).
pub fn key_content_request(profile: u8, key: u8, interval: u8) -> (Packet, Form) {
    (packet(&[0x11, 0x02, 0x00, 0x01, profile, key, interval]), Form::B)
}

/// A key's stored content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyContent {
    pub size: u16,
    pub key_type: u8,
    pub idx1: u8,
    pub idx2: u8,
    pub data: Vec<u8>,
}

pub const KEY_TYPE_LAUNCH: u8 = 7;

/// Parse a reassembled key-content reply. The data is `size` bytes long; the reply is
/// padded with `FF` after it (hardware, fw 0x57), so it is cut at `size`.
pub fn parse_key_content(b: &[u8]) -> Option<KeyContent> {
    if b.len() < 5 {
        return None;
    }
    let size = u16::from_le_bytes([b[0], b[1]]);
    let end = (5 + size as usize).min(b.len());
    Some(KeyContent { size, key_type: b[2], idx1: b[3], idx2: b[4], data: b[5..end].to_vec() })
}

// ---------------------------------------------------------------------------------------
// Profiles and persistence (§8)

/// `14 00 00 00 <slot> <menuIndex>` (timeout 950 ms).
pub fn switch_profile(slot: u8, menu: u8) -> Packet {
    packet(&[0x14, 0x00, 0x00, 0x00, slot, menu])
}

/// `13 55 00 00 <menuIndex>` (timeout 1300 ms).
pub fn save(menu: u8) -> Packet {
    packet(&[0x13, 0x55, 0x00, 0x00, menu])
}

/// `13 40 00 00 <all>` (timeout 12000 ms).
pub fn reset_flash(all: bool) -> Packet {
    packet(&[0x13, 0x40, 0x00, 0x00, all as u8])
}

pub fn reset_dock_pictures(profile_mask: u8) -> Packet {
    packet(&[0x13, 0x41, 0x00, 0x00, profile_mask])
}

pub fn reset_numpad_pictures(p: [u8; 5]) -> Packet {
    packet(&[0x13, 0x42, 0x00, 0x00, p[0], p[1], p[2], p[3], p[4]])
}

/// `13 60`: reset all key bindings (timeout 3000 ms).
pub fn reset_keys() -> Packet {
    packet(&[0x13, 0x60])
}

/// `13 61`: reset all lighting effects (timeout 3000 ms).
pub fn reset_effects() -> Packet {
    packet(&[0x13, 0x61])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hex, unhex};

    fn assert_pkt(p: &Packet, expect: &str) {
        let e = unhex(expect);
        assert_eq!(&p[..e.len()], &e[..], "prefix mismatch: {}", hex(p));
        assert!(p[e.len()..].iter().all(|&b| b == 0), "tail not zero: {}", hex(p));
    }

    #[test]
    fn golden_g1_to_g12() {
        assert_pkt(&switch_profile(2, 3), "14 00 00 00 02 03");
        assert_pkt(&remap(110, 184), "14 20 6e 00 b8 00");
        assert_pkt(&remap(110, 257), "14 20 6e 00 01 01");
        assert_pkt(&shortcut_remap(110, 39, 8), "14 21 6e 00 27 08");
        assert_pkt(&remap(110, 255), "14 20 6e 00 ff 00");
        let t = ClockTime { month: 10, day: 14, hour: 13, minute: 5, second: 9 };
        assert_pkt(&clock(t, true, false), "11 84 00 01 00 00 0a 0e 0d 05 09 01 00");
        assert_pkt(&pc_info(pc_slot::APM, 321), "11 81 05 00 41 01");
        assert_pkt(&pc_info(pc_slot::CPU, 150), "11 81 00 00 64 00");
        assert_pkt(&volume(37), "11 83 00 00 25");
        assert_pkt(&toggle_set(toggle::GAME_MODE, 1), "12 06 00 01 01");
        assert_pkt(&toggle_set(toggle::INDICATOR, 0), "12 07 00 01 00");
        assert_pkt(&save(6), "13 55 00 00 06");
    }

    #[test]
    fn golden_g13_to_g15() {
        let p = custom_mode(80);
        assert_eq!(&p[..6], &unhex("14 2c 0a 00 ff 50")[..]);
        assert!(p[6..].iter().all(|&b| b == 0xFF));
        assert_eq!(p[6..].len(), 58);

        let mut l = Lighting::new(MenuIndex::Static);
        l.color1 = Rgb(255, 0, 0);
        l.brightness = 100;
        assert_pkt(&lighting_packet(&l).unwrap(), "14 2c 00 00 ff 64 00 ff ff ff 00 00");

        let mut l = Lighting::new(MenuIndex::Breathing);
        l.speed = 50;
        l.color1 = Rgb(0x00, 0xa0, 0xff);
        l.brightness = 100;
        assert_pkt(&lighting_packet(&l).unwrap(), "14 2c 01 00 03 64 00 ff ff 00 a0 ff");
    }

    #[test]
    fn effects_are_sent_with_apply_all_0() {
        for m in MenuIndex::ALL.into_iter().filter(|&m| m != MenuIndex::Custom) {
            for ct in [ColorType::Single, ColorType::Dual, ColorType::Random] {
                let mut l = Lighting::new(m);
                l.color_type = ct;
                let p = lighting_packet(&l).unwrap();
                assert_eq!(p[3], 0, "{m:?} {ct:?}");
                assert_ne!(&p[..4], &[0x14, 0x2C, 0x00, 0x01], "{m:?} {ct:?}: per-key header");
            }
        }
    }

    #[test]
    fn key_content_is_cut_at_size() {
        // Launch content as read back from the keyboard (fw 0x57): FF padding follows.
        let mut b = unhex("14 00 07 7d ff 02 68 74 74 70 73 3a 2f 2f 65 78 61 6d 70 6c 65 2e 63 6f 6d");
        b.extend([0xFF; 30]);
        let c = parse_key_content(&b).unwrap();
        assert_eq!((c.size, c.key_type, c.idx1), (20, KEY_TYPE_LAUNCH, 0x7d));
        assert_eq!(crate::launch::decode(&c.data).unwrap().text, "https://example.com");
        // A key reset after having content: size 0, type FF.
        let c = parse_key_content(&unhex("00 00 ff 00 ff ff ff")).unwrap();
        assert!(c.data.is_empty() && c.key_type == 0xFF);
    }

    #[test]
    fn golden_g16_to_g18() {
        let payload = launch::encode(crate::types::LaunchKind::Url, "https://mountain.gg");
        let pk = launch_packets(112, &payload);
        assert_eq!(pk.len(), 1);
        assert_pkt(&pk[0], "17 70 14 00 02 68 74 74 70 73 3a 2f 2f 6d 6f 75 6e 74 61 69 6e 2e 67 67");
        let p130 = launch_packets(1, &[0x41; 130]);
        let hdr: Vec<_> = p130.iter().map(|p| (p[2], p[3])).collect();
        assert_eq!(hdr, vec![(0x3c, 1), (0x3c, 2), (0x0a, 3)]);
        let p120 = launch_packets(1, &[0x41; 120]);
        let hdr: Vec<_> = p120.iter().map(|p| (p[2], p[3])).collect();
        assert_eq!(hdr, vec![(0x3c, 1), (0x3c, 2)]);
    }

    #[test]
    fn golden_g19_ext_info() {
        let d = DockSettings {
            pages_enabled: 0x09,
            menu_color: Rgb(0xff, 0x80, 0x00),
            screen_saver_enabled: true,
            turn_off_enabled: true,
            screen_saver_sec: 300,
            turn_off_sec: 1800,
            profiles_shown: [1, 2, 3, 4, 5],
            pixel_shift_time: 0,
            double_click_ms: 50,
            dock_brightness: [100; 5],
            numpad_brightness: [50; 5],
            ..Default::default()
        };
        let p = ext_info_write(Variant::EverestMax, &ExtendInfo::from_dock(&d)).unwrap();
        assert_pkt(&p, "11 14 00 01 02 09 00 ff 80 00 03 2c 01 08 07 01 02 03 04 05 02 00 32 e4 b2 e4 b2 e4 b2 e4 b2 e4 b2");
        // round trip
        let back = ExtendInfo::from_bytes(&p[4..33]);
        assert_eq!(back.screen_saver, 300);
        assert_eq!(back.show_profile, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn b10_duplicate_show_profile_rejected() {
        let d = DockSettings { profiles_shown: [1, 2, 3, 4, 0x11], ..Default::default() };
        assert!(ext_info_write(Variant::EverestMax, &ExtendInfo::from_dock(&d)).is_err());
        let d = DockSettings { profiles_shown: [1, 2, 3, 4, 0], ..Default::default() };
        assert!(ext_info_write(Variant::EverestMax, &ExtendInfo::from_dock(&d)).is_err());
    }

    #[test]
    fn disabled_profiles_and_rewrite() {
        // As read from a keyboard with only profile 1 enabled (fw 0x57).
        let read = ExtendInfo::from_bytes(&unhex("01 a9 41 dc 29 bc 70 1e 00 1e 00 01 12 13 14 15 01 00 32 00 00 00 00 00 00 00 00 00 00"));
        assert_eq!(read.disabled_profiles(), vec![2, 3, 4, 5]);
        assert!(read.profile_enabled(1));
        let mut e = read.rewrite();
        assert!(e.set_profile_enabled(2, true));
        assert!(!e.set_profile_enabled(6, true));
        assert_eq!(e.disabled_profiles(), vec![3, 4, 5]);
        let p = ext_info_write(Variant::EverestMax, &e).unwrap();
        // Write rules applied, profile 2 enabled, brightness left at 0 (= keep).
        assert_pkt(&p, "11 14 00 01 02 a9 00 dc 29 bc 70 1e 00 1e 00 01 02 13 14 15 02 00 32 00 00 00 00 00 00 00 00 00 00");

        let mut d = ExtendInfo::from_dock(&DockSettings::default());
        d.keep_profile_flags(&read);
        assert_eq!(d.show_profile, [0x01, 0x12, 0x13, 0x14, 0x15]);
    }

    #[test]
    fn golden_g20_per_key() {
        let leds: Vec<Rgb> = (0..126).map(|i| Rgb(i as u8, i as u8, i as u8)).collect();
        let pk = per_key_colors(Variant::EverestMax, &leds);
        assert_eq!(pk.len(), 7);
        for (k, p) in pk.iter().enumerate() {
            assert_eq!(&p[..7], &[0x14, 0x2c, 0x00, 0x01, k as u8, 0x4b, 0x00]);
        }
        assert_eq!(&pk[1][7..10], &[0x13, 0x13, 0x13]);
        assert!(pk[6][7 + 36..].iter().all(|&b| b == 0));
        assert_eq!(pk[6][7 + 35], 125);
    }

    #[test]
    fn golden_g21_effect_table() {
        let t: Vec<u8> = (0..126).collect();
        let pk = effect_table(Variant::EverestMax, &t);
        assert_eq!(pk.len(), 3);
        for (k, p) in pk.iter().enumerate() {
            assert_eq!(&p[..4], &[0x14, 0xa0, k as u8, 0x01]);
        }
        assert_pkt(&pk[2], "14 a0 02 01 78 79 7a 7b 7c 7d");
    }

    #[test]
    fn golden_g22_macro() {
        let mut ev = vec![[0x24, 0x02, 0x34, 0x12]; 16];
        ev.push(crate::macros::TERMINATOR);
        let pk = macro_packets(0x7b, 1, &ev).unwrap();
        assert_eq!(pk.len(), 3);
        assert_pkt(&pk[0], "14 18 01 00 01");
        let mut expect = unhex("15 7b 0f 00");
        for _ in 0..15 {
            expect.extend(unhex("24 02 34 12"));
        }
        assert_eq!(&pk[1][..], &expect[..]);
        assert_pkt(&pk[2], "15 7b 02 01 24 02 34 12 ff ff 00 00");
    }

    #[test]
    fn r1_r2_parse() {
        let p = crate::packet(&unhex("11 00 00 00 05 01 00 00 06 02 03 04"));
        let f = parse_fw_info(&p);
        assert_eq!(f.fw_string, "105");
        assert_eq!((f.profile_count, f.effect_mode, f.current_profile, f.menu_index), (5, 2, 3, 4));
        let mut c = clock_format_request();
        c[11] = 1;
        c[12] = 0;
        assert_eq!(parse_clock_format(&c), (true, false));
        assert!(is_nak(&unhex("ff aa 00")));
    }

    #[test]
    fn block_effects() {
        let mut l = Lighting::new(MenuIndex::ColorWave);
        l.color_type = ColorType::Dual;
        l.color1 = Rgb(1, 2, 3);
        l.color2 = Rgb(4, 5, 6);
        l.speed = 100;
        l.direction = 2;
        let p = lighting_packet(&l).unwrap();
        assert_eq!(&p[2..10], &[4, 0, 6, 100, 0, 2, 2, 4]);
        assert_eq!(&p[10..26], &[25, 1, 2, 3, 50, 4, 5, 6, 75, 1, 2, 3, 100, 4, 5, 6]);
        l.direction = 3;
        assert!(lighting_packet(&l).is_err());

        let mut t = Lighting::new(MenuIndex::Tornado);
        t.color1 = Rgb(9, 9, 9);
        t.speed = 0;
        let p = lighting_packet(&t).unwrap();
        assert_eq!(&p[2..14], &[7, 0, 10, 100, 0, 9, 0, 1, 100, 9, 9, 9]);
        assert_eq!(p[14], 0xFF, "stop1 unused");

        t.color_type = ColorType::Random;
        let p = lighting_packet(&t).unwrap();
        assert_eq!((p[6], p[8], p[9], p[10]), (2, 2, 0, 0xFF));
    }

    #[test]
    fn simple_effect_colours() {
        let mut l = Lighting::new(MenuIndex::Reactive);
        l.color_type = ColorType::Dual;
        l.color1 = Rgb(1, 1, 1);
        l.color2 = Rgb(2, 2, 2);
        l.color3 = Rgb(3, 3, 3);
        let p = lighting_packet(&l).unwrap();
        assert_eq!(p[6], 0, "reactive with background uses randColor 0");
        assert_eq!(&p[12..15], &[0, 0, 0]);
        assert_eq!(&p[18..21], &[3, 3, 3]);
        let mut b = Lighting::new(MenuIndex::Breathing);
        b.color_type = ColorType::Dual;
        b.color2 = Rgb(2, 2, 2);
        let p = lighting_packet(&b).unwrap();
        assert_eq!((p[6], p[12]), (16, 2));
        let off = lighting_packet(&Lighting::new(MenuIndex::Off)).unwrap();
        assert_eq!((off[2], off[6]), (12, 0xFF));
        let mut y = Lighting::new(MenuIndex::Yeti);
        y.color_type = ColorType::Random;
        assert_eq!(lighting_packet(&y).unwrap()[6], 0);
    }

    #[test]
    fn macropad_vectors() {
        let red = vec![Rgb(255, 0, 0); 12];
        let pk = per_key_colors(Variant::MacroPad, &red);
        assert_eq!(pk.len(), 1);
        let mut e = unhex("14 2c 00 01 00 4b 00");
        for _ in 0..12 {
            e.extend([0xff, 0, 0]);
        }
        assert_pkt(&pk[0], &hex(&e));
        let t = effect_table(Variant::MacroPad, &[0; 12]);
        assert_eq!(t.len(), 1);
        assert_pkt(&t[0], "14 a0 00 01");
        assert_pkt(&remap(0xaa, 0xb8), "14 20 aa 00 b8 00");
        let x = ExtendInfo { show_profile: [1, 2, 3, 4, 5], ..Default::default() };
        let p = ext_info_write(Variant::MacroPad, &x).unwrap();
        assert_pkt(&p, "11 14 00 01 00 00 00 00 00 00 00 00 00 00 00 01 02 03 04 05");
    }

    #[test]
    fn customize_effect_seq() {
        let e = vec![[1, 2, 3, 4]; 30];
        let pk = customize_effect(&e, false, 50, Variant::EverestMax).unwrap();
        assert_eq!(pk.iter().map(|p| p[7]).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(&pk[2][8..12], &[1, 2, 3, 4]);
        assert_eq!(&pk[2][12..16], &[1, 2, 3, 4]);
        assert_eq!(&pk[2][16..20], &[0xFF; 4], "unused slots are FF");
        let one = customize_effect(&e[..3], true, 50, Variant::EverestMax).unwrap();
        assert_eq!((one.len(), one[0][4], one[0][7]), (1, 0xFF, 0));
        let exact = customize_effect(&[[0; 4]; 28], false, 50, Variant::EverestMax).unwrap();
        assert_eq!(exact.iter().map(|p| p[7]).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn speed_and_brightness() {
        assert_eq!(map_speed(effect::MATRIX, 12), 20);
        assert_eq!(map_speed(effect::MATRIX, 88), 0);
        assert_eq!(map_speed(effect::YETI, 70), 3);
        assert_eq!(map_speed(effect::STATIC, 42), 42);
        assert_eq!(quantize_brightness(60), 50);
        assert_eq!(quantize_brightness(63), 75);
        assert_eq!(quantize_brightness(255), 100);
    }
}

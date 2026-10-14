//! Makalu 67 protocol (spec 11). Feature report `A1` commands (64 bytes including the
//! report id) and `A0` bulk records (1041 bytes). Byte numbering counts the id as b0.

use crate::model::{MouseButton, MouseLighting, MouseSensor};
use crate::{err, ProtoResult};

pub const REPORT_CMD: u8 = 0xA1;
pub const REPORT_BULK: u8 = 0xA0;
pub const CMD_LEN: usize = 64;
pub const BULK_LEN: usize = 1041;
pub const ALL_PROFILES: u8 = 6;
pub const MAX_MACRO_EVENTS: usize = 140;

pub type Cmd = [u8; CMD_LEN];

/// A command with its post-write delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub bytes: Cmd,
    pub delay_ms: u64,
}

fn cmd(group: u8, sub: u8, delay_ms: u64) -> Command {
    let mut bytes = [0u8; CMD_LEN];
    bytes[0] = REPORT_CMD;
    bytes[1] = group;
    bytes[2] = sub;
    Command { bytes, delay_ms }
}

fn with_profile(mut c: Command, profile: u8) -> Command {
    c.bytes[5] = profile;
    c
}

pub fn firmware_version() -> Command {
    cmd(0x02, 0x00, 80)
}
pub fn mcu_type() -> Command {
    cmd(0x1A, 0x00, 50)
}
pub fn reset_profile(p: u8) -> Command {
    with_profile(cmd(0x0B, 0x01, 50), p)
}
pub fn sync_profile(p: u8) -> Command {
    with_profile(cmd(0x0B, 0x02, 50), p)
}
pub fn get_current_profile() -> Command {
    cmd(0x0B, 0x03, 60)
}
pub fn set_current_profile(p: u8) -> Command {
    with_profile(cmd(0x0B, 0x04, 100), p)
}
pub fn add_profile(p: u8) -> Command {
    with_profile(cmd(0x0B, 0x05, 50), p)
}
pub fn delete_profile(p: u8) -> Command {
    with_profile(cmd(0x0B, 0x06, 50), p)
}
pub fn profile_count() -> Command {
    cmd(0x0B, 0x09, 60)
}
pub fn copy_profile(src: u8, dst: u8) -> Command {
    let mut c = with_profile(cmd(0x0B, 0x0A, 50), src);
    c.bytes[6] = dst;
    c
}
pub fn factory_reset() -> Command {
    cmd(0x0E, 0x00, 50)
}

/// Polling-rate code: 1 = 1000 Hz, 2 = 500, 4 = 250, 8 = 125.
pub fn polling_code(hz: u16) -> ProtoResult<u8> {
    Ok(match hz {
        1000 => 1,
        500 => 2,
        250 => 4,
        125 => 8,
        _ => return err(format!("unsupported polling rate {hz}")),
    })
}

pub fn polling_rate(p: u8, hz: u16) -> ProtoResult<Command> {
    let mut c = with_profile(cmd(0x0D, 0x01, 50), p);
    c.bytes[6] = polling_code(hz)?;
    Ok(c)
}
pub fn response_time(p: u8, ms: u8) -> Command {
    let mut c = with_profile(cmd(0x0D, 0x02, 50), p);
    c.bytes[6] = ms;
    c
}
pub fn angle_snapping(p: u8, on: bool) -> Command {
    let mut c = with_profile(cmd(0x0D, 0x03, 30), p);
    c.bytes[6] = on as u8;
    c
}
pub fn lift_off(p: u8, high: bool) -> Command {
    let mut c = with_profile(cmd(0x0D, 0x04, 50), p);
    c.bytes[6] = high as u8;
    c
}

fn dpi_block(s: &MouseSensor) -> ProtoResult<[u8; 22]> {
    let n = s.dpi_levels.len();
    if !(1..=5).contains(&n) {
        return err("1..5 DPI levels are required");
    }
    if s.current_level == 0 || s.current_level as usize > n {
        return err("current DPI level out of range");
    }
    let mut b = [0u8; 22];
    b[0] = n as u8;
    b[1] = s.current_level;
    for (i, (x, y)) in s.dpi_levels.iter().enumerate() {
        b[2 + 4 * i..4 + 4 * i].copy_from_slice(&x.to_le_bytes());
        b[4 + 4 * i..6 + 4 * i].copy_from_slice(&y.to_le_bytes());
    }
    Ok(b)
}

/// `0D 0A`: b6 count, b7 current, b8–b15 = 0, b16–b35 = 5 × (X, Y).
pub fn dpi(p: u8, s: &MouseSensor) -> ProtoResult<Command> {
    let b = dpi_block(s)?;
    let mut c = with_profile(cmd(0x0D, 0x0A, 50), p);
    c.bytes[6] = b[0];
    c.bytes[7] = b[1];
    c.bytes[16..36].copy_from_slice(&b[2..22]);
    Ok(c)
}

/// `0D 0F`: the whole sensor block.
pub fn sensor_block(p: u8, s: &MouseSensor) -> ProtoResult<Command> {
    let mut c = with_profile(cmd(0x0D, 0x0F, 50), p);
    c.bytes[16] = polling_code(s.polling_hz)?;
    c.bytes[17] = s.response_ms;
    c.bytes[18] = s.angle_snapping as u8;
    c.bytes[19] = s.lift_off_high as u8;
    c.bytes[20..42].copy_from_slice(&dpi_block(s)?);
    Ok(c)
}

pub fn calibration_start() -> Command {
    cmd(0x0D, 0xA4, 70)
}
pub fn calibration_set_surface(surface: u16) -> Command {
    let mut c = cmd(0x0D, 0xA5, 100);
    c.bytes[6..8].copy_from_slice(&surface.to_le_bytes());
    c
}
pub fn calibration_reset() -> Command {
    cmd(0x0D, 0xA6, 70)
}
pub fn calibration_result() -> Command {
    cmd(0x0D, 0xA7, 100)
}

fn lighting_block(c: &mut Command, l: &MouseLighting) -> ProtoResult<()> {
    if ![0, 1, 2, 3, 4, 5, 6, 7, 8, 15].contains(&l.effect) {
        return err(format!("unknown mouse lighting effect {}", l.effect));
    }
    c.bytes[16] = l.effect;
    for (i, col) in l.colors.iter().enumerate() {
        c.bytes[17 + 3 * i..20 + 3 * i].copy_from_slice(&col.bytes());
    }
    c.bytes[41] = crate::everest::quantize_brightness(l.brightness);
    c.bytes[42] = l.direction.min(1);
    c.bytes[43] = l.speed.min(2);
    Ok(())
}

/// `0C 00`: lighting, saved.
pub fn lighting_saved(p: u8, l: &MouseLighting) -> ProtoResult<Command> {
    let mut c = with_profile(cmd(0x0C, 0x00, 100), p);
    lighting_block(&mut c, l)?;
    Ok(c)
}

/// `3C 00`: lighting, live only.
pub fn lighting_live(l: &MouseLighting) -> ProtoResult<Command> {
    let mut c = cmd(0x3C, 0x00, 10);
    lighting_block(&mut c, l)?;
    Ok(c)
}

/// Button function types.
pub mod func {
    pub const MOUSE: u8 = 0x00;
    pub const WHEEL: u8 = 0x01;
    pub const KEY: u8 = 0x02;
    pub const MACRO: u8 = 0x03;
    pub const PROFILE: u8 = 0x08;
    pub const DPI: u8 = 0x09;
    pub const SNIPER: u8 = 0x0C;
    pub const OS_COMMAND: u8 = 0x18;
    pub const MEDIA: u8 = 0x20;
    pub const BRIGHTNESS: u8 = 0x21;
    pub const EFFECT: u8 = 0x22;
    pub const PROGRAM: u8 = 0x23;
    pub const URL: u8 = 0x24;
    pub const DISABLED: u8 = 0xFF;
}

/// Encoded button record and an optional bulk payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonRecord {
    pub ftype: u8,
    pub param: u8,
    pub aux: u16,
    pub aux2: u16,
    pub bulk: Option<Vec<u8>>,
}

/// Macro event (7 bytes): kind, code u16, make, delay (3 bytes).
pub fn macro_event(kind: u8, code: u16, make: bool, delay_ms: u32) -> [u8; 7] {
    let c = code.to_le_bytes();
    let d = delay_ms.min(0xFF_FFFF).to_le_bytes();
    [kind, c[0], c[1], make as u8, d[0], d[1], d[2]]
}

pub fn macro_bulk(events: &[[u8; 7]]) -> ProtoResult<Vec<u8>> {
    if events.len() > MAX_MACRO_EVENTS {
        return err(format!("at most {MAX_MACRO_EVENTS} mouse macro events"));
    }
    let mut v: Vec<u8> = events.iter().flatten().copied().collect();
    v.extend_from_slice(&[0u8; 7]);
    Ok(v)
}

fn mouse_mask(v: &str) -> Option<u8> {
    Some(match v.to_ascii_lowercase().as_str() {
        "left" => 1,
        "right" => 2,
        "middle" => 4,
        "back" => 8,
        "forward" => 16,
        _ => return None,
    })
}

fn media_usage(v: &str) -> Option<(u8, u16)> {
    Some(match v.to_ascii_lowercase().as_str() {
        "play/pause" => (0xCD, 0),
        "stop" => (0xB7, 0),
        "previous" => (0xB6, 0),
        "next" => (0xB5, 0),
        "volume up" => (0xE9, 0),
        "volume down" => (0xEA, 0),
        "mute" => (0xE2, 0),
        "media select" => (0, 0x0183),
        _ => return None,
    })
}

fn next_prev(v: &str) -> Option<u8> {
    match v.to_ascii_lowercase().as_str() {
        "next" => Some(0xF1),
        "previous" | "prev" => Some(0xF3),
        _ => None,
    }
}

/// Mouse macro events from a macro-library macro: keyboard events use HID usages.
/// TODO(spec): the mouse event classes `10`/`11` and their codes are inferred.
pub fn macro_events(m: &crate::model::Macro) -> ProtoResult<Vec<[u8; 7]>> {
    use crate::model::MacroEventType as T;
    let mut m = m.clone();
    m.normalize();
    let mut out = Vec::new();
    for e in &m.events {
        let ev = match e.kind {
            T::Keydown | T::Keyup => {
                let k = crate::macros::host_key(e).ok_or_else(|| crate::ProtoError("unknown key in macro".into()))?;
                if k.hid == 0 {
                    return err(format!("key {} has no HID keyboard usage", k.name));
                }
                macro_event(0x01, k.hid as u16, e.kind == T::Keydown, e.delay_ms)
            }
            T::Mousedown | T::Mouseup => {
                let mask = match e.button.unwrap_or(1) {
                    1 => 1,
                    2 => 2,
                    3 => 4,
                    b => return err(format!("mouse button {b} not supported")),
                };
                macro_event(0x10, mask, e.kind == T::Mousedown, e.delay_ms)
            }
            T::Wheel => macro_event(0x11, if e.rotation.unwrap_or(1) >= 0 { 1 } else { 0xFF }, true, e.delay_ms),
        };
        out.push(ev);
    }
    Ok(out)
}

/// Encode a button binding. `library` is used for `macro` bindings.
pub fn button_record(b: &MouseButton, library: &[crate::model::Macro]) -> ProtoResult<ButtonRecord> {
    let rec = |ftype, param| ButtonRecord { ftype, param, aux: 0, aux2: 0, bulk: None };
    let v = b.value.trim();
    Ok(match b.kind.as_str() {
        "mouse" => rec(func::MOUSE, mouse_mask(v).ok_or_else(|| crate::ProtoError(format!("unknown mouse button {v:?}")))?),
        "wheel" => rec(func::WHEEL, if v.eq_ignore_ascii_case("down") { 0xFF } else { 1 }),
        "key" => {
            let s = crate::shortcut::Shortcut::parse(v)?;
            let k = s.key.ok_or_else(|| crate::ProtoError("a key is required".into()))?;
            if k.hid == 0 {
                return err(format!("key {} cannot be assigned to a mouse button", k.name));
            }
            // TODO(spec): the modifier byte format is inferred to be the HID modifier bitmap.
            let mut hid_mods = 0u8;
            use crate::types::modifier as m;
            if s.mods & m::CTRL != 0 {
                hid_mods |= 0x01;
            }
            if s.mods & m::SHIFT != 0 {
                hid_mods |= 0x02;
            }
            if s.mods & m::ALT != 0 {
                hid_mods |= 0x04;
            }
            if s.mods & m::WIN != 0 {
                hid_mods |= 0x08;
            }
            ButtonRecord { ftype: func::KEY, param: hid_mods, aux: k.hid as u16, aux2: 0, bulk: None }
        }
        "macro" => {
            let m = library.iter().find(|m| m.name == v).ok_or_else(|| crate::ProtoError(format!("no macro named {v:?}")))?;
            let events = macro_events(m)?;
            let param = match m.playback {
                crate::types::PlaybackMode::Once => 0x01,
                crate::types::PlaybackMode::Hold => 0xF1,
                crate::types::PlaybackMode::Repeat => 0xF2,
            };
            ButtonRecord { ftype: func::MACRO, param, aux: 1, aux2: events.len() as u16, bulk: Some(macro_bulk(&events)?) }
        }
        "profile" => rec(func::PROFILE, next_prev(v).ok_or_else(|| crate::ProtoError("profile: next or previous".into()))?),
        "dpi" => rec(func::DPI, next_prev(v).ok_or_else(|| crate::ProtoError("dpi: next or previous".into()))?),
        "sniper" => {
            let dpi: u16 = v.parse().map_err(|_| crate::ProtoError("sniper DPI must be a number".into()))?;
            ButtonRecord { ftype: func::SNIPER, param: 0, aux: dpi, aux2: dpi, bulk: None }
        }
        "brightness" => rec(func::BRIGHTNESS, 0),
        "effect" => rec(func::EFFECT, 0),
        "media" => {
            let (param, aux) = media_usage(v).ok_or_else(|| crate::ProtoError(format!("unknown media action {v:?}")))?;
            ButtonRecord { ftype: func::MEDIA, param, aux, aux2: 0, bulk: None }
        }
        "program" | "url" => {
            if b.kind == "url" && !(v.starts_with("http://") || v.starts_with("https://")) {
                return err("URL must start with http:// or https://");
            }
            let text = match (&b.kind[..], b.value2.as_deref()) {
                ("program", Some(args)) if !args.is_empty() => format!("{v}~{args}"),
                _ => v.to_string(),
            };
            if text.len() > 1023 {
                return err("path or URL too long");
            }
            let ftype = if b.kind == "program" { func::PROGRAM } else { func::URL };
            ButtonRecord { ftype, param: 0, aux: text.len() as u16, aux2: 0, bulk: Some(text.into_bytes()) }
        }
        "disabled" => rec(func::DISABLED, 0),
        other => return err(format!("unknown mouse binding type {other:?}")),
    })
}

/// `0A 00`: button binding record at b16..b22.
pub fn button_command(p: u8, button: u8, r: &ButtonRecord) -> ProtoResult<Command> {
    if !(1..=6).contains(&button) {
        return err("button must be 1..6");
    }
    let mut c = with_profile(cmd(0x0A, 0x00, 50), p);
    c.bytes[6] = button;
    c.bytes[16] = r.ftype;
    c.bytes[17] = r.param;
    c.bytes[18..20].copy_from_slice(&r.aux.to_le_bytes());
    c.bytes[20..22].copy_from_slice(&r.aux2.to_le_bytes());
    c.bytes[22] = 0x0F;
    Ok(c)
}

/// Bulk write record (`A0`, 1041 bytes).
pub fn bulk_write(profile: u8, button: u8, data: &[u8]) -> ProtoResult<Vec<u8>> {
    if data.len() > 1024 {
        return err("bulk data exceeds 1024 bytes");
    }
    let mut b = vec![0u8; BULK_LEN];
    b[0] = REPORT_BULK;
    b[1] = 0x21;
    b[2] = button;
    b[5] = profile;
    b[16..16 + data.len()].copy_from_slice(data);
    let sum: u16 = b[16..1040].iter().fold(0u16, |a, &x| a.wrapping_add(x as u16));
    b[7..9].copy_from_slice(&sum.to_le_bytes());
    Ok(b)
}

/// Bulk read request `A1 25 <b4> 00 00 <b5>` for an event's identifiers.
pub fn bulk_read_request(e4: u8, e5: u8) -> Command {
    let mut c = cmd(0x25, e4, 50);
    c.bytes[5] = e5;
    c
}

/// Status check: b1 = 1 success; 3 or 4 busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Busy,
    Failed(u8),
}

pub fn parse_status(reply: &[u8]) -> Status {
    match reply.get(1).copied().unwrap_or(0) {
        1 => Status::Ok,
        3 | 4 => Status::Busy,
        v => Status::Failed(v),
    }
}

/// Text stored in a bulk read reply (from b16, NUL-terminated).
pub fn bulk_text(reply: &[u8]) -> String {
    let data = reply.get(16..).unwrap_or(&[]);
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).trim().to_string()
}

/// Input events (9-byte reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    RunProgram { e4: u8, e5: u8 },
    RunBrowser { e4: u8, e5: u8 },
    ProfileChanged(u8),
    DpiChanged,
    LightingChanged,
    OtherChanged,
    Calibration([u8; 3]),
    Unknown(u8),
}

/// Parse a 9-byte input report (b0 = report id).
pub fn parse_event(r: &[u8]) -> Option<Event> {
    if r.len() < 6 {
        return None;
    }
    Some(match r[1] {
        0x23 => Event::RunProgram { e4: r[4], e5: r[5] },
        0x24 => Event::RunBrowser { e4: r[4], e5: r[5] },
        0x01 => Event::ProfileChanged(r[2]),
        0x02 => Event::DpiChanged,
        0x03 | 0x04 => Event::LightingChanged,
        0x05 => Event::OtherChanged,
        0x06 => Event::Calibration([r[2], r[3], r[4]]),
        v => Event::Unknown(v),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Rgb;
    use crate::unhex;

    fn assert_cmd(c: &Command, expect: &str) {
        let e = unhex(expect);
        assert_eq!(&c.bytes[..e.len()], &e[..]);
        assert!(c.bytes[e.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn k1_k3() {
        assert_cmd(&polling_rate(1, 500).unwrap(), "a1 0d 01 00 00 01 02");
        assert_cmd(&set_current_profile(4), "a1 0b 04 00 00 04");
    }

    #[test]
    fn k2_dpi() {
        let s = MouseSensor { dpi_levels: vec![(800, 800), (1600, 1600), (3200, 3200)], current_level: 2, ..Default::default() };
        let c = dpi(2, &s).unwrap();
        assert_cmd(&c, "a1 0d 0a 00 00 02 03 02 00 00 00 00 00 00 00 00 20 03 20 03 40 06 40 06 80 0c 80 0c");
    }

    #[test]
    fn k4_lighting() {
        let l = MouseLighting { effect: 1, colors: [Rgb(255, 0, 0); 8], brightness: 100, direction: 0, speed: 1 };
        let c = lighting_live(&l).unwrap();
        assert_eq!(&c.bytes[..3], &[0xa1, 0x3c, 0x00]);
        assert_eq!(c.bytes[16], 1);
        for i in 0..8 {
            assert_eq!(&c.bytes[17 + 3 * i..20 + 3 * i], &[0xff, 0, 0]);
        }
        assert_eq!(&c.bytes[41..44], &[0x64, 0x00, 0x01]);
        assert!(c.bytes[44..].iter().all(|&b| b == 0));
    }

    #[test]
    fn k5_button() {
        let b = MouseButton { button: 4, kind: "media".into(), value: "Next".into(), value2: None };
        let c = button_command(1, 4, &button_record(&b, &[]).unwrap()).unwrap();
        assert_eq!(&c.bytes[..7], &unhex("a1 0a 00 00 00 01 04")[..]);
        assert_eq!((c.bytes[16], c.bytes[17], c.bytes[22]), (0x20, 0xb5, 0x0f));
    }

    #[test]
    fn k6_status_and_bulk() {
        assert_eq!(parse_status(&[0xa1, 3]), Status::Busy);
        assert_eq!(parse_status(&[0xa1, 1]), Status::Ok);
        let b = bulk_write(1, 3, b"abc").unwrap();
        assert_eq!(b.len(), BULK_LEN);
        assert_eq!(u16::from_le_bytes([b[7], b[8]]), (b'a' as u16) + (b'b' as u16) + (b'c' as u16));
        let mut reply = vec![0u8; 64];
        reply[16..19].copy_from_slice(b"x/y");
        assert_eq!(bulk_text(&reply), "x/y");
        assert_eq!(parse_event(&[0, 0x23, 0, 0, 1, 3, 0, 0, 0]), Some(Event::RunProgram { e4: 1, e5: 3 }));
    }
}

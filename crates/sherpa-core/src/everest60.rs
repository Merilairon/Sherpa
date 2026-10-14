//! Everest 60 protocol (spec 10): 64-byte feature reports `<cmd> 46 23 EA` + u32 arguments.

use crate::everest::BlockData;
use crate::model::Lighting;
use crate::types::MenuIndex;
use crate::{err, Packet, ProtoResult};

pub const MAGIC: [u8; 3] = [0x46, 0x23, 0xEA];

/// Delay between SetFeature and GetFeature (ms). TODO(spec): real values are unknown.
pub const DELAY_MS: u64 = 20;
pub const DELAY_LONG_MS: u64 = 40;

pub mod cmd {
    pub const SUB_DEVICE_INFO: u8 = 0x01;
    pub const DEVICE_INFO: u8 = 0x02;
    pub const AP_ON: u8 = 0x03;
    pub const AP_OFF: u8 = 0x04;
    pub const GET_LAYOUT: u8 = 0x05;
    pub const SET_LAYOUT: u8 = 0x06;
    pub const FW_INFO: u8 = 0x07;
    pub const POLL: u8 = 0x08;
    pub const MAIN_LIGHTING: u8 = 0x0C;
    pub const RESET_DEVICE: u8 = 0x0D;
    pub const SET_GAME_MODE_STATUS: u8 = 0x0E;
    pub const GET_GAME_MODE_STATUS: u8 = 0x0F;
    pub const SET_INDICATOR: u8 = 0x10;
    pub const GET_INDICATOR: u8 = 0x11;
    pub const GET_YETI: u8 = 0x12;
    pub const SET_YETI: u8 = 0x13;
    pub const GET_GAME_MODE: u8 = 0x14;
    pub const SET_GAME_MODE: u8 = 0x15;
    pub const SWITCH_PROFILE: u8 = 0x16;
    pub const SET_EFFECT: u8 = 0x17;
    pub const SET_BLOCK_EFFECT: u8 = 0x18;
    pub const SAVE: u8 = 0x1A;
    pub const RESET_FLASH: u8 = 0x1B;
    pub const RESET_EFFECTS: u8 = 0x1C;
    pub const KEY_FUNCTIONS: u8 = 0x20;
    pub const RESET_KEYS: u8 = 0x21;
    pub const REMAP: u8 = 0x22;
    pub const SHORTCUT_REMAP: u8 = 0x23;
    pub const MACRO_BEGIN: u8 = 0x25;
    pub const MACRO_DATA: u8 = 0x26;
    pub const MACRO_END: u8 = 0x27;
    pub const LAUNCH_BEGIN: u8 = 0x2A;
    pub const LAUNCH_DATA: u8 = 0x2B;
    pub const LAUNCH_END: u8 = 0x2C;
    pub const SET_SYNC_PROFILES: u8 = 0x30;
    pub const GET_SYNC_PROFILES: u8 = 0x31;
    pub const REMAP_FN: u8 = 0x4F;
    pub const MACRO_BEGIN_FN: u8 = 0x49;
    pub const MACRO_DATA_FN: u8 = 0x4A;
    pub const MACRO_END_FN: u8 = 0x4B;
    pub const LAUNCH_BEGIN_FN: u8 = 0x4C;
    pub const LAUNCH_DATA_FN: u8 = 0x4D;
    pub const LAUNCH_END_FN: u8 = 0x4E;
}

/// Command with up to four u32 arguments.
pub fn command(code: u8, args: &[u32]) -> Packet {
    let mut p = [0u8; 64];
    p[0] = code;
    p[1..4].copy_from_slice(&MAGIC);
    for (i, a) in args.iter().take(4).enumerate() {
        p[4 + 4 * i..8 + 4 * i].copy_from_slice(&a.to_le_bytes());
    }
    p
}

/// Data-variant command: up to 60 raw bytes at b4.
pub fn data_command(code: u8, data: &[u8]) -> Packet {
    let mut p = [0u8; 64];
    p[0] = code;
    p[1..4].copy_from_slice(&MAGIC);
    let n = data.len().min(60);
    p[4..4 + n].copy_from_slice(&data[..n]);
    p
}

/// Result `r<i>` of a reply.
pub fn result(p: &Packet, i: usize) -> u32 {
    let o = 4 + 4 * i;
    if o + 4 > 64 {
        return 0;
    }
    u32::from_le_bytes([p[o], p[o + 1], p[o + 2], p[o + 3]])
}

/// Reply check: b0 echoes the command code.
pub fn reply_ok(req: &Packet, reply: &Packet) -> bool {
    req[0] == reply[0]
}

/// Long delay for the end command of a multi-part transfer.
pub fn delay_for(code: u8) -> u64 {
    match code {
        cmd::MACRO_END | cmd::MACRO_END_FN | cmd::LAUNCH_END | cmd::LAUNCH_END_FN => DELAY_LONG_MS,
        _ => DELAY_MS,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FwInfo {
    pub fw_version: u32,
    pub profile_count: u32,
    pub effect_mode: u32,
    pub current_profile: u32,
    pub effect_menu: u32,
}

pub fn parse_fw_info(p: &Packet) -> FwInfo {
    FwInfo {
        fw_version: result(p, 0),
        // As on the Everest Max, the device reports the profile count + 1.
        profile_count: result(p, 2).saturating_sub(1),
        effect_mode: result(p, 3),
        current_profile: result(p, 4),
        effect_menu: result(p, 5),
    }
}

/// Parsed status poll reply (§2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Poll {
    /// 0 none, 1 left, 2 right.
    pub numpad: u8,
    /// `(key code, pressed)`.
    pub events: Vec<(u8, bool)>,
    pub status: [u8; 2],
    pub profile: u8,
    pub effect: u8,
}

pub fn parse_poll(p: &Packet) -> Poll {
    let n = p[5] as usize;
    let events = if (1..=26).contains(&n) { (0..n).map(|i| (p[6 + 2 * i], p[7 + 2 * i] == 1)).collect() } else { Vec::new() };
    Poll { numpad: p[4], events, status: [p[60], p[61]], profile: p[62], effect: p[63] }
}

pub fn numpad_side(v: u8) -> &'static str {
    match v {
        1 => "left",
        2 => "right",
        _ => "none",
    }
}

/// Quantise to the UI steps 0, 25, 50, 75, 100.
fn q(v: u8) -> u8 {
    crate::everest::quantize_brightness(v)
}

/// Lighting command(s) for an entry: `17` (EffectData) or `18` (BlockData) with the
/// Everest 60 effect id. TODO(spec): Everest 60 speed/direction semantics are inferred
/// to follow the Everest Max layout.
pub fn lighting_packet(l: &Lighting) -> ProtoResult<Packet> {
    let mut l = l.clone();
    l.speed = q(l.speed);
    l.brightness = q(l.brightness);
    let id = l.menu_index.everest60_effect();
    match l.menu_index {
        MenuIndex::ColorWave | MenuIndex::Tornado => {
            let max = BlockData::from_lighting(l.menu_index.effect_index(), &l).encode()?;
            let mut data = max[2..62].to_vec();
            data[0] = id;
            Ok(data_command(cmd::SET_BLOCK_EFFECT, &data))
        }
        MenuIndex::Custom => {
            // TODO(spec): the per-key custom transfer (32/34/35/36) layout is unknown;
            // only the effect id and brightness are sent.
            let mut data = [0u8; 60];
            data[0] = id;
            data[1] = 1;
            data[2] = 0xFF;
            data[3] = l.brightness;
            Ok(data_command(cmd::SET_EFFECT, &data))
        }
        _ => {
            let max = crate::everest::simple_effect(l.menu_index.effect_index(), &l)?;
            let mut data = max[2..62].to_vec();
            data[0] = id;
            Ok(data_command(cmd::SET_EFFECT, &data))
        }
    }
}

/// Layer-dependent command codes: (remap, macro begin/data/end, launch begin/data/end).
pub struct LayerCodes {
    pub remap: u8,
    pub macro_cmds: [u8; 3],
    pub launch_cmds: [u8; 3],
}

pub fn layer_codes(fn_layer: bool) -> LayerCodes {
    if fn_layer {
        LayerCodes {
            remap: cmd::REMAP_FN,
            macro_cmds: [cmd::MACRO_BEGIN_FN, cmd::MACRO_DATA_FN, cmd::MACRO_END_FN],
            launch_cmds: [cmd::LAUNCH_BEGIN_FN, cmd::LAUNCH_DATA_FN, cmd::LAUNCH_END_FN],
        }
    } else {
        LayerCodes {
            remap: cmd::REMAP,
            macro_cmds: [cmd::MACRO_BEGIN, cmd::MACRO_DATA, cmd::MACRO_END],
            launch_cmds: [cmd::LAUNCH_BEGIN, cmd::LAUNCH_DATA, cmd::LAUNCH_END],
        }
    }
}

/// Macro transfer: begin, data chunks (14 events each), end.
pub fn macro_packets(fn_layer: bool, src: u8, playback: u8, events: &[[u8; 4]]) -> ProtoResult<Vec<Packet>> {
    if src == 0 {
        return err("macro source key must be 1..255");
    }
    if playback > 4 {
        return err("playback type must be ≤ 4");
    }
    let c = layer_codes(fn_layer).macro_cmds;
    let mut out = vec![command(c[0], &[src as u32, playback as u32, 1, events.len() as u32])];
    for chunk in events.chunks(14) {
        let mut data = Vec::with_capacity(60);
        data.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        for e in chunk {
            data.extend_from_slice(e);
        }
        out.push(data_command(c[1], &data));
    }
    out.push(command(c[2], &[]));
    Ok(out)
}

/// Launch-content transfer: begin, data chunks (56 bytes each), end.
pub fn launch_packets(fn_layer: bool, src: u8, payload: &[u8]) -> Vec<Packet> {
    let c = layer_codes(fn_layer).launch_cmds;
    let mut out = vec![command(c[0], &[src as u32, payload.len() as u32])];
    for chunk in payload.chunks(56) {
        let mut data = Vec::with_capacity(60);
        data.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        data.extend_from_slice(chunk);
        out.push(data_command(c[1], &data));
    }
    out.push(command(c[2], &[]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unhex;

    fn assert_pkt(p: &Packet, expect: &str) {
        let e = unhex(expect);
        assert_eq!(&p[..e.len()], &e[..]);
        assert!(p[e.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn vectors() {
        assert_pkt(&command(cmd::POLL, &[]), "08 46 23 ea");
        assert_pkt(&command(cmd::SWITCH_PROFILE, &[3, 2]), "16 46 23 ea 03 00 00 00 02 00 00 00");
        assert_pkt(&command(cmd::SET_LAYOUT, &[1]), "06 46 23 ea 01 00 00 00");
        let ev = vec![[0u8; 4]; 7];
        let pk = macro_packets(false, 30, 1, &ev).unwrap();
        assert_pkt(&pk[0], "25 46 23 ea 1e 00 00 00 01 00 00 00 01 00 00 00 07 00 00 00");
        assert_eq!(pk.len(), 3);
        assert_eq!(pk[1][4], 7);
        assert_eq!(pk[2][0], 0x27);
    }

    #[test]
    fn e5_poll() {
        let mut p = [0u8; 64];
        p[0] = 0x08;
        p[4] = 2;
        p[5] = 1;
        p[6] = 0x1e;
        p[7] = 1;
        p[62] = 2;
        p[63] = 4;
        let poll = parse_poll(&p);
        assert_eq!(numpad_side(poll.numpad), "right");
        assert_eq!(poll.events, vec![(30, true)]);
        assert_eq!((poll.profile, poll.effect), (2, 4));
        p[5] = 30;
        assert!(parse_poll(&p).events.is_empty(), "invalid count ignored");
    }

    #[test]
    fn chunks() {
        let ev = vec![[1u8; 4]; 29];
        let pk = macro_packets(true, 5, 4, &ev).unwrap();
        assert_eq!(pk.iter().map(|p| p[0]).collect::<Vec<_>>(), vec![0x49, 0x4A, 0x4A, 0x4A, 0x4B]);
        assert_eq!((pk[1][4], pk[3][4]), (14, 1));
        let lp = launch_packets(false, 9, &[7u8; 100]);
        assert_eq!(lp.len(), 4);
        assert_eq!((lp[0][8], lp[1][4], lp[2][4]), (100, 56, 44));
        assert!(macro_packets(false, 1, 5, &ev).is_err());
    }

    #[test]
    fn effect_ids() {
        let p = lighting_packet(&Lighting::new(MenuIndex::Breathing)).unwrap();
        assert_eq!((p[0], p[4]), (0x17, 4));
        let p = lighting_packet(&Lighting::new(MenuIndex::Tornado)).unwrap();
        assert_eq!((p[0], p[4]), (0x18, 3));
    }
}

//! Device macro event encoding shared by the Everest family and the Everest 60
//! (spec 04 §7.4, 10 §4.6).

use crate::keys;
use crate::model::{Macro, MacroEvent, MacroEventType};
use crate::shortcut;
use crate::tables::KeyRow;
use crate::{err, ProtoResult};

/// Maximum number of device events including the terminator.
pub const MAX_EVENTS: usize = 1000;
pub const TERMINATOR: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];

pub const PAGE_KEYBOARD: u8 = 0;
pub const PAGE_MOUSE: u8 = 1;
pub const ACTION_RELEASE: u8 = 1;
pub const ACTION_PRESS: u8 = 2;
pub const ACTION_WHEEL: u8 = 3;
pub const WHEEL_DOWN: u8 = 48;
pub const WHEEL_UP: u8 = 49;

pub fn event(key: u8, page: u8, action: u8, delay: u16) -> [u8; 4] {
    let d = delay.to_le_bytes();
    [key, (page << 4) | action, d[0], d[1]]
}

/// Resolve the host key of a macro event (`key` name first, then `rawcode` VK).
pub fn host_key(e: &MacroEvent) -> Option<&'static keys::HostKey> {
    e.key.as_deref().and_then(keys::by_name).or_else(|| e.rawcode.and_then(keys::by_vk))
}

/// Firmware key of a macro key event in the given device key table.
fn event_fw_key(e: &MacroEvent, table: &[KeyRow]) -> ProtoResult<u8> {
    if let Some(k) = host_key(e) {
        if let Some(fw) = shortcut::fw_key(table, k) {
            return Ok(fw as u8);
        }
    }
    // Imported Base Camp files store VKs; some only exist in the device table.
    if let Some(vk) = e.rawcode {
        if let Some(r) = table.iter().find(|r| r.vk == vk) {
            return Ok(r.fw_key as u8);
        }
    }
    err(format!("macro key {:?} has no firmware key on this device", e.key.as_deref().or(Some("?"))))
}

/// Encode a macro's events for the device, appending the terminator.
pub fn encode(m: &Macro, table: &[KeyRow]) -> ProtoResult<Vec<[u8; 4]>> {
    let mut m = m.clone();
    m.normalize();
    let mut out = Vec::with_capacity(m.events.len() + 1);
    for e in &m.events {
        let delay = e.delay_ms.min(u16::MAX as u32) as u16;
        let ev = match e.kind {
            MacroEventType::Keydown => event(event_fw_key(e, table)?, PAGE_KEYBOARD, ACTION_PRESS, delay),
            MacroEventType::Keyup => event(event_fw_key(e, table)?, PAGE_KEYBOARD, ACTION_RELEASE, delay),
            MacroEventType::Mousedown | MacroEventType::Mouseup => {
                let b = e.button.unwrap_or(1);
                if !(1..=3).contains(&b) {
                    return err(format!("mouse button {b} cannot be stored in a device macro"));
                }
                let action = if e.kind == MacroEventType::Mousedown { ACTION_PRESS } else { ACTION_RELEASE };
                event(b, PAGE_MOUSE, action, delay)
            }
            MacroEventType::Wheel => {
                let key = if e.rotation.unwrap_or(1) >= 0 { WHEEL_UP } else { WHEEL_DOWN };
                event(key, PAGE_MOUSE, ACTION_WHEEL, delay)
            }
        };
        out.push(ev);
    }
    out.push(TERMINATOR);
    if out.len() > MAX_EVENTS {
        return err(format!("macro has {} events; the device limit is {} including the terminator", out.len(), MAX_EVENTS));
    }
    Ok(out)
}

/// The built-in "Task manager" macro: LCtrl↓ LShift↓ Esc↓ Esc↑ LShift↑ LCtrl↑ (01 §5).
pub fn task_manager() -> Vec<[u8; 4]> {
    vec![
        event(58, PAGE_KEYBOARD, ACTION_PRESS, 0),
        event(44, PAGE_KEYBOARD, ACTION_PRESS, 0),
        event(110, PAGE_KEYBOARD, ACTION_PRESS, 0),
        event(110, PAGE_KEYBOARD, ACTION_RELEASE, 0),
        event(44, PAGE_KEYBOARD, ACTION_RELEASE, 0),
        event(58, PAGE_KEYBOARD, ACTION_RELEASE, 0),
        TERMINATOR,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;
    use crate::types::DelayMode;

    #[test]
    fn encodes_keys_mouse_wheel() {
        let m = Macro {
            name: "t".into(),
            events: vec![
                MacroEvent::key(MacroEventType::Keydown, "A", 20),
                MacroEvent::key(MacroEventType::Keyup, "A", 20),
                MacroEvent { kind: MacroEventType::Mousedown, key: None, rawcode: None, button: Some(2), rotation: None, delay_ms: 5 },
                MacroEvent { kind: MacroEventType::Wheel, key: None, rawcode: None, button: None, rotation: Some(-1), delay_ms: 7 },
            ],
            ..Default::default()
        };
        let ev = encode(&m, tables::everest_max_keys()).unwrap();
        assert_eq!(ev[0], [31, 0x02, 20, 0]);
        assert_eq!(ev[1], [31, 0x01, 20, 0]);
        assert_eq!(ev[2], [2, 0x12, 5, 0]);
        assert_eq!(ev[3], [48, 0x13, 0, 0], "last delay forced to 0");
        assert_eq!(ev[4], TERMINATOR);
    }

    #[test]
    fn delay_none_and_rawcode_import() {
        let m = Macro {
            delay_mode: DelayMode::None,
            events: vec![
                MacroEvent { kind: MacroEventType::Keydown, key: None, rawcode: Some(0x41), button: None, rotation: None, delay_ms: 99 },
                MacroEvent { kind: MacroEventType::Keyup, key: None, rawcode: Some(0x41), button: None, rotation: None, delay_ms: 99 },
            ],
            ..Default::default()
        };
        let ev = encode(&m, tables::everest_max_keys()).unwrap();
        assert_eq!(ev[0], [31, 0x02, 0, 0]);
    }

    #[test]
    fn limit() {
        let m = Macro { events: vec![MacroEvent::key(MacroEventType::Keydown, "A", 0); 1000], ..Default::default() };
        assert!(encode(&m, tables::everest_max_keys()).is_err());
        let m = Macro { events: vec![MacroEvent::key(MacroEventType::Keydown, "A", 0); 999], ..Default::default() };
        assert_eq!(encode(&m, tables::everest_max_keys()).unwrap().len(), 1000);
    }
}

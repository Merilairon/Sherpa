//! Host-side key naming: canonical key names, Windows VK codes, Linux evdev codes and
//! USB HID keyboard usages. This is public platform data (not Base Camp data) used to
//! translate between shortcut strings, recorded input and the device key tables.

/// A host key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostKey {
    /// Canonical name, used in shortcut strings and macro events.
    pub name: &'static str,
    /// Windows virtual-key code.
    pub vk: u32,
    /// Linux input event code (`KEY_*`), 0 if none.
    pub evdev: u16,
    /// USB HID keyboard page usage, 0 if none.
    pub hid: u8,
}

const fn k(name: &'static str, vk: u32, evdev: u16, hid: u8) -> HostKey {
    HostKey { name, vk, evdev, hid }
}

/// All known keys.
pub static KEYS: &[HostKey] = &[
    k("Esc", 0x1B, 1, 0x29),
    k("F1", 0x70, 59, 0x3A),
    k("F2", 0x71, 60, 0x3B),
    k("F3", 0x72, 61, 0x3C),
    k("F4", 0x73, 62, 0x3D),
    k("F5", 0x74, 63, 0x3E),
    k("F6", 0x75, 64, 0x3F),
    k("F7", 0x76, 65, 0x40),
    k("F8", 0x77, 66, 0x41),
    k("F9", 0x78, 67, 0x42),
    k("F10", 0x79, 68, 0x43),
    k("F11", 0x7A, 87, 0x44),
    k("F12", 0x7B, 88, 0x45),
    k("F13", 0x7C, 183, 0x68),
    k("F14", 0x7D, 184, 0x69),
    k("F15", 0x7E, 185, 0x6A),
    k("F16", 0x7F, 186, 0x6B),
    k("F17", 0x80, 187, 0x6C),
    k("F18", 0x81, 188, 0x6D),
    k("F19", 0x82, 189, 0x6E),
    k("F20", 0x83, 190, 0x6F),
    k("F21", 0x84, 191, 0x70),
    k("F22", 0x85, 192, 0x71),
    k("F23", 0x86, 193, 0x72),
    k("F24", 0x87, 194, 0x73),
    k("`", 0xC0, 41, 0x35),
    k("1", 0x31, 2, 0x1E),
    k("2", 0x32, 3, 0x1F),
    k("3", 0x33, 4, 0x20),
    k("4", 0x34, 5, 0x21),
    k("5", 0x35, 6, 0x22),
    k("6", 0x36, 7, 0x23),
    k("7", 0x37, 8, 0x24),
    k("8", 0x38, 9, 0x25),
    k("9", 0x39, 10, 0x26),
    k("0", 0x30, 11, 0x27),
    k("-", 0xBD, 12, 0x2D),
    k("=", 0xBB, 13, 0x2E),
    k("Backspace", 0x08, 14, 0x2A),
    k("Tab", 0x09, 15, 0x2B),
    k("Q", 0x51, 16, 0x14),
    k("W", 0x57, 17, 0x1A),
    k("E", 0x45, 18, 0x08),
    k("R", 0x52, 19, 0x15),
    k("T", 0x54, 20, 0x17),
    k("Y", 0x59, 21, 0x1C),
    k("U", 0x55, 22, 0x18),
    k("I", 0x49, 23, 0x0C),
    k("O", 0x4F, 24, 0x12),
    k("P", 0x50, 25, 0x13),
    k("[", 0xDB, 26, 0x2F),
    k("]", 0xDD, 27, 0x30),
    k("Enter", 0x0D, 28, 0x28),
    k("LCtrl", 0xA2, 29, 0xE0),
    k("A", 0x41, 30, 0x04),
    k("S", 0x53, 31, 0x16),
    k("D", 0x44, 32, 0x07),
    k("F", 0x46, 33, 0x09),
    k("G", 0x47, 34, 0x0A),
    k("H", 0x48, 35, 0x0B),
    k("J", 0x4A, 36, 0x0D),
    k("K", 0x4B, 37, 0x0E),
    k("L", 0x4C, 38, 0x0F),
    k(";", 0xBA, 39, 0x33),
    k("'", 0xDE, 40, 0x34),
    k("LShift", 0xA0, 42, 0xE1),
    k("\\", 0xDC, 43, 0x31),
    k("Z", 0x5A, 44, 0x1D),
    k("X", 0x58, 45, 0x1B),
    k("C", 0x43, 46, 0x06),
    k("V", 0x56, 47, 0x19),
    k("B", 0x42, 48, 0x05),
    k("N", 0x4E, 49, 0x11),
    k("M", 0x4D, 50, 0x10),
    k(",", 0xBC, 51, 0x36),
    k(".", 0xBE, 52, 0x37),
    k("/", 0xBF, 53, 0x38),
    k("RShift", 0xA1, 54, 0xE5),
    k("Num *", 0x6A, 55, 0x55),
    k("LAlt", 0xA4, 56, 0xE2),
    k("Space", 0x20, 57, 0x2C),
    k("CapsLock", 0x14, 58, 0x39),
    k("NumLock", 0x90, 69, 0x53),
    k("ScrollLock", 0x91, 70, 0x47),
    k("Num 7", 0x67, 71, 0x5F),
    k("Num 8", 0x68, 72, 0x60),
    k("Num 9", 0x69, 73, 0x61),
    k("Num -", 0x6D, 74, 0x56),
    k("Num 4", 0x64, 75, 0x5C),
    k("Num 5", 0x65, 76, 0x5D),
    k("Num 6", 0x66, 77, 0x5E),
    k("Num +", 0x6B, 78, 0x57),
    k("Num 1", 0x61, 79, 0x59),
    k("Num 2", 0x62, 80, 0x5A),
    k("Num 3", 0x63, 81, 0x5B),
    k("Num 0", 0x60, 82, 0x62),
    k("Num .", 0x6E, 83, 0x63),
    // Base Camp's key table uses the pseudo VK 3612 for the numpad Enter.
    k("Num Enter", 3612, 96, 0x58),
    k("RCtrl", 0xA3, 97, 0xE4),
    k("Num /", 0x6F, 98, 0x54),
    k("PrintScreen", 0x2C, 99, 0x46),
    k("RAlt", 0xA5, 100, 0xE6),
    k("Home", 0x24, 102, 0x4A),
    k("Up", 0x26, 103, 0x52),
    k("PageUp", 0x21, 104, 0x4B),
    k("Left", 0x25, 105, 0x50),
    k("Right", 0x27, 106, 0x4F),
    k("End", 0x23, 107, 0x4D),
    k("Down", 0x28, 108, 0x51),
    k("PageDown", 0x22, 109, 0x4E),
    k("Insert", 0x2D, 110, 0x49),
    k("Delete", 0x2E, 111, 0x4C),
    k("Mute", 0xAD, 113, 0x7F),
    k("VolumeDown", 0xAE, 114, 0x81),
    k("VolumeUp", 0xAF, 115, 0x80),
    k("Pause", 0x13, 119, 0x48),
    k("LWin", 0x5B, 125, 0xE3),
    k("RWin", 0x5C, 126, 0xE7),
    k("Menu", 0x5D, 127, 0x65),
    k("NextTrack", 0xB0, 163, 0),
    k("PlayPause", 0xB3, 164, 0),
    k("PrevTrack", 0xB1, 165, 0),
    k("StopMedia", 0xB2, 166, 0),
];

/// Alternate spellings accepted in shortcut strings and imported files
/// (including the labels used in the device key tables).
static ALIASES: &[(&str, &str)] = &[
    ("escape", "Esc"),
    ("ctrl", "LCtrl"),
    ("control", "LCtrl"),
    ("lctrl", "LCtrl"),
    ("shift", "LShift"),
    ("alt", "LAlt"),
    ("win", "LWin"),
    ("super", "LWin"),
    ("meta", "LWin"),
    ("cmd", "LWin"),
    ("⊞", "LWin"),
    ("alt gr", "RAlt"),
    ("altgr", "RAlt"),
    ("return", "Enter"),
    ("enter↵", "Enter"),
    ("del", "Delete"),
    ("ins", "Insert"),
    ("pgup", "PageUp"),
    ("pgdn", "PageDown"),
    ("prt sc", "PrintScreen"),
    ("prtsc", "PrintScreen"),
    ("snapshot", "PrintScreen"),
    ("scroll lk", "ScrollLock"),
    ("scroll lock", "ScrollLock"),
    ("caps lk", "CapsLock"),
    ("caps lock", "CapsLock"),
    ("num lk", "NumLock"),
    ("num lock", "NumLock"),
    ("numlock", "NumLock"),
    ("⭠", "Backspace"),
    ("back", "Backspace"),
    ("tab↹", "Tab"),
    ("↑shift", "LShift"),
    ("⏴", "Left"),
    ("←", "Left"),
    ("⏶", "Up"),
    ("↑", "Up"),
    ("⏵", "Right"),
    ("→", "Right"),
    ("⏷", "Down"),
    ("↓", "Down"),
    (" ", "Space"),
    ("spacebar", "Space"),
    // A single "+" means the plus key itself (the "=+" key on US layouts).
    ("+", "="),
    ("plus", "="),
    ("minus", "-"),
    ("play pause", "PlayPause"),
    ("next track", "NextTrack"),
    ("pre track", "PrevTrack"),
    ("previous track", "PrevTrack"),
    ("apps", "Menu"),
];

/// Find a key by canonical name or alias (case-insensitive).
pub fn by_name(name: &str) -> Option<&'static HostKey> {
    if name.is_empty() {
        return None;
    }
    if let Some(k) = KEYS.iter().find(|k| k.name.eq_ignore_ascii_case(name)) {
        return Some(k);
    }
    let lower = name.to_lowercase();
    let target = ALIASES.iter().find(|(a, _)| *a == lower).map(|(_, t)| *t)?;
    KEYS.iter().find(|k| k.name == target)
}

pub fn by_vk(vk: u32) -> Option<&'static HostKey> {
    KEYS.iter().find(|k| k.vk == vk)
}

pub fn by_evdev(code: u16) -> Option<&'static HostKey> {
    if code == 0 {
        return None;
    }
    KEYS.iter().find(|k| k.evdev == code)
}

/// Linux evdev mouse-button codes.
pub mod evdev_btn {
    pub const LEFT: u16 = 0x110;
    pub const RIGHT: u16 = 0x111;
    pub const MIDDLE: u16 = 0x112;
    pub const SIDE: u16 = 0x113;
    pub const EXTRA: u16 = 0x114;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    #[test]
    fn names_resolve() {
        assert_eq!(by_name("ctrl").unwrap().name, "LCtrl");
        assert_eq!(by_name("x").unwrap().vk, 0x58);
        assert_eq!(by_name("+").unwrap().vk, 0xBB);
        assert_eq!(by_name("⏴").unwrap().name, "Left");
    }

    #[test]
    fn unique_codes() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a.vk, b.vk, "{} {}", a.name, b.name);
                assert_ne!(a.evdev, b.evdev, "{} {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn every_keyboard_table_entry_has_a_host_key() {
        // Every real keyboard key in the Everest Max table maps. The table uses pseudo
        // VKs 230..241 for F13..F24 and >255 for device-only keys.
        for row in tables::everest_max_keys() {
            if (row.vk < 230 || row.vk == 3612) && row.vk != 0 {
                assert!(by_vk(row.vk).is_some(), "no host key for {row:?}");
            }
        }
    }
}

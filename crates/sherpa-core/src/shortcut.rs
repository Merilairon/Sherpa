//! Shortcut strings such as `Ctrl+Shift+X` (spec 05 §3.1).

use crate::keys::{self, HostKey};
use crate::tables::KeyRow;
use crate::types::modifier;
use crate::{err, ProtoResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    /// Modifier mask (Ctrl 1, Shift 2, Alt 4, Win 8).
    pub mods: u8,
    /// The final, non-modifier key. `None` for a modifier-only shortcut such as `Ctrl`.
    pub key: Option<&'static HostKey>,
}

fn modifier_bit(tok: &str) -> Option<u8> {
    match tok.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(modifier::CTRL),
        "shift" => Some(modifier::SHIFT),
        "alt" => Some(modifier::ALT),
        "win" | "super" | "meta" => Some(modifier::WIN),
        _ => None,
    }
}

impl Shortcut {
    pub fn parse(s: &str) -> ProtoResult<Shortcut> {
        let s = s.trim();
        if s.is_empty() {
            return err("empty shortcut");
        }
        if s == "+" {
            return Ok(Shortcut { mods: 0, key: keys::by_name("+") });
        }
        // "Ctrl++" ends with the plus key itself.
        let (head, last) = if let Some(h) = s.strip_suffix("++") {
            (h, "+")
        } else {
            match s.rfind('+') {
                Some(i) => (&s[..i], &s[i + 1..]),
                None => ("", s),
            }
        };
        let mut mods = 0u8;
        for tok in head.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            mods |= modifier_bit(tok).ok_or_else(|| crate::ProtoError(format!("unknown modifier {tok:?}")))?;
        }
        let last = last.trim();
        if let Some(bit) = modifier_bit(last) {
            return Ok(Shortcut { mods: mods | bit, key: None });
        }
        let key = keys::by_name(last).ok_or_else(|| crate::ProtoError(format!("unknown key {last:?}")))?;
        Ok(Shortcut { mods, key: Some(key) })
    }

    /// `Win+L` locks the session instead of sending keys.
    pub fn is_lock(&self) -> bool {
        self.mods == modifier::WIN && self.key.map(|k| k.name) == Some("L")
    }
}

impl std::fmt::Display for Shortcut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        for (bit, name) in [(modifier::CTRL, "Ctrl"), (modifier::SHIFT, "Shift"), (modifier::ALT, "Alt"), (modifier::WIN, "Win")] {
            if self.mods & bit != 0 {
                parts.push(name.to_string());
            }
        }
        if let Some(k) = self.key {
            parts.push(k.name.to_string());
        }
        f.write_str(&parts.join("+"))
    }
}

/// Firmware key code of a host key in a device key table: by VK first, then by name
/// (the tables use pseudo VKs for some keys such as F13..F24).
pub fn fw_key(table: &[KeyRow], key: &HostKey) -> Option<u16> {
    table
        .iter()
        .find(|r| r.vk == key.vk)
        .or_else(|| table.iter().find(|r| keys::by_name(&r.name).map(|k| k.name) == Some(key.name)))
        .map(|r| r.fw_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    #[test]
    fn parse_forms() {
        let s = Shortcut::parse("Ctrl+Shift+X").unwrap();
        assert_eq!(s.mods, 3);
        assert_eq!(s.key.unwrap().name, "X");
        let p = Shortcut::parse("+").unwrap();
        assert_eq!((p.mods, p.key.unwrap().vk), (0, 0xBB));
        let cp = Shortcut::parse("ctrl++").unwrap();
        assert_eq!((cp.mods, cp.key.unwrap().vk), (1, 0xBB));
        assert!(Shortcut::parse("win+l").unwrap().is_lock());
        assert!(Shortcut::parse("Hyper+X").is_err());
        assert_eq!(Shortcut::parse("alt+f4").unwrap().to_string(), "Alt+F4");
    }

    #[test]
    fn fw_keys() {
        let t = tables::everest_max_keys();
        assert_eq!(fw_key(t, keys::by_name("L").unwrap()), Some(39));
        assert_eq!(fw_key(t, keys::by_name("E").unwrap()), Some(19));
        assert_eq!(fw_key(t, keys::by_name("Esc").unwrap()), Some(110));
        assert_eq!(fw_key(t, keys::by_name("F13").unwrap()), Some(230));
        assert_eq!(fw_key(t, keys::by_name("LCtrl").unwrap()), Some(58));
        assert_eq!(fw_key(t, keys::by_name("LShift").unwrap()), Some(44));
    }
}

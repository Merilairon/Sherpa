//! Translate a key binding into the device operation that stores it (spec 01 §5, F-KEY-2).

use crate::launch;
use crate::macros;
use crate::model::{BindingType, KeyBinding, Macro};
use crate::shortcut::{self, Shortcut};
use crate::tables::KeyRow;
use crate::types::{LaunchKind, PlaybackMode};
use crate::{err, ProtoResult};

/// What has to be written to the device for a binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOp {
    /// Plain remap to a fwKey or function target code (255 = default, 0 = disable).
    Remap(u16),
    /// Shortcut remap: target fwKey + modifier mask.
    Shortcut { key: u16, mods: u8 },
    /// Macro stored on the key: playback device code + encoded events (incl. terminator).
    Macro { playback: u8, events: Vec<[u8; 4]> },
    /// Launch content (host executes on release).
    Launch(Vec<u8>),
    /// Nothing is stored on the device (DisplayPad navigation, …).
    None,
}

pub const REMAP_DEFAULT: u16 = 255;
pub const REMAP_DISABLE: u16 = 0;

/// Media sub-options and their target codes (04 §7.2).
pub const MEDIA: &[(&str, u16)] =
    &[("Play/Pause", 183), ("Stop", 182), ("Previous", 181), ("Next", 180), ("Volume up", 187), ("Volume down", 188), ("Mute", 184)];

/// Mouse sub-options stored as a remap.
pub const MOUSE: &[(&str, u16)] =
    &[("Left", 257), ("Right", 258), ("Middle", 259), ("Forward", 260), ("Back", 261), ("Scroll up", 304), ("Scroll down", 305)];

/// Mouse sub-options that would need a stored one-event wheel macro. The horizontal wheel
/// key codes are not in the specification.
pub const MOUSE_UNSPECIFIED: &[&str] = &["Scroll left", "Scroll right"];

pub const PROFILE: &[(&str, u16)] = &[
    ("Profile 1", 160),
    ("Profile 2", 161),
    ("Profile 3", 162),
    ("Profile 4", 163),
    ("Profile 5", 164),
    ("Cycle", 210),
    ("Previous", 211),
    ("Next", 212),
];

pub const OS_REMAP: &[(&str, u16)] = &[("Shutdown", 128), ("Sleep", 129), ("Hibernate", 129), ("Calculator", 195)];
pub const OS_OTHER: &[&str] = &["Lock computer", "Run explorer", "Task manager"];

fn lookup(table: &[(&str, u16)], v: &str) -> Option<u16> {
    table.iter().find(|(n, _)| n.eq_ignore_ascii_case(v.trim())).map(|(_, c)| *c)
}

/// F13..F24 → 230..241.
pub fn keyboard_function(v: &str) -> Option<u16> {
    let n: u16 = v.trim().trim_start_matches(['F', 'f']).parse().ok()?;
    (13..=24).contains(&n).then(|| 230 + (n - 13))
}

/// Launch payload for a host-executed binding.
pub fn launch_payload(b: &KeyBinding) -> Option<Vec<u8>> {
    let v2 = b.value2.as_deref().unwrap_or("");
    let sub = b.sub_type.as_deref().unwrap_or("");
    let (kind, text) = match b.kind {
        BindingType::Program => (LaunchKind::Program, launch::join_args([b.value.as_str(), v2])),
        BindingType::Url => (LaunchKind::Url, b.value.clone()),
        BindingType::Obs => (LaunchKind::Obs, launch::join_args([sub_or_value(sub, &b.value), obs_arg(sub, b)])),
        BindingType::Folder => (LaunchKind::Folder, b.value.clone()),
        BindingType::Twitch => (LaunchKind::Twitch, launch::join_args([sub, b.value.as_str(), v2])),
        BindingType::YouTube => (LaunchKind::YouTube, launch::join_args([sub, b.value.as_str()])),
        BindingType::Spotify => (LaunchKind::Spotify, launch::join_args([sub, b.value.as_str(), v2])),
        BindingType::HotkeySwitch => (LaunchKind::HotkeySwitch, launch::join_args([b.value.as_str(), v2])),
        BindingType::MultiAction => (LaunchKind::MultiAction, b.value.clone()),
        _ => return None,
    };
    Some(launch::encode(kind, &text))
}

/// OBS bindings carry the command in `subType` and its argument in `value`; accept a
/// command in `value` when there is no `subType`.
fn sub_or_value<'a>(sub: &'a str, value: &'a str) -> &'a str {
    if sub.is_empty() {
        value
    } else {
        sub
    }
}

fn obs_arg<'a>(sub: &str, b: &'a KeyBinding) -> &'a str {
    if sub.is_empty() {
        b.value2.as_deref().unwrap_or("")
    } else {
        b.value.as_str()
    }
}

/// Resolve a binding for a device whose key table is `table`.
pub fn resolve(b: &KeyBinding, table: &[KeyRow], library: &[Macro]) -> ProtoResult<KeyOp> {
    let v = b.value.trim();
    Ok(match b.kind {
        BindingType::Default => KeyOp::Remap(REMAP_DEFAULT),
        BindingType::Disable => KeyOp::Remap(REMAP_DISABLE),
        BindingType::Shortcut => shortcut_op(v, table)?,
        BindingType::KeyboardFunction => {
            KeyOp::Remap(keyboard_function(v).ok_or_else(|| crate::ProtoError(format!("unknown keyboard function {v:?}")))?)
        }
        BindingType::Media => KeyOp::Remap(lookup(MEDIA, v).ok_or_else(|| crate::ProtoError(format!("unknown media action {v:?}")))?),
        BindingType::Mouse => match lookup(MOUSE, v) {
            Some(c) => KeyOp::Remap(c),
            // TODO(spec): horizontal wheel macro key codes are not specified.
            None if MOUSE_UNSPECIFIED.iter().any(|m| m.eq_ignore_ascii_case(v)) => {
                return err(format!("mouse action {v:?} is not supported yet (not in the specification)"))
            }
            None => return err(format!("unknown mouse action {v:?}")),
        },
        BindingType::Profile => KeyOp::Remap(lookup(PROFILE, v).ok_or_else(|| crate::ProtoError(format!("unknown profile action {v:?}")))?),
        BindingType::OsCommand => {
            if let Some(c) = lookup(OS_REMAP, v) {
                KeyOp::Remap(c)
            } else if v.eq_ignore_ascii_case("Lock computer") {
                KeyOp::Shortcut { key: 39, mods: crate::types::modifier::WIN }
            } else if v.eq_ignore_ascii_case("Run explorer") {
                KeyOp::Shortcut { key: 19, mods: crate::types::modifier::WIN }
            } else if v.eq_ignore_ascii_case("Task manager") {
                KeyOp::Macro { playback: PlaybackMode::Once.device_code(), events: macros::task_manager() }
            } else {
                return err(format!("unknown OS command {v:?}"));
            }
        }
        BindingType::Macro => {
            let m = library.iter().find(|m| m.name == v).ok_or_else(|| crate::ProtoError(format!("no macro named {v:?}")))?;
            KeyOp::Macro { playback: m.playback.device_code(), events: macros::encode(m, table)? }
        }
        BindingType::DisplayFolder | BindingType::DisplayBack | BindingType::Delay => KeyOp::None,
        _ => KeyOp::Launch(launch_payload(b).unwrap_or_default()),
    })
}

fn shortcut_op(v: &str, table: &[KeyRow]) -> ProtoResult<KeyOp> {
    let s = Shortcut::parse(v)?;
    let key = s.key.ok_or_else(|| crate::ProtoError("a shortcut needs a non-modifier key".into()))?;
    let fw = shortcut::fw_key(table, key).ok_or_else(|| crate::ProtoError(format!("key {} does not exist on this device", key.name)))?;
    Ok(if s.mods == 0 { KeyOp::Remap(fw) } else { KeyOp::Shortcut { key: fw, mods: s.mods } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    fn b(kind: BindingType, value: &str) -> KeyBinding {
        KeyBinding { fw_key: 110, kind, value: value.into(), ..Default::default() }
    }

    #[test]
    fn resolves() {
        let t = tables::everest_max_keys();
        assert_eq!(resolve(&b(BindingType::Default, ""), t, &[]).unwrap(), KeyOp::Remap(255));
        assert_eq!(resolve(&b(BindingType::Disable, ""), t, &[]).unwrap(), KeyOp::Remap(0));
        assert_eq!(resolve(&b(BindingType::Media, "Mute"), t, &[]).unwrap(), KeyOp::Remap(184));
        assert_eq!(resolve(&b(BindingType::Mouse, "Left"), t, &[]).unwrap(), KeyOp::Remap(257));
        assert_eq!(resolve(&b(BindingType::KeyboardFunction, "F24"), t, &[]).unwrap(), KeyOp::Remap(241));
        assert_eq!(resolve(&b(BindingType::Shortcut, "Win+L"), t, &[]).unwrap(), KeyOp::Shortcut { key: 39, mods: 8 });
        assert_eq!(resolve(&b(BindingType::Shortcut, "A"), t, &[]).unwrap(), KeyOp::Remap(31));
        assert_eq!(resolve(&b(BindingType::OsCommand, "Lock computer"), t, &[]).unwrap(), KeyOp::Shortcut { key: 39, mods: 8 });
        assert_eq!(resolve(&b(BindingType::OsCommand, "Run explorer"), t, &[]).unwrap(), KeyOp::Shortcut { key: 19, mods: 8 });
        assert_eq!(resolve(&b(BindingType::Profile, "Next"), t, &[]).unwrap(), KeyOp::Remap(212));
        assert!(resolve(&b(BindingType::Mouse, "Scroll left"), t, &[]).is_err());
        assert!(resolve(&b(BindingType::Macro, "missing"), t, &[]).is_err());
        assert_eq!(
            resolve(&b(BindingType::Url, "https://mountain.gg"), t, &[]).unwrap(),
            KeyOp::Launch([&[2u8][..], b"https://mountain.gg"].concat())
        );
        let mut hk = b(BindingType::HotkeySwitch, "Ctrl+C");
        hk.value2 = Some("Ctrl+V".into());
        assert_eq!(resolve(&hk, t, &[]).unwrap(), KeyOp::Launch([&[8u8][..], b"Ctrl+C~Ctrl+V"].concat()));
        let mut obs = b(BindingType::Obs, "Live");
        obs.sub_type = Some("Set Current Scene".into());
        assert_eq!(resolve(&obs, t, &[]).unwrap(), KeyOp::Launch([&[3u8][..], b"Set Current Scene~Live"].concat()));
    }

    #[test]
    fn task_manager_macro() {
        match resolve(&b(BindingType::OsCommand, "Task manager"), tables::everest_max_keys(), &[]).unwrap() {
            KeyOp::Macro { playback, events } => {
                assert_eq!(playback, 1);
                assert_eq!(events.iter().map(|e| e[0]).collect::<Vec<_>>(), vec![58, 44, 110, 110, 44, 58, 0xFF]);
            }
            other => panic!("{other:?}"),
        }
    }
}

//! Enums and small value types shared by all devices (spec 03 §1).

use serde::{Deserialize, Serialize};

/// 24-bit colour. Serialised as `"#rrggbb"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    pub fn bytes(self) -> [u8; 3] {
        [self.0, self.1, self.2]
    }

    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad colour {s:?}")))
    }
}

/// Firmware lighting effect id of the Everest Max family (`EffectIndex`).
pub mod effect {
    pub const STATIC: u8 = 0;
    pub const BREATHING: u8 = 1;
    pub const REACTIVE: u8 = 3;
    pub const COLOR_WAVE: u8 = 4;
    pub const REACTIVE_B: u8 = 5;
    pub const YETI: u8 = 6;
    pub const TORNADO: u8 = 7;
    pub const MATRIX: u8 = 9;
    pub const CUSTOM: u8 = 10;
    pub const REACTIVE_C: u8 = 11;
    pub const OFF: u8 = 12;
}

/// Firmware effect slot (`MenuIndex`), in UI order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum MenuIndex {
    Static = 0,
    ColorWave = 1,
    Tornado = 2,
    Breathing = 3,
    Reactive = 4,
    Matrix = 5,
    Custom = 6,
    Yeti = 7,
    Off = 8,
}

impl MenuIndex {
    pub const ALL: [MenuIndex; 9] = [
        MenuIndex::Static,
        MenuIndex::ColorWave,
        MenuIndex::Tornado,
        MenuIndex::Breathing,
        MenuIndex::Reactive,
        MenuIndex::Matrix,
        MenuIndex::Custom,
        MenuIndex::Yeti,
        MenuIndex::Off,
    ];

    pub fn from_u8(v: u8) -> Option<MenuIndex> {
        Self::ALL.get(v as usize).copied()
    }

    pub fn value(self) -> u8 {
        self as u8
    }

    /// The Everest Max effect id this menu entry uses.
    pub fn effect_index(self) -> u8 {
        match self {
            MenuIndex::Static => effect::STATIC,
            MenuIndex::ColorWave => effect::COLOR_WAVE,
            MenuIndex::Tornado => effect::TORNADO,
            MenuIndex::Breathing => effect::BREATHING,
            MenuIndex::Reactive => effect::REACTIVE,
            MenuIndex::Matrix => effect::MATRIX,
            MenuIndex::Custom => effect::CUSTOM,
            MenuIndex::Yeti => effect::YETI,
            MenuIndex::Off => effect::OFF,
        }
    }

    /// Everest 60 effect id (spec 10 §4.4).
    pub fn everest60_effect(self) -> u8 {
        match self {
            MenuIndex::Static => 1,
            MenuIndex::ColorWave => 2,
            MenuIndex::Tornado => 3,
            MenuIndex::Breathing => 4,
            MenuIndex::Reactive => 5,
            MenuIndex::Matrix => 6,
            MenuIndex::Custom => 7,
            MenuIndex::Yeti => 8,
            MenuIndex::Off => 9,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MenuIndex::Static => "Static",
            MenuIndex::ColorWave => "Color Wave",
            MenuIndex::Tornado => "Tornado",
            MenuIndex::Breathing => "Breathing",
            MenuIndex::Reactive => "Reactive",
            MenuIndex::Matrix => "Matrix",
            MenuIndex::Custom => "Custom",
            MenuIndex::Yeti => "YETI mode",
            MenuIndex::Off => "Off",
        }
    }
}

/// Colour type of a lighting entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorType {
    #[default]
    Single,
    Dual,
    Random,
}

/// Macro playback mode with its device type code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackMode {
    #[default]
    Once,
    Hold,
    Repeat,
}

impl PlaybackMode {
    pub fn device_code(self) -> u8 {
        match self {
            PlaybackMode::Once => 1,
            PlaybackMode::Hold => 4,
            PlaybackMode::Repeat => 3,
        }
    }
}

/// Macro delay mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DelayMode {
    #[default]
    Recorded,
    Custom,
    None,
}

/// Modifier mask bits.
pub mod modifier {
    pub const CTRL: u8 = 1;
    pub const SHIFT: u8 = 2;
    pub const ALT: u8 = 4;
    pub const WIN: u8 = 8;
}

/// Launch-content kind byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchKind {
    Program = 1,
    Url = 2,
    Obs = 3,
    Folder = 4,
    Twitch = 5,
    YouTube = 6,
    Spotify = 7,
    HotkeySwitch = 8,
    MultiAction = 9,
}

impl LaunchKind {
    pub fn from_u8(v: u8) -> Option<LaunchKind> {
        Some(match v {
            1 => LaunchKind::Program,
            2 => LaunchKind::Url,
            3 => LaunchKind::Obs,
            4 => LaunchKind::Folder,
            5 => LaunchKind::Twitch,
            6 => LaunchKind::YouTube,
            7 => LaunchKind::Spotify,
            8 => LaunchKind::HotkeySwitch,
            9 => LaunchKind::MultiAction,
            _ => return None,
        })
    }
}

/// Device families, by product id (spec 01 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum DeviceKind {
    #[serde(rename = "everest-max")]
    EverestMax,
    #[serde(rename = "everest-60")]
    Everest60,
    #[serde(rename = "macropad")]
    MacroPad,
    #[serde(rename = "displaypad")]
    DisplayPad,
    #[serde(rename = "makalu-67")]
    Makalu67,
}

impl DeviceKind {
    pub fn from_pid(pid: u16) -> Option<DeviceKind> {
        Some(match pid {
            0x0001 => DeviceKind::EverestMax,
            0x0003 | 0x0004 => DeviceKind::Makalu67,
            0x0005 | 0x0006 => DeviceKind::Everest60,
            0x0008 => DeviceKind::MacroPad,
            0x0009 => DeviceKind::DisplayPad,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            DeviceKind::EverestMax => "Everest Max",
            DeviceKind::Everest60 => "Everest 60",
            DeviceKind::MacroPad => "MacroPad",
            DeviceKind::DisplayPad => "DisplayPad",
            DeviceKind::Makalu67 => "Makalu 67",
        }
    }

    /// Several units of these kinds can be attached at once; each has its own profiles.
    pub fn multi_instance(self) -> bool {
        matches!(self, DeviceKind::MacroPad | DeviceKind::DisplayPad)
    }

    /// Identifier used in store keys and over IPC (same as the serde name).
    pub fn slug(self) -> &'static str {
        match self {
            DeviceKind::EverestMax => "everest-max",
            DeviceKind::Everest60 => "everest-60",
            DeviceKind::MacroPad => "macropad",
            DeviceKind::DisplayPad => "displaypad",
            DeviceKind::Makalu67 => "makalu-67",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_names_match_slugs() {
        for k in [DeviceKind::EverestMax, DeviceKind::Everest60, DeviceKind::MacroPad, DeviceKind::DisplayPad, DeviceKind::Makalu67] {
            assert_eq!(serde_json::to_value(k).unwrap(), serde_json::json!(k.slug()));
        }
    }
}

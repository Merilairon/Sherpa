//! Key and layout tables from `data/` (interoperability data, used as-is).

use std::sync::OnceLock;

use crate::types::DeviceKind;

/// One row of a key table (`*_keys.csv`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    /// `DLLKeyId`: firmware key code (remap/macro/launch source and target).
    pub fw_key: u16,
    /// `DLLMatrixIndex`: LED index (Everest Max) or event/LED id (Everest 60).
    pub matrix: u16,
    /// `KeyId`: Windows virtual-key code used by Base Camp's recorder.
    pub vk: u32,
    /// `KeyName` (US layout).
    pub name: String,
    /// `LayerType` (Everest 60 only; 3 = Fn layer).
    pub layer: Option<u8>,
}

/// One row of a layout table (`*_layout.csv`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRow {
    pub index: u16,
    pub hid_usage: u16,
    pub ui_row: u16,
    pub ui_col: u16,
    pub led: u16,
    pub fw_key: u16,
    pub label: String,
    pub sync_pos: Option<u16>,
}

impl LayoutRow {
    /// Rows that are device-generated events rather than physical keys.
    pub fn is_virtual(&self) -> bool {
        self.ui_row == 696 || (171..=180).contains(&self.index)
    }

    /// Physical key that should be drawn in the UI.
    pub fn is_drawable(&self) -> bool {
        !self.is_virtual() && self.ui_row != 0
    }
}

/// Minimal CSV field splitter: handles double-quoted fields and the `\\` escape used in
/// the tables for a literal backslash.
fn split_csv(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|f| if f == "\\\\" { "\\".to_string() } else { f }).collect()
}

fn rows(text: &str) -> impl Iterator<Item = Vec<String>> + '_ {
    text.lines().skip(1).map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty()).map(split_csv)
}

fn num(s: &str) -> u32 {
    s.trim().parse().unwrap_or(0)
}

pub fn parse_keys(text: &str) -> Vec<KeyRow> {
    rows(text)
        .map(|f| KeyRow {
            fw_key: num(&f[0]) as u16,
            matrix: num(&f[1]) as u16,
            vk: num(&f[2]),
            name: f[3].clone(),
            layer: f.get(4).and_then(|s| s.trim().parse().ok()),
        })
        .collect()
}

pub fn parse_layout(text: &str) -> Vec<LayoutRow> {
    rows(text)
        .map(|f| LayoutRow {
            index: num(&f[0]) as u16,
            hid_usage: num(&f[1]) as u16,
            ui_row: num(&f[2]) as u16,
            ui_col: num(&f[3]) as u16,
            led: num(&f[4]) as u16,
            fw_key: num(&f[5]) as u16,
            label: f[6].clone(),
            sync_pos: f.get(7).and_then(|s| s.trim().parse().ok()),
        })
        .collect()
}

macro_rules! table {
    ($fn:ident, $ty:ty, $parse:ident, $file:literal) => {
        pub fn $fn() -> &'static [$ty] {
            static CELL: OnceLock<Vec<$ty>> = OnceLock::new();
            CELL.get_or_init(|| $parse(include_str!(concat!("../../../data/", $file))))
        }
    };
}

table!(everest_max_keys, KeyRow, parse_keys, "everest_max_keys.csv");
table!(everest_max_layout, LayoutRow, parse_layout, "everest_max_layout.csv");
table!(macropad_layout, LayoutRow, parse_layout, "macropad_layout.csv");
table!(displaypad_layout, LayoutRow, parse_layout, "displaypad_layout.csv");
table!(everest60_keys, KeyRow, parse_keys, "everest60_keys.csv");
table!(everest60_fn_keys, KeyRow, parse_keys, "everest60_fn_keys.csv");

/// Layout table for a bitmap-event device.
pub fn layout_for(kind: DeviceKind) -> &'static [LayoutRow] {
    match kind {
        DeviceKind::EverestMax => everest_max_layout(),
        DeviceKind::MacroPad => macropad_layout(),
        DeviceKind::DisplayPad => displaypad_layout(),
        _ => &[],
    }
}

/// Key table used to translate VK codes to firmware keys for a device kind.
/// MacroPad/DisplayPad macros use the Everest Max table (spec 08 §2).
pub fn keys_for(kind: DeviceKind) -> &'static [KeyRow] {
    match kind {
        DeviceKind::Everest60 => everest60_keys(),
        _ => everest_max_keys(),
    }
}

/// Look up a layout row by HID usage (usage 0 is "no usage").
pub fn layout_by_usage(layout: &[LayoutRow], usage: u16) -> Option<&LayoutRow> {
    if usage == 0 {
        return None;
    }
    layout.iter().find(|r| r.hid_usage == usage)
}

/// Firmware key code for a Windows VK code, using the given key table.
pub fn fw_key_for_vk(keys: &[KeyRow], vk: u32) -> Option<u16> {
    keys.iter().find(|k| k.vk == vk).map(|k| k.fw_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_load() {
        assert_eq!(everest_max_keys().len(), 126);
        assert_eq!(everest_max_layout().len(), 180);
        assert_eq!(macropad_layout().len(), 12);
        assert_eq!(displaypad_layout(), macropad_layout());
        assert_eq!(everest60_keys().len(), 116);
        assert_eq!(everest60_fn_keys().len(), 116);
        assert!(everest60_fn_keys().iter().all(|k| k.layer == Some(3)));
    }

    #[test]
    fn quoting_and_backslash() {
        let bs = everest_max_keys().iter().find(|k| k.fw_key == 29).unwrap();
        assert_eq!(bs.name, "\\");
        let comma = everest60_keys().iter().find(|k| k.vk == 188).unwrap();
        assert_eq!(comma.name, ",");
        let esc = &everest_max_layout()[0];
        assert_eq!((esc.index, esc.hid_usage, esc.fw_key, esc.label.as_str()), (0, 41, 110, "Esc"));
    }

    #[test]
    fn usage_lookup() {
        let row = layout_by_usage(everest_max_layout(), 0x29).unwrap();
        assert_eq!(row.index, 0);
        let p2 = layout_by_usage(everest_max_layout(), 774).unwrap();
        assert_eq!(p2.index, 172);
        assert!(p2.is_virtual());
    }
}

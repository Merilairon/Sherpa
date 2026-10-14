//! Config store: the source of truth for profiles, lighting, bindings and macros
//! (spec 02 §2, 03 §3). One JSON file, written atomically.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use sherpa_core::model::{DockSettings, Macro, MouseButton, MouseProfile, Profile, Settings, PROFILE_SLOTS};
use sherpa_core::types::DeviceKind;

use crate::error::{Error, Result};

pub const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceConfig {
    pub kind: DeviceKind,
    pub name: String,
    pub profiles: Vec<Profile>,
    pub dock: DockSettings,
    /// Firmware layout id last read from the device.
    pub layout: u8,
    /// Last known device toggles (game mode, indicator LEDs, …), for the UI.
    pub modes: BTreeMap<String, u8>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        DeviceConfig::new(DeviceKind::EverestMax)
    }
}

/// Number of LEDs a device kind's custom lighting array holds.
pub fn led_count(kind: DeviceKind) -> usize {
    match kind {
        DeviceKind::EverestMax => 126,
        DeviceKind::MacroPad => 12,
        DeviceKind::Everest60 => 232,
        DeviceKind::DisplayPad | DeviceKind::Makalu67 => 0,
    }
}

impl DeviceConfig {
    pub fn new(kind: DeviceKind) -> Self {
        let profiles = (1..=PROFILE_SLOTS)
            .map(|slot| {
                let mut p = Profile::new(slot, led_count(kind));
                if kind == DeviceKind::Makalu67 {
                    p.mouse = Some(MouseProfile { buttons: MouseButton::defaults(), ..Default::default() });
                }
                p
            })
            .collect();
        DeviceConfig { kind, name: kind.name().to_string(), profiles, dock: DockSettings::default(), layout: 0, modes: BTreeMap::new() }
    }

    pub fn profile(&self, slot: u8) -> Result<&Profile> {
        self.profiles.iter().find(|p| p.slot == slot).ok_or_else(|| Error::NotFound(format!("profile {slot}")))
    }

    pub fn profile_mut(&mut self, slot: u8) -> Result<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.slot == slot).ok_or_else(|| Error::NotFound(format!("profile {slot}")))
    }

    pub fn selected(&self) -> &Profile {
        self.profiles.iter().find(|p| p.selected).unwrap_or(&self.profiles[0])
    }

    pub fn selected_slot(&self) -> u8 {
        self.selected().slot
    }

    /// Mark one slot selected (exactly one per device).
    pub fn select(&mut self, slot: u8) -> Result<()> {
        self.profile(slot)?;
        for p in &mut self.profiles {
            p.selected = p.slot == slot;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoreData {
    pub version: u32,
    pub settings: Settings,
    pub macros: Vec<Macro>,
    pub devices: BTreeMap<String, DeviceConfig>,
    /// Program/folder paths the user approved for launch from device content (05 §3).
    pub approved_launches: Vec<String>,
}

impl Default for StoreData {
    fn default() -> Self {
        StoreData {
            version: STORE_VERSION,
            settings: Settings::default(),
            macros: Vec::new(),
            devices: BTreeMap::new(),
            approved_launches: Vec::new(),
        }
    }
}

pub struct Store {
    path: Option<PathBuf>,
    pub data: StoreData,
}

/// Store key of a device: one per kind, or per unit for multi-instance kinds.
pub fn device_key(kind: DeviceKind, identity: &str) -> String {
    if kind.multi_instance() {
        format!("{}:{}", kind.slug(), identity)
    } else {
        kind.slug().to_string()
    }
}

impl Store {
    /// In-memory store (tests).
    pub fn memory() -> Store {
        Store { path: None, data: StoreData::default() }
    }

    pub fn load(path: &Path) -> Result<Store> {
        let data = match fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StoreData::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Store { path: Some(path.to_path_buf()), data })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Write to a temporary file, then rename over the store (atomic replace).
    pub fn save(&self) -> Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = open_private(&tmp)?;
            f.write_all(serde_json::to_string_pretty(&self.data)?.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn device(&self, key: &str) -> Result<&DeviceConfig> {
        self.data.devices.get(key).ok_or_else(|| Error::NotFound(format!("device {key}")))
    }

    pub fn device_mut(&mut self, key: &str) -> Result<&mut DeviceConfig> {
        self.data.devices.get_mut(key).ok_or_else(|| Error::NotFound(format!("device {key}")))
    }

    /// Get or create the config of a device.
    pub fn ensure_device(&mut self, key: &str, kind: DeviceKind) -> &mut DeviceConfig {
        self.data.devices.entry(key.to_string()).or_insert_with(|| DeviceConfig::new(kind))
    }

    pub fn macro_named(&self, name: &str) -> Option<&Macro> {
        self.data.macros.iter().find(|m| m.name == name)
    }
}

#[cfg(unix)]
fn open_private(p: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(p)
}

#[cfg(not(unix))]
fn open_private(p: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new().write(true).create(true).truncate(true).open(p)
}

/// `$XDG_CONFIG_HOME/sherpa` (Linux) or `%APPDATA%\sherpa` (Windows).
pub fn default_config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(d) = std::env::var_os("APPDATA") {
            return PathBuf::from(d).join("sherpa");
        }
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("sherpa");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".config").join("sherpa");
    }
    PathBuf::from(".").join("sherpa")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_atomic() {
        let dir = std::env::temp_dir().join(format!("bc-store-{}", std::process::id()));
        let path = dir.join("store.json");
        let mut s = Store::load(&path).unwrap();
        s.ensure_device("everest-max", DeviceKind::EverestMax).select(3).unwrap();
        s.data.macros.push(Macro { name: "M".into(), ..Default::default() });
        s.save().unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let t = Store::load(&path).unwrap();
        assert_eq!(t.data, s.data);
        assert_eq!(t.device("everest-max").unwrap().selected_slot(), 3);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keys() {
        assert_eq!(device_key(DeviceKind::EverestMax, "x"), "everest-max");
        assert_eq!(device_key(DeviceKind::MacroPad, "usb-1"), "macropad:usb-1");
    }
}

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
    /// Dock settings changed while detached, to apply on attach (D-1).
    pub dock_dirty: bool,
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
        DeviceConfig {
            kind,
            name: kind.name().to_string(),
            profiles,
            dock: DockSettings::default(),
            layout: 0,
            modes: BTreeMap::new(),
            dock_dirty: false,
        }
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

    /// Load the store. A file that cannot be parsed, or that a newer version wrote, is
    /// moved aside (never overwritten) and the last good backup (`store.json.bak`) or the
    /// defaults are used instead, so the daemon always starts.
    pub fn load(path: &Path) -> Result<Store> {
        let data = match read_store(path) {
            Ok(Some(d)) => d,
            Ok(None) => StoreData::default(),
            Err(LoadError::Io(e)) => return Err(e.into()),
            Err(LoadError::Unusable(reason)) => {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
                let aside = path.with_extension(format!("json.broken-{stamp}"));
                match fs::rename(path, &aside) {
                    Ok(()) => log::warn!("{}: {reason}; moved it to {}", path.display(), aside.display()),
                    Err(e) => return Err(Error::Io(format!("{}: {reason}, and it could not be moved aside: {e}", path.display()))),
                }
                match read_store(&backup_path(path)) {
                    Ok(Some(d)) => {
                        log::warn!("restored the configuration from {}", backup_path(path).display());
                        d
                    }
                    _ => {
                        log::warn!("starting with a default configuration");
                        StoreData::default()
                    }
                }
            }
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
        // Keep the previous good file as a backup for `load`.
        if path.exists() {
            let _ = fs::copy(path, backup_path(path));
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

enum LoadError {
    Io(std::io::Error),
    Unusable(String),
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// `Ok(None)` when the file does not exist.
fn read_store(path: &Path) -> std::result::Result<Option<StoreData>, LoadError> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => return Err(LoadError::Unusable(format!("not UTF-8 ({e})"))),
        Err(e) => return Err(LoadError::Io(e)),
    };
    let data: StoreData = serde_json::from_str(&text).map_err(|e| LoadError::Unusable(format!("unreadable ({e})")))?;
    if data.version > STORE_VERSION {
        return Err(LoadError::Unusable(format!("written by a newer version (format {} > {STORE_VERSION})", data.version)));
    }
    Ok(Some(data))
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
    fn corrupt_or_newer_store_is_moved_aside() {
        let dir = std::env::temp_dir().join(format!("bc-store-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store.json");
        let aside = || fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("broken")).count();

        // Truncated file, no backup: defaults, bad file kept.
        fs::write(&path, "{\"version\": 1, \"macros\": [").unwrap();
        let s = Store::load(&path).unwrap();
        assert_eq!(s.data, StoreData::default());
        assert_eq!(aside(), 1);

        // Two saves leave a backup of the first; a corrupt store then restores it.
        let mut s = Store::load(&path).unwrap();
        s.data.macros.push(Macro { name: "keep".into(), ..Default::default() });
        s.save().unwrap();
        s.save().unwrap();
        fs::write(&path, "garbage").unwrap();
        assert_eq!(Store::load(&path).unwrap().data.macros[0].name, "keep");

        // A newer format is never overwritten.
        let newer = serde_json::json!({"version": STORE_VERSION + 1}).to_string();
        fs::write(&path, &newer).unwrap();
        Store::load(&path).unwrap();
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|e| fs::read_to_string(e.path()).ok().as_deref() == Some(newer.as_str())));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keys() {
        assert_eq!(device_key(DeviceKind::EverestMax, "x"), "everest-max");
        assert_eq!(device_key(DeviceKind::MacroPad, "usb-1"), "macropad:usb-1");
    }
}

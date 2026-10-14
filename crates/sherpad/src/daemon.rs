//! The daemon core: device lifecycle, the config store and the API used by UIs.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use sherpa_core::binding::{self, KeyOp};
use sherpa_core::bitmap::KeyState;
use sherpa_core::displaypad as dp;
use sherpa_core::everest::{self as ev, ClockTime, ExtendInfo, Variant};
use sherpa_core::everest60 as e6;
use sherpa_core::makalu67 as mk;
use sherpa_core::model::{
    BindingType, CustomLighting, DisplayLayer, DockSettings, KeyBinding, Lighting, Macro, MouseButton, MouseLighting, MouseSensor, Profile,
    Settings,
};
use sherpa_core::tables::{self, LayoutRow};
use sherpa_core::types::{DeviceKind, MenuIndex};
use sherpa_core::Packet;

use crate::actions::Executor;
use crate::appwatch::{self, AppWatcher};
use crate::bus::Bus;
use crate::devices::displaypad::DisplayPadDev;
use crate::devices::everest::EverestDev;
use crate::devices::everest60::Everest60Dev;
use crate::devices::makalu::MakaluDev;
use crate::devices::{Backend, Found};
use crate::error::{invalid, Error, Result};
use crate::link::Link;
use crate::recorder::Recorder;
use crate::store::{DeviceConfig, Store};
use crate::sysmon::{self, SysMon};

/// Timing knobs (tests shrink them).
#[derive(Debug, Clone)]
pub struct Timing {
    pub scan: Duration,
    /// Clock sync after attach (F-DEV-3: about 10 s).
    pub clock_delay: Duration,
    /// Clock re-sync period (F-DCK-2: 30 min).
    pub clock_period: Duration,
    /// Volume / PC-info push period (1 s).
    pub feed: Duration,
    /// Fallback re-read of the dock page.
    pub dock_refresh: Duration,
    pub app_watch: Duration,
    pub e60_poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            scan: Duration::from_millis(1500),
            clock_delay: Duration::from_secs(10),
            clock_period: Duration::from_secs(30 * 60),
            feed: Duration::from_secs(1),
            dock_refresh: Duration::from_secs(10),
            app_watch: Duration::from_secs(2),
            e60_poll: Duration::from_millis(15),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub config_dir: Option<PathBuf>,
    pub timing: Timing,
    /// Enables test-only API such as `SimulateKey`.
    pub simulated: bool,
}

pub enum Driver {
    Everest(EverestDev),
    DisplayPad(DisplayPadDev),
    Everest60(Arc<Everest60Dev>),
    Makalu(Arc<MakaluDev>),
}

impl Driver {
    pub fn is_alive(&self) -> bool {
        match self {
            Driver::Everest(d) => d.link.is_alive(),
            Driver::DisplayPad(d) => d.cmd.link.is_alive(),
            Driver::Everest60(d) => d.is_alive(),
            Driver::Makalu(d) => d.is_alive(),
        }
    }

    pub fn close(&self) {
        match self {
            Driver::Everest(d) => d.link.close(),
            Driver::DisplayPad(d) => d.cmd.link.close(),
            Driver::Everest60(d) => d.close(),
            Driver::Makalu(d) => d.close(),
        }
    }

    /// The Everest-family command interface, when the device has one.
    pub fn everest(&self) -> Option<&EverestDev> {
        match self {
            Driver::Everest(d) => Some(d),
            Driver::DisplayPad(d) => Some(&d.cmd),
            _ => None,
        }
    }
}

/// Live state read from an attached device.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub fw: String,
    pub layout: u32,
    pub current_profile: u8,
    pub dock_side: String,
    pub numpad_side: String,
    pub dock_menu: u8,
    pub display_layer: u32,
    pub effect: u8,
    /// Firmware profiles disabled on the keyboard (Everest Max `showProfile` flag).
    pub disabled_profiles: Vec<u8>,
    #[serde(skip)]
    pub fn_held: bool,
}

impl Live {
    /// Update from an extended-info read.
    pub fn set_ext(&mut self, e: &ExtendInfo) {
        self.dock_side = e.dock_side().into();
        self.numpad_side = e.numpad_side().into();
        self.dock_menu = e.menu_index;
        self.disabled_profiles = e.disabled_profiles();
    }
}

pub struct Attached {
    pub key: String,
    pub kind: DeviceKind,
    pub pid: u16,
    pub identity: String,
    pub product: String,
    pub driver: Driver,
    pub live: Mutex<Live>,
    /// Keys whose multi-action is still running.
    pub busy: Mutex<HashSet<u16>>,
}

impl Attached {
    pub fn live(&self) -> MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn layout(&self) -> &'static [LayoutRow] {
        tables::layout_for(self.kind)
    }
}

pub struct Daemon {
    pub opts: Options,
    pub store: Mutex<Store>,
    pub bus: Bus,
    pub devices: RwLock<BTreeMap<String, Arc<Attached>>>,
    backend: Box<dyn Backend>,
    pub exec: Arc<dyn Executor>,
    obs_cfg: Option<Arc<crate::actions::system::SystemExecutor>>,
    pub(crate) apm: Mutex<VecDeque<Instant>>,
    recorder: Mutex<Option<Recorder>>,
    images: Mutex<HashMap<String, (Vec<u8>, String)>>,
    stopping: AtomicBool,
    me: Weak<Daemon>,
}

fn param<T: DeserializeOwned>(p: &Value, key: &str) -> Result<T> {
    serde_json::from_value(p.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| Error::Invalid(format!("parameter {key:?}: {e}")))
}

fn opt_param<T: DeserializeOwned>(p: &Value, key: &str) -> Result<Option<T>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| Error::Invalid(format!("parameter {key:?}: {e}"))),
    }
}

pub fn now_clock() -> ClockTime {
    use chrono::{Datelike, Timelike};
    let n = chrono::Local::now();
    ClockTime { month: n.month() as u8, day: n.day() as u8, hour: n.hour() as u8, minute: n.minute() as u8, second: n.second() as u8 }
}

/// Key table of a device kind, for binding resolution.
fn key_table(kind: DeviceKind) -> &'static [tables::KeyRow] {
    tables::keys_for(kind)
}

/// Does this kind store bindings on the device?
fn writes_bindings(kind: DeviceKind) -> bool {
    matches!(kind, DeviceKind::EverestMax | DeviceKind::MacroPad | DeviceKind::Everest60)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfileExport {
    format: String,
    version: u32,
    kind: DeviceKind,
    profile: Profile,
}

impl Daemon {
    pub fn new(
        opts: Options,
        backend: Box<dyn Backend>,
        exec: Arc<dyn Executor>,
        sys: Option<Arc<crate::actions::system::SystemExecutor>>,
    ) -> Result<Arc<Daemon>> {
        let store = match &opts.config_dir {
            Some(d) => Store::load(&d.join("store.json"))?,
            None => Store::memory(),
        };
        let d = Arc::new_cyclic(|me| Daemon {
            opts,
            store: Mutex::new(store),
            bus: Bus::default(),
            devices: RwLock::new(BTreeMap::new()),
            backend,
            exec,
            obs_cfg: sys,
            apm: Mutex::new(VecDeque::new()),
            recorder: Mutex::new(None),
            images: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
            me: me.clone(),
        });
        d.apply_settings();
        Ok(d)
    }

    pub fn arc(&self) -> Arc<Daemon> {
        self.me.upgrade().expect("daemon alive")
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start the background threads: device manager and app watcher.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        thread::Builder::new().name("devices".into()).spawn(move || me.device_loop()).unwrap();
        let me = self.clone();
        thread::Builder::new().name("appwatch".into()).spawn(move || me.app_watch_loop()).unwrap();
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let all: Vec<_> = self.devices.read().unwrap().values().cloned().collect();
        for a in all {
            a.driver.close();
        }
    }

    fn running(&self) -> bool {
        !self.stopping.load(Ordering::SeqCst)
    }

    pub fn attached(&self, key: &str) -> Option<Arc<Attached>> {
        self.devices.read().unwrap().get(key).cloned()
    }

    fn require(&self, key: &str) -> Result<Arc<Attached>> {
        self.attached(key).ok_or_else(|| Error::NotFound(format!("device {key} is not connected")))
    }

    fn apply_settings(&self) {
        if let Some(sys) = &self.obs_cfg {
            let s = self.store().data.settings.clone();
            *sys.obs.lock().unwrap() = crate::actions::obs::ObsConfig { port: s.obs_port, password: s.obs_password };
        }
    }

    // -----------------------------------------------------------------------------------
    // Device lifecycle

    /// One discovery pass (also used by tests to attach synchronously).
    pub fn scan_once(&self) {
        let attached: HashSet<String> = self.devices.read().unwrap().keys().cloned().collect();
        let (present, found) = self.backend.scan(&attached);
        for key in attached {
            let gone = !present.contains(&key) || self.attached(&key).is_none_or(|a| !a.driver.is_alive());
            if gone {
                self.detach(&key);
            }
        }
        for f in found {
            self.attach(f);
        }
    }

    fn device_loop(self: Arc<Self>) {
        while self.running() {
            self.scan_once();
            thread::sleep(self.opts.timing.scan);
        }
    }

    pub fn detach(&self, key: &str) {
        if let Some(a) = self.devices.write().unwrap().remove(key) {
            a.driver.close();
            log::info!("detached {key}");
            self.bus.device("DeviceDetached", key);
        }
    }

    fn attach(&self, f: Found) {
        log::info!("attaching {} ({})", f.kind.name(), f.key);
        let key = f.key.clone();
        let (driver, everest_events, e60, makalu_events) = match f.kind {
            DeviceKind::EverestMax | DeviceKind::MacroPad => {
                let (link, events) = Link::start(f.main, &key);
                let v = if f.kind == DeviceKind::EverestMax { Variant::EverestMax } else { Variant::MacroPad };
                (Driver::Everest(EverestDev::new(link, v)), Some(events), None, None)
            }
            DeviceKind::DisplayPad => {
                let (link, events) = Link::start(f.main, &key);
                (Driver::DisplayPad(DisplayPadDev::new(link, f.bulk)), Some(events), None, None)
            }
            DeviceKind::Everest60 => {
                let d = Arc::new(Everest60Dev::new(f.main));
                (Driver::Everest60(d.clone()), None, Some(d), None)
            }
            DeviceKind::Makalu67 => {
                let (d, rx) = MakaluDev::start(f.main);
                (Driver::Makalu(d), None, None, Some(rx))
            }
        };
        let a = Arc::new(Attached {
            key: key.clone(),
            kind: f.kind,
            pid: f.pid,
            identity: f.identity,
            product: f.product,
            driver,
            live: Mutex::new(Live::default()),
            busy: Mutex::new(HashSet::new()),
        });
        self.store().ensure_device(&key, f.kind);
        self.devices.write().unwrap().insert(key.clone(), a.clone());

        if let Err(e) = self.initial_sync(&a) {
            log::warn!("{key}: initial sync failed: {e}");
        }

        let me = self.arc();
        if let Some(events) = everest_events {
            let a2 = a.clone();
            let m2 = me.clone();
            thread::Builder::new().name(format!("events-{key}")).spawn(move || m2.everest_event_loop(a2, events)).unwrap();
        }
        if let Some(d) = e60 {
            let a2 = a.clone();
            let m2 = me.clone();
            thread::Builder::new().name(format!("poll-{key}")).spawn(move || m2.e60_loop(a2, d)).unwrap();
        }
        if let Some(rx) = makalu_events {
            let a2 = a.clone();
            let m2 = me.clone();
            thread::Builder::new().name(format!("events-{key}")).spawn(move || m2.makalu_loop(a2, rx)).unwrap();
        }
        if f.kind == DeviceKind::EverestMax {
            let a2 = a.clone();
            thread::Builder::new().name(format!("dock-{key}")).spawn(move || me.dock_loop(a2)).unwrap();
        }
        self.bus.device("DeviceAttached", &key);
    }

    /// Read identity, reconcile the active profile (device wins, D-3) and re-apply dirty
    /// or (optionally) all store state (D-1/D-2).
    fn initial_sync(&self, a: &Arc<Attached>) -> Result<()> {
        let device_slot = match &a.driver {
            Driver::Everest(d) | Driver::DisplayPad(DisplayPadDev { cmd: d, .. }) => {
                let fw = d.fw_info()?;
                let layout = d.layout().unwrap_or(0);
                let mut l = a.live();
                l.fw = fw.fw_string.clone();
                l.layout = layout as u32;
                l.current_profile = fw.current_profile;
                drop(l);
                if a.kind == DeviceKind::EverestMax {
                    if let Ok(e) = d.ext_info() {
                        a.live().set_ext(&e);
                    }
                }
                self.store().device_mut(&a.key)?.layout = layout;
                fw.current_profile
            }
            Driver::Everest60(d) => {
                let fw = d.fw_info()?;
                let mut l = a.live();
                l.fw = format!("{:x}", fw.fw_version);
                l.layout = d.layout().unwrap_or(0);
                l.current_profile = fw.current_profile as u8;
                fw.current_profile as u8
            }
            Driver::Makalu(d) => {
                let fw = d.firmware().unwrap_or(0);
                let p = d.current_profile()?;
                let mut l = a.live();
                l.fw = format!("{fw:x}");
                l.current_profile = p;
                p
            }
        };
        if (1..=5).contains(&device_slot) {
            let mut st = self.store();
            let cfg = st.device_mut(&a.key)?;
            if cfg.selected_slot() != device_slot {
                cfg.select(device_slot)?;
                st.save()?;
                drop(st);
                self.bus.emit("ProfileChangedOnDevice", json!({"id": a.key, "slot": device_slot}));
            }
        }
        let (dirty, sync_all, selected) = {
            let st = self.store();
            let cfg = st.device(&a.key)?;
            let dirty: Vec<u8> = cfg.profiles.iter().filter(|p| p.dirty).map(|p| p.slot).collect();
            (dirty, st.data.settings.sync_on_attach, cfg.selected_slot())
        };
        let mut slots = dirty;
        if sync_all && !slots.contains(&selected) {
            slots.push(selected);
        }
        for slot in slots {
            match self.full_apply(a, slot) {
                Ok(()) => {
                    let mut st = self.store();
                    if let Ok(p) = st.device_mut(&a.key).and_then(|c| c.profile_mut(slot)) {
                        p.dirty = false;
                    }
                    st.save()?;
                }
                Err(e) => log::warn!("{}: re-applying profile {slot} failed: {e}", a.key),
            }
        }
        if a.kind == DeviceKind::DisplayPad {
            if let Driver::DisplayPad(d) = &a.driver {
                let black = dp::solid(dp::PANEL_W, dp::PANEL_H, 0, 0, 0);
                if let Err(e) = d.panel(&black) {
                    log::warn!("{}: panel init failed: {e}", a.key);
                }
            }
            let slot = self.store().device(&a.key)?.selected_slot();
            let _ = a.driver.everest().map(|d| d.switch_profile(slot, MenuIndex::Static));
            self.draw_layer(a, 0)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // Low-level device operations

    /// Run `f` with `slot` active on the device, switching back afterwards if needed.
    fn with_slot<T>(&self, a: &Attached, slot: u8, f: impl FnOnce() -> Result<T>) -> Result<T> {
        let (current, cur_menu, menu) = {
            let st = self.store();
            let cfg = st.device(&a.key)?;
            (cfg.selected_slot(), cfg.selected().active_menu(), cfg.profile(slot)?.active_menu())
        };
        // The keyboard ACKs writes to a disabled profile, but drops them or stores them
        // over another profile's key data. Also when it is already on that profile.
        if a.live().disabled_profiles.contains(&slot) {
            return Err(Error::ProfileDisabled(slot));
        }
        if current == slot || matches!(a.driver, Driver::Makalu(_)) {
            return f();
        }
        self.switch_device_profile(a, slot, menu)?;
        let r = f();
        let back = self.switch_device_profile(a, current, cur_menu);
        let v = r?;
        back?;
        Ok(v)
    }

    fn switch_device_profile(&self, a: &Attached, slot: u8, menu: MenuIndex) -> Result<()> {
        match &a.driver {
            Driver::Everest(d) | Driver::DisplayPad(DisplayPadDev { cmd: d, .. }) => d.switch_profile(slot, menu)?,
            Driver::Everest60(d) => d.switch_profile(slot, menu.value())?,
            Driver::Makalu(d) => {
                d.exec(&mk::set_current_profile(slot))?;
            }
        }
        a.live().current_profile = slot;
        Ok(())
    }

    fn device_lighting(&self, a: &Attached, l: &Lighting, custom: &CustomLighting) -> Result<()> {
        match &a.driver {
            Driver::Everest(d) => d.apply_lighting(l, custom, true),
            Driver::Everest60(d) => d.apply_lighting(l, true),
            _ => Ok(()),
        }
    }

    fn device_key(&self, a: &Attached, b: &KeyBinding, op: &KeyOp) -> Result<()> {
        match &a.driver {
            Driver::Everest(d) => d.apply_key(b.fw_key, op),
            Driver::Everest60(d) => d.apply_key(b.layer == 3, b.fw_key, op),
            _ => Ok(()),
        }
    }

    /// Write a whole profile to the device (lighting, bindings, mouse settings).
    fn full_apply(&self, a: &Attached, slot: u8) -> Result<()> {
        let (profile, macros) = {
            let st = self.store();
            (st.device(&a.key)?.profile(slot)?.clone(), st.data.macros.clone())
        };
        if let Driver::Makalu(d) = &a.driver {
            if let Some(m) = &profile.mouse {
                d.exec(&mk::sensor_block(slot, &m.sensor)?)?;
                d.exec(&mk::lighting_saved(slot, &m.lighting)?)?;
                for b in &m.buttons {
                    d.set_button(slot, b.button, &mk::button_record(b, &macros)?)?;
                }
            }
            return Ok(());
        }
        if !writes_bindings(a.kind) {
            return Ok(());
        }
        self.with_slot(a, slot, || {
            if let Some(l) = profile.active_lighting() {
                self.device_lighting(a, l, &profile.custom)?;
            }
            for b in &profile.bindings {
                match binding::resolve(b, key_table(a.kind), &macros) {
                    Ok(op) => self.device_key(a, b, &op)?,
                    Err(e) => log::warn!("{}: skipping binding of key {}: {e}", a.key, b.led),
                }
            }
            Ok(())
        })
    }

    // -----------------------------------------------------------------------------------
    // Store editing (B2: on a device error the store is left unchanged)

    /// Edit one profile. `device` runs when the device is attached (with the profile made
    /// active if needed). On device failure the store is untouched, except for a
    /// disconnect, which keeps the change and marks the profile dirty (D-1).
    fn edit_profile(
        &self,
        dev: &str,
        slot: u8,
        edit: impl Fn(&mut Profile) -> Result<()>,
        device: impl FnOnce(&Attached, &Profile, &[Macro]) -> Result<()>,
    ) -> Result<()> {
        let (preview, macros, kind) = {
            let st = self.store();
            let cfg = st.device(dev)?;
            let mut p = cfg.profile(slot)?.clone();
            edit(&mut p)?;
            (p, st.data.macros.clone(), cfg.kind)
        };
        let mut dirty = false;
        match self.attached(dev) {
            Some(a) => match self.with_slot(&a, slot, || device(&a, &preview, &macros)) {
                Ok(()) => {}
                // Kept and re-applied on attach or when the profile is enabled.
                Err(Error::Disconnected | Error::ProfileDisabled(_)) => dirty = true,
                Err(e) => return Err(e),
            },
            None => dirty = writes_bindings(kind) || kind == DeviceKind::Makalu67,
        }
        {
            let mut st = self.store();
            let p = st.device_mut(dev)?.profile_mut(slot)?;
            edit(p)?;
            if dirty {
                p.dirty = true;
            }
            st.save()?;
        }
        self.bus.emit("StoreChanged", json!({"id": dev, "slot": slot}));
        Ok(())
    }

    /// Copy a change to every other profile of the device (F-PRO-7).
    fn sync_to_other_profiles(&self, dev: &str, slot: u8, edit: impl Fn(&mut Profile)) -> Result<()> {
        let others: Vec<u8> = self.store().device(dev)?.profiles.iter().map(|p| p.slot).filter(|s| *s != slot).collect();
        let attached = self.attached(dev);
        for s in others {
            let res = match &attached {
                Some(a) => {
                    let mut p = self.store().device(dev)?.profile(s)?.clone();
                    edit(&mut p);
                    let macros = self.store().data.macros.clone();
                    self.with_slot(a, s, || self.write_profile_delta(a, &p, &macros))
                }
                None => Err(Error::Disconnected),
            };
            let mut st = self.store();
            let p = st.device_mut(dev)?.profile_mut(s)?;
            edit(p);
            if res.is_err() {
                p.dirty = true;
            }
            st.save()?;
        }
        Ok(())
    }

    /// Re-write the active lighting and all bindings of a profile.
    fn write_profile_delta(&self, a: &Attached, p: &Profile, macros: &[Macro]) -> Result<()> {
        if let Some(l) = p.active_lighting() {
            self.device_lighting(a, l, &p.custom)?;
        }
        for b in &p.bindings {
            if let Ok(op) = binding::resolve(b, key_table(a.kind), macros) {
                self.device_key(a, b, &op)?;
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // API

    pub fn list_devices(&self) -> Value {
        let st = self.store();
        let only_connected = st.data.settings.display_only_connected_devices;
        let devs = self.devices.read().unwrap();
        let mut out = Vec::new();
        let mut entries: Vec<_> = st.data.devices.iter().collect();
        entries.sort_by_key(|(k, c)| (c.kind, (*k).clone()));
        for (key, cfg) in entries {
            let a = devs.get(key);
            if only_connected && a.is_none() {
                continue;
            }
            out.push(json!({
                "id": key,
                "kind": cfg.kind,
                "name": cfg.name,
                "pid": a.map(|a| a.pid),
                "connected": a.is_some(),
                "product": a.map(|a| a.product.clone()),
                "live": a.map(|a| serde_json::to_value(&*a.live()).unwrap()),
                "selectedSlot": cfg.selected_slot(),
            }));
        }
        Value::Array(out)
    }

    pub fn get_device(&self, dev: &str) -> Result<Value> {
        let st = self.store();
        let cfg = st.device(dev)?;
        let a = self.attached(dev);
        Ok(json!({
            "id": dev,
            "kind": cfg.kind,
            "connected": a.is_some(),
            "live": a.map(|a| serde_json::to_value(&*a.live()).unwrap()),
            "config": cfg,
        }))
    }

    pub fn get_layout(&self, dev: &str) -> Result<Value> {
        let kind = self.store().device(dev)?.kind;
        Ok(match kind {
            DeviceKind::EverestMax | DeviceKind::MacroPad | DeviceKind::DisplayPad => {
                let rows: Vec<Value> = tables::layout_for(kind)
                    .iter()
                    .filter(|r| r.is_drawable() || kind != DeviceKind::EverestMax)
                    .map(|r| json!({"index": r.index, "led": r.led, "fwKey": r.fw_key, "label": r.label, "row": r.ui_row, "col": r.ui_col}))
                    .collect();
                let cols = match kind {
                    DeviceKind::DisplayPad => 6,
                    DeviceKind::MacroPad => 4,
                    _ => 0,
                };
                json!({"kind": kind, "keys": rows, "gridColumns": cols})
            }
            DeviceKind::Everest60 => {
                let base: Vec<Value> = tables::everest60_keys()
                    .iter()
                    .map(|k| json!({"led": k.matrix, "fwKey": k.fw_key, "label": k.name, "layer": 0}))
                    .collect();
                let fnl: Vec<Value> = tables::everest60_fn_keys()
                    .iter()
                    .map(|k| json!({"led": k.matrix, "fwKey": k.fw_key, "label": k.name, "layer": 3}))
                    .collect();
                json!({"kind": kind, "keys": base, "fnKeys": fnl, "gridColumns": 15})
            }
            DeviceKind::Makalu67 => {
                let names = ["Left", "Right", "Middle", "Back", "Forward", "DPI"];
                let rows: Vec<Value> = names.iter().enumerate().map(|(i, n)| json!({"button": i + 1, "label": n})).collect();
                json!({"kind": kind, "buttons": rows})
            }
        })
    }

    pub fn select_profile(&self, dev: &str, slot: u8) -> Result<()> {
        let (menu, dirty) = {
            let st = self.store();
            let p = st.device(dev)?.profile(slot)?;
            (p.active_menu(), p.dirty)
        };
        if let Some(a) = self.attached(dev) {
            if a.live().disabled_profiles.contains(&slot) {
                return Err(Error::ProfileDisabled(slot));
            }
            self.switch_device_profile(&a, slot, menu)?;
            if dirty && self.full_apply(&a, slot).is_ok() {
                self.store().device_mut(dev)?.profile_mut(slot)?.dirty = false;
            }
        }
        {
            let mut st = self.store();
            st.device_mut(dev)?.select(slot)?;
            st.save()?;
        }
        if let Some(a) = self.attached(dev) {
            if a.kind == DeviceKind::DisplayPad {
                self.draw_layer(&a, 0)?;
            }
        }
        self.bus.emit("ProfileChanged", json!({"id": dev, "slot": slot}));
        Ok(())
    }

    pub fn update_profile(&self, dev: &str, slot: u8, p: &Value) -> Result<()> {
        {
            let mut st = self.store();
            let prof = st.device_mut(dev)?.profile_mut(slot)?;
            if let Some(n) = opt_param::<String>(p, "name")? {
                prof.name = n;
            }
            if p.get("linkedExe").is_some() {
                prof.linked_exe = opt_param::<String>(p, "linkedExe")?.filter(|s| !s.trim().is_empty());
            }
            if let Some(r) = opt_param::<bool>(p, "revertOnExit")? {
                prof.revert_on_exit = r;
            }
            if p.get("image").is_some() {
                prof.image = opt_param(p, "image")?;
            }
            if let Some(o) = opt_param::<u32>(p, "order")? {
                prof.order = o;
            }
            st.save()?;
        }
        self.bus.emit("StoreChanged", json!({"id": dev, "slot": slot}));
        Ok(())
    }

    /// Reset a profile to defaults (keeps name, slot, links).
    pub fn reset_profile(&self, dev: &str, slot: u8) -> Result<()> {
        let kind = self.store().device(dev)?.kind;
        let fresh = DeviceConfig::new(kind).profile(slot)?.clone();
        let old_bindings = self.store().device(dev)?.profile(slot)?.bindings.clone();
        self.edit_profile(
            dev,
            slot,
            |p| {
                p.lighting = fresh.lighting.clone();
                p.custom = fresh.custom.clone();
                p.bindings.clear();
                p.mouse = fresh.mouse.clone();
                p.layers = fresh.layers.clone();
                Ok(())
            },
            |a, p, _| {
                for b in &old_bindings {
                    self.device_key(a, b, &KeyOp::Remap(binding::REMAP_DEFAULT))?;
                }
                if let Some(l) = p.active_lighting() {
                    self.device_lighting(a, l, &p.custom)?;
                }
                Ok(())
            },
        )
    }

    /// Copy the content of one profile into another (duplicate).
    pub fn copy_profile(&self, dev: &str, from: u8, to: u8) -> Result<()> {
        let src = self.store().device(dev)?.profile(from)?.clone();
        self.replace_profile_content(dev, to, &src)
    }

    fn replace_profile_content(&self, dev: &str, slot: u8, src: &Profile) -> Result<()> {
        self.edit_profile(
            dev,
            slot,
            |p| {
                p.lighting = src.lighting.clone();
                p.custom = src.custom.clone();
                p.bindings = src.bindings.clone();
                p.mouse = src.mouse.clone();
                p.layers = src.layers.clone();
                Ok(())
            },
            |a, p, macros| self.write_profile_delta(a, p, macros),
        )
    }

    pub fn export_profile(&self, dev: &str, slot: u8) -> Result<Value> {
        let st = self.store();
        let cfg = st.device(dev)?;
        Ok(serde_json::to_value(ProfileExport {
            format: "sherpa-profile".into(),
            version: 1,
            kind: cfg.kind,
            profile: cfg.profile(slot)?.clone(),
        })?)
    }

    pub fn import_profile(&self, dev: &str, slot: u8, data: Value) -> Result<()> {
        let x: ProfileExport = serde_json::from_value(data)?;
        if x.format != "sherpa-profile" {
            return invalid("not a profile export");
        }
        let kind = self.store().device(dev)?.kind;
        if x.kind != kind {
            return invalid(format!("profile is for {}, not {}", x.kind.name(), kind.name()));
        }
        self.replace_profile_content(dev, slot, &x.profile)?;
        let mut st = self.store();
        st.device_mut(dev)?.profile_mut(slot)?.name = x.profile.name;
        st.save()
    }

    pub fn set_lighting(&self, dev: &str, slot: u8, l: Lighting, activate: bool) -> Result<()> {
        if let Some(a) = self.attached(dev) {
            if a.kind == DeviceKind::EverestMax || a.kind == DeviceKind::MacroPad {
                ev::lighting_packet(&l)?; // validate before touching anything
            }
        }
        let menu = l.menu_index;
        let edit = |p: &mut Profile| {
            let was_active = p.lighting.iter().any(|x| x.menu_index == menu && x.active);
            let mut nl = l.clone();
            nl.active = activate || was_active;
            if let Some(slot) = p.lighting.iter_mut().find(|x| x.menu_index == menu) {
                *slot = nl;
            } else {
                p.lighting.push(nl);
            }
            if activate {
                for x in &mut p.lighting {
                    x.active = x.menu_index == menu;
                }
            }
            Ok(())
        };
        self.edit_profile(dev, slot, edit, |a, p, _| match p.lighting.iter().find(|x| x.menu_index == menu && x.active) {
            Some(x) => self.device_lighting(a, x, &p.custom),
            None => Ok(()),
        })?;
        if l.sync_across_profiles {
            self.sync_to_other_profiles(dev, slot, |p| {
                let _ = edit(p);
            })?;
        }
        Ok(())
    }

    /// Live preview: send without saving.
    pub fn preview_lighting(&self, dev: &str, l: &Lighting) -> Result<()> {
        let a = self.require(dev)?;
        let current = a.live().current_profile;
        if a.live().disabled_profiles.contains(&current) {
            return Err(Error::ProfileDisabled(current));
        }
        match &a.driver {
            Driver::Everest(d) => {
                let custom = self.store().device(dev)?.selected().custom.clone();
                d.apply_lighting(l, &custom, false)
            }
            Driver::Everest60(d) => d.apply_lighting(l, false),
            _ => Err(Error::Unsupported("lighting preview".into())),
        }
    }

    pub fn set_custom(&self, dev: &str, slot: u8, custom: CustomLighting, activate: bool) -> Result<()> {
        let n = crate::store::led_count(self.store().device(dev)?.kind);
        let mut custom = custom;
        custom.leds.resize(n, sherpa_core::types::Rgb::BLACK);
        custom.per_led_effect.resize(n, 0);
        self.edit_profile(
            dev,
            slot,
            |p| {
                p.custom = custom.clone();
                if activate {
                    for x in &mut p.lighting {
                        x.active = x.menu_index == MenuIndex::Custom;
                    }
                }
                Ok(())
            },
            |a, p, _| match p.active_lighting() {
                Some(l) if l.menu_index == MenuIndex::Custom => self.device_lighting(a, l, &p.custom),
                _ => Ok(()),
            },
        )
    }

    pub fn set_binding(&self, dev: &str, slot: u8, mut b: KeyBinding) -> Result<()> {
        let kind = self.store().device(dev)?.kind;
        if kind == DeviceKind::Makalu67 {
            return invalid("use SetMouseButton for the Makalu 67");
        }
        if b.fw_key == 0 {
            b.fw_key = self.fw_key_for(kind, b.led, b.layer).unwrap_or(0);
        }
        let macros = self.store().data.macros.clone();
        // Validate (and fail early) for bindings stored on the device.
        let op = if writes_bindings(kind) { binding::resolve(&b, key_table(kind), &macros)? } else { KeyOp::None };
        if b.kind == BindingType::MultiAction {
            serde_json::from_str::<Vec<sherpa_core::model::MultiStep>>(&b.value)
                .map_err(|e| Error::Invalid(format!("multi-action steps: {e}")))?;
        }
        let b2 = b.clone();
        self.edit_profile(
            dev,
            slot,
            |p| {
                let mut nb = b2.clone();
                // Keep the hotkey-switch runtime state.
                if let Some(old) = p.binding(nb.led, nb.layer, nb.layer_id) {
                    if old.kind == nb.kind {
                        nb.toggle_state = old.toggle_state;
                    }
                }
                p.put_binding(nb);
                Ok(())
            },
            |a, _, _| self.device_key(a, &b, &op),
        )?;
        if b.sync_across_profiles {
            let b3 = b.clone();
            self.sync_to_other_profiles(dev, slot, |p| p.put_binding(b3.clone()))?;
        }
        if kind == DeviceKind::DisplayPad {
            if let Some(a) = self.attached(dev) {
                let layer = a.live().display_layer;
                if layer == b.layer_id && self.store().device(dev)?.selected_slot() == slot {
                    self.draw_layer(&a, layer)?;
                }
            }
        }
        Ok(())
    }

    fn fw_key_for(&self, kind: DeviceKind, led: u16, layer: u8) -> Option<u16> {
        match kind {
            DeviceKind::Everest60 => {
                let t = if layer == 3 { tables::everest60_fn_keys() } else { tables::everest60_keys() };
                t.iter().find(|k| k.matrix == led).map(|k| k.fw_key)
            }
            _ => tables::layout_for(kind).iter().find(|r| r.led == led && r.fw_key != 0).map(|r| r.fw_key),
        }
    }

    pub fn clear_binding(&self, dev: &str, slot: u8, led: u16, layer: u8, layer_id: u32) -> Result<()> {
        let existing = self.store().device(dev)?.profile(slot)?.binding(led, layer, layer_id).cloned();
        let Some(old) = existing else { return Ok(()) };
        let kind = self.store().device(dev)?.kind;
        self.edit_profile(
            dev,
            slot,
            |p| {
                p.bindings.retain(|b| !(b.led == led && b.layer == layer && b.layer_id == layer_id));
                Ok(())
            },
            |a, _, _| {
                if writes_bindings(kind) {
                    self.device_key(a, &old, &KeyOp::Remap(binding::REMAP_DEFAULT))
                } else {
                    Ok(())
                }
            },
        )?;
        if kind == DeviceKind::DisplayPad {
            if let Some(a) = self.attached(dev) {
                let layer = a.live().display_layer;
                self.draw_layer(&a, layer)?;
            }
        }
        Ok(())
    }

    // Macros ------------------------------------------------------------------------------

    pub fn put_macro(&self, mut m: Macro, old_name: Option<String>) -> Result<()> {
        m.name = m.name.trim().to_string();
        if m.name.is_empty() {
            return invalid("a macro needs a name");
        }
        m.normalize();
        if m.events.len() + 1 > sherpa_core::macros::MAX_EVENTS {
            return invalid("a macro holds at most 999 events");
        }
        let renamed_from = old_name.filter(|o| *o != m.name);
        {
            let mut st = self.store();
            if st.data.macros.iter().any(|x| x.name == m.name) && renamed_from.is_some() {
                return invalid(format!("a macro named {:?} already exists", m.name));
            }
            if let Some(old) = &renamed_from {
                st.data.macros.retain(|x| x.name != *old);
                for cfg in st.data.devices.values_mut() {
                    for p in &mut cfg.profiles {
                        for b in &mut p.bindings {
                            if b.kind == BindingType::Macro && b.value == *old {
                                b.value = m.name.clone();
                            }
                        }
                        if let Some(mouse) = &mut p.mouse {
                            for b in &mut mouse.buttons {
                                if b.kind == "macro" && b.value == *old {
                                    b.value = m.name.clone();
                                }
                            }
                        }
                    }
                }
            }
            if m.order == 0 {
                m.order = st.data.macros.iter().map(|x| x.order).max().unwrap_or(0) + 1;
            }
            match st.data.macros.iter_mut().find(|x| x.name == m.name) {
                Some(x) => *x = m.clone(),
                None => st.data.macros.push(m.clone()),
            }
            st.save()?;
        }
        self.bus.emit("MacrosChanged", json!({}));
        self.rewrite_macro_keys(&m.name)
    }

    /// F-MAC-7: rewrite the macro onto every key bound to it, in every profile and device.
    fn rewrite_macro_keys(&self, name: &str) -> Result<()> {
        let targets: Vec<(String, u8, Vec<KeyBinding>, Vec<MouseButton>)> = {
            let st = self.store();
            st.data
                .devices
                .iter()
                .flat_map(|(k, cfg)| {
                    cfg.profiles.iter().filter_map(move |p| {
                        let kb: Vec<KeyBinding> =
                            p.bindings.iter().filter(|b| b.kind == BindingType::Macro && b.value == name).cloned().collect();
                        let mb: Vec<MouseButton> = p
                            .mouse
                            .iter()
                            .flat_map(|m| m.buttons.iter())
                            .filter(|b| b.kind == "macro" && b.value == name)
                            .cloned()
                            .collect();
                        (!kb.is_empty() || !mb.is_empty()).then(|| (k.clone(), p.slot, kb, mb))
                    })
                })
                .collect()
        };
        let macros = self.store().data.macros.clone();
        let mut first_err = None;
        for (dev, slot, keys, buttons) in targets {
            let res = match self.attached(&dev) {
                Some(a) => match &a.driver {
                    Driver::Makalu(d) => buttons.iter().try_for_each(|b| d.set_button(slot, b.button, &mk::button_record(b, &macros)?)),
                    _ if writes_bindings(a.kind) => self.with_slot(&a, slot, || {
                        for b in &keys {
                            let op = binding::resolve(b, key_table(a.kind), &macros)?;
                            self.device_key(&a, b, &op)?;
                        }
                        Ok(())
                    }),
                    _ => Ok(()),
                },
                None => Err(Error::Disconnected),
            };
            if let Err(e) = res {
                let mut st = self.store();
                if let Ok(p) = st.device_mut(&dev).and_then(|c| c.profile_mut(slot)) {
                    p.dirty = true;
                }
                st.save()?;
                if e != Error::Disconnected && first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    pub fn delete_macro(&self, name: &str) -> Result<()> {
        let users: Vec<(String, u8, KeyBinding)> = {
            let st = self.store();
            let mut v = Vec::new();
            for (k, cfg) in &st.data.devices {
                for p in &cfg.profiles {
                    for b in p.bindings.iter().filter(|b| b.kind == BindingType::Macro && b.value == name) {
                        v.push((k.clone(), p.slot, b.clone()));
                    }
                }
            }
            v
        };
        for (dev, slot, b) in users {
            self.clear_binding(&dev, slot, b.led, b.layer, b.layer_id)?;
        }
        let mut st = self.store();
        st.data.macros.retain(|m| m.name != name);
        st.save()?;
        drop(st);
        self.bus.emit("MacrosChanged", json!({}));
        Ok(())
    }

    pub fn import_macros(&self, data: Value) -> Result<usize> {
        let list = match &data {
            Value::Array(_) => data.clone(),
            Value::Object(o) => o.get("macros").cloned().ok_or_else(|| Error::Invalid("no macros in file".into()))?,
            _ => return invalid("not a macro export"),
        };
        let macros: Vec<Macro> = serde_json::from_value(list)?;
        let n = macros.len();
        for mut m in macros {
            let base = m.name.clone();
            let mut i = 2;
            while self.store().macro_named(&m.name).is_some() {
                m.name = format!("{base} ({i})");
                i += 1;
            }
            m.order = 0;
            self.put_macro(m, None)?;
        }
        Ok(n)
    }

    pub fn start_recording(&self, keyboard: bool, mouse: bool) -> Result<()> {
        let mut r = self.recorder.lock().unwrap();
        if r.is_some() {
            return invalid("already recording");
        }
        *r = Some(Recorder::start(keyboard, mouse)?);
        Ok(())
    }

    pub fn stop_recording(&self) -> Result<Value> {
        let r = self.recorder.lock().unwrap().take().ok_or_else(|| Error::Invalid("not recording".into()))?;
        Ok(serde_json::to_value(r.stop())?)
    }

    // Device settings ---------------------------------------------------------------------

    pub fn set_dock(&self, dev: &str, dock: DockSettings) -> Result<()> {
        let mut e = ExtendInfo::from_dock(&dock);
        e.validate()?;
        if let Some(a) = self.attached(dev) {
            if let Driver::Everest(d) = &a.driver {
                // The dock settings do not say which profiles are enabled: keep the device's.
                if a.kind == DeviceKind::EverestMax {
                    e.keep_profile_flags(&d.ext_info()?);
                }
                d.set_ext_info(&e)?;
                if a.kind == DeviceKind::EverestMax {
                    a.live().disabled_profiles = e.disabled_profiles();
                }
                d.clock_sync(now_clock(), Some((dock.clock_digital, dock.clock12h)))?;
            }
        }
        let mut st = self.store();
        st.device_mut(dev)?.dock = dock;
        st.save()?;
        drop(st);
        self.bus.emit("StoreChanged", json!({"id": dev}));
        Ok(())
    }

    /// Enable or disable a firmware profile on the keyboard (Everest Max `showProfile`
    /// flag). Edits made while it was disabled are applied when it is enabled.
    pub fn set_profile_enabled(&self, dev: &str, slot: u8, enabled: bool) -> Result<()> {
        let a = self.require(dev)?;
        let d = match &a.driver {
            Driver::Everest(d) if a.kind == DeviceKind::EverestMax => d,
            _ => return Err(Error::Unsupported("enabling profiles on this device".into())),
        };
        if !enabled && a.live().current_profile == slot {
            return invalid(format!("profile {slot} is active on the keyboard; switch to another profile first"));
        }
        let mut e = d.ext_info()?.rewrite();
        if !e.set_profile_enabled(slot, enabled) {
            return invalid(format!("profile {slot} is not in the keyboard's profile list"));
        }
        d.set_ext_info(&e)?;
        a.live().disabled_profiles = e.disabled_profiles();
        let dirty = self.store().device(dev)?.profile(slot)?.dirty;
        if enabled && dirty && self.full_apply(&a, slot).is_ok() {
            let mut st = self.store();
            st.device_mut(dev)?.profile_mut(slot)?.dirty = false;
            st.save()?;
        }
        self.bus.emit("DeviceChanged", json!({"id": dev}));
        Ok(())
    }

    pub fn dock_state(&self, dev: &str) -> Result<Value> {
        let a = self.require(dev)?;
        let d = a.driver.everest().ok_or_else(|| Error::Unsupported("dock".into()))?;
        let e = d.ext_info()?;
        Ok(json!({
            "dockSide": e.dock_side(),
            "numpadSide": e.numpad_side(),
            "menuIndex": e.menu_index,
            "raw": e,
        }))
    }

    pub fn set_mode(&self, dev: &str, mode: &str, value: u8) -> Result<()> {
        let a = self.require(dev)?;
        match &a.driver {
            Driver::Everest(d) | Driver::DisplayPad(DisplayPadDev { cmd: d, .. }) => {
                use ev::toggle as t;
                let p = match mode {
                    "gameMode" => ev::toggle_set(t::GAME_MODE, value),
                    "gameModeStatus" => ev::toggle_set(t::GAME_MODE_STATUS, value),
                    "indicator" => ev::toggle_set(t::INDICATOR, value),
                    "syncProfiles" => ev::toggle_set(t::SYNC_PROFILES, value),
                    "yeti" => ev::toggle_set(t::YETI, value),
                    "lighting" => ev::main_lighting(value != 0),
                    "keyFunctions" => ev::key_functions(value != 0),
                    "syncEffect" => ev::sync_effect(value != 0, 0),
                    "logo" if a.kind == DeviceKind::DisplayPad => dp::show_logo(value != 0),
                    other => return invalid(format!("unknown mode {other:?}")),
                };
                d.cmd(&p)?;
            }
            Driver::Everest60(d) => {
                use e6::cmd as c;
                let code = match mode {
                    "lighting" => c::MAIN_LIGHTING,
                    "gameModeStatus" => c::SET_GAME_MODE_STATUS,
                    "indicator" => c::SET_INDICATOR,
                    "yeti" => c::SET_YETI,
                    "gameMode" => c::SET_GAME_MODE,
                    "syncProfiles" => c::SET_SYNC_PROFILES,
                    "keyFunctions" => c::KEY_FUNCTIONS,
                    other => return invalid(format!("unknown mode {other:?}")),
                };
                d.set(&e6::command(code, &[value as u32]))?;
            }
            Driver::Makalu(_) => return Err(Error::Unsupported("modes on the Makalu 67".into())),
        }
        let mut st = self.store();
        st.device_mut(dev)?.modes.insert(mode.to_string(), value);
        st.save()
    }

    pub fn get_modes(&self, dev: &str) -> Result<Value> {
        let a = self.require(dev)?;
        let mut m = self.store().device(dev)?.modes.clone();
        match &a.driver {
            Driver::Everest(d) if a.kind == DeviceKind::EverestMax => {
                use ev::toggle as t;
                for (name, code) in [
                    ("gameMode", t::GAME_MODE),
                    ("indicator", t::INDICATOR),
                    ("syncProfiles", t::SYNC_PROFILES),
                    ("gameModeStatus", t::GAME_MODE_STATUS),
                    ("yeti", t::YETI),
                ] {
                    if let Ok(v) = d.get_toggle(code) {
                        m.insert(name.into(), v);
                    }
                }
            }
            Driver::Everest60(d) => {
                use e6::cmd as c;
                for (name, code) in [
                    ("gameModeStatus", c::GET_GAME_MODE_STATUS),
                    ("indicator", c::GET_INDICATOR),
                    ("yeti", c::GET_YETI),
                    ("gameMode", c::GET_GAME_MODE),
                    ("syncProfiles", c::GET_SYNC_PROFILES),
                ] {
                    if let Ok(r) = d.exec(&e6::command(code, &[])) {
                        m.insert(name.into(), e6::result(&r, 0) as u8);
                    }
                }
            }
            _ => {}
        }
        Ok(serde_json::to_value(m)?)
    }

    pub fn reset(&self, dev: &str, what: &str) -> Result<()> {
        let a = self.require(dev)?;
        match &a.driver {
            Driver::Everest(d) | Driver::DisplayPad(DisplayPadDev { cmd: d, .. }) => {
                let p = match what {
                    "keys" => ev::reset_keys(),
                    "effects" => ev::reset_effects(),
                    "all" => ev::reset_flash(true),
                    "dockPictures" => ev::reset_dock_pictures(0x1F),
                    "numpadPictures" => ev::reset_numpad_pictures([1; 5]),
                    "pictures" => dp::reset_all_pictures(),
                    other => return invalid(format!("unknown reset {other:?}")),
                };
                d.cmd(&p)?;
            }
            Driver::Everest60(d) => {
                let p = match what {
                    "keys" => e6::command(e6::cmd::RESET_KEYS, &[]),
                    "effects" => e6::command(e6::cmd::RESET_EFFECTS, &[]),
                    "all" => e6::command(e6::cmd::RESET_FLASH, &[1]),
                    other => return invalid(format!("unknown reset {other:?}")),
                };
                d.exec(&p)?;
            }
            Driver::Makalu(d) => {
                match what {
                    "all" => d.exec(&mk::factory_reset())?,
                    other => return invalid(format!("unknown reset {other:?}")),
                };
            }
        }
        let mut st = self.store();
        let kind = st.device(dev)?.kind;
        let fresh = DeviceConfig::new(kind);
        let cfg = st.device_mut(dev)?;
        for p in &mut cfg.profiles {
            let f = fresh.profile(p.slot)?;
            if matches!(what, "keys" | "all") {
                p.bindings.clear();
                p.mouse = f.mouse.clone();
            }
            if matches!(what, "effects" | "all") {
                p.lighting = f.lighting.clone();
                p.custom = f.custom.clone();
            }
            p.dirty = false;
        }
        st.save()?;
        drop(st);
        self.bus.emit("StoreChanged", json!({"id": dev}));
        Ok(())
    }

    // Makalu 67 ---------------------------------------------------------------------------

    fn makalu(&self, dev: &str) -> Result<Option<Arc<MakaluDev>>> {
        Ok(match self.attached(dev) {
            Some(a) => match &a.driver {
                Driver::Makalu(d) => Some(d.clone()),
                _ => return invalid("not a Makalu 67"),
            },
            None => None,
        })
    }

    fn edit_mouse(&self, dev: &str, slot: u8, f: impl FnOnce(&mut sherpa_core::model::MouseProfile)) -> Result<()> {
        let mut st = self.store();
        let p = st.device_mut(dev)?.profile_mut(slot)?;
        f(p.mouse.get_or_insert_with(Default::default));
        st.save()?;
        drop(st);
        self.bus.emit("StoreChanged", json!({"id": dev, "slot": slot}));
        Ok(())
    }

    pub fn set_mouse_sensor(&self, dev: &str, slot: u8, s: MouseSensor) -> Result<()> {
        let c = mk::sensor_block(slot, &s)?;
        if let Some(d) = self.makalu(dev)? {
            d.exec(&c)?;
        }
        self.edit_mouse(dev, slot, |m| m.sensor = s)
    }

    pub fn set_mouse_lighting(&self, dev: &str, slot: u8, l: MouseLighting, live: bool) -> Result<()> {
        let d = self.makalu(dev)?;
        if live {
            return match d {
                Some(d) => d.exec(&mk::lighting_live(&l)?).map(|_| ()),
                None => Err(Error::NotFound("device not connected".into())),
            };
        }
        let c = mk::lighting_saved(slot, &l)?;
        if let Some(d) = d {
            d.exec(&c)?;
        }
        self.edit_mouse(dev, slot, |m| m.lighting = l)
    }

    pub fn set_mouse_button(&self, dev: &str, slot: u8, b: MouseButton) -> Result<()> {
        let macros = self.store().data.macros.clone();
        let rec = mk::button_record(&b, &macros)?;
        if let Some(d) = self.makalu(dev)? {
            d.set_button(slot, b.button, &rec)?;
        }
        self.edit_mouse(dev, slot, |m| match m.buttons.iter_mut().find(|x| x.button == b.button) {
            Some(x) => *x = b,
            None => m.buttons.push(b),
        })
    }

    pub fn calibrate(&self, dev: &str, step: &str, surface: Option<u16>) -> Result<Value> {
        let d = self.makalu(dev)?.ok_or_else(|| Error::NotFound("device not connected".into()))?;
        let r = match step {
            "start" => d.exec(&mk::calibration_start())?,
            "result" => d.exec(&mk::calibration_result())?,
            "save" => d.exec(&mk::calibration_set_surface(surface.ok_or_else(|| Error::Invalid("surface required".into()))?))?,
            "reset" => d.exec(&mk::calibration_reset())?,
            other => return invalid(format!("unknown calibration step {other:?}")),
        };
        Ok(
            json!({"result": r.get(16).copied().unwrap_or(0), "surface": u16::from_le_bytes([r.get(17).copied().unwrap_or(0), r.get(18).copied().unwrap_or(0)])}),
        )
    }

    // DisplayPad --------------------------------------------------------------------------

    fn image_dir(&self) -> Option<PathBuf> {
        self.opts.config_dir.as_ref().map(|d| d.join("images"))
    }

    /// Store an icon (102×102 B,G,R, base64) with a preview data URL. Returns its id.
    pub fn put_image(&self, bgr_b64: &str, preview: &str) -> Result<String> {
        let bgr =
            base64::engine::general_purpose::STANDARD.decode(bgr_b64.trim()).map_err(|e| Error::Invalid(format!("image data: {e}")))?;
        if bgr.len() != dp::ICON_BYTES {
            return invalid(format!("icon must be {} bytes", dp::ICON_BYTES));
        }
        let id: String = Sha256::digest(&bgr).iter().take(10).map(|b| format!("{b:02x}")).collect();
        if let Some(dir) = self.image_dir() {
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join(format!("{id}.bgr")), &bgr)?;
            std::fs::write(dir.join(format!("{id}.preview")), preview)?;
        }
        self.images.lock().unwrap().insert(id.clone(), (bgr, preview.to_string()));
        Ok(id)
    }

    pub(crate) fn load_image(&self, id: &str) -> Option<(Vec<u8>, String)> {
        if !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        if let Some(v) = self.images.lock().unwrap().get(id) {
            return Some(v.clone());
        }
        let dir = self.image_dir()?;
        let bgr = std::fs::read(dir.join(format!("{id}.bgr"))).ok()?;
        let prev = std::fs::read_to_string(dir.join(format!("{id}.preview"))).unwrap_or_default();
        self.images.lock().unwrap().insert(id.to_string(), (bgr.clone(), prev.clone()));
        Some((bgr, prev))
    }

    pub fn get_image(&self, id: &str) -> Result<String> {
        self.load_image(id).map(|v| v.1).ok_or_else(|| Error::NotFound(format!("image {id}")))
    }

    /// Upload the icons of a layer of the selected profile (live).
    pub fn draw_layer(&self, a: &Attached, layer: u32) -> Result<()> {
        let Driver::DisplayPad(d) = &a.driver else { return Ok(()) };
        let profile = self.store().device(&a.key)?.selected().clone();
        let black = dp::solid(dp::ICON_SIZE, dp::ICON_SIZE, 0, 0, 0);
        for row in tables::displaypad_layout() {
            let img = profile.binding(row.led, 0, layer).and_then(|b| {
                let id = if b.is_first_image_selected { b.image.as_ref() } else { b.second_image.as_ref().or(b.image.as_ref()) };
                id.and_then(|i| self.load_image(i)).map(|v| v.0)
            });
            if let Err(e) = d.icon_live(row.index as u8, img.as_deref().unwrap_or(&black)) {
                if e == Error::Disconnected || matches!(e, Error::Unsupported(_)) {
                    return Err(e);
                }
                log::warn!("{}: icon {} upload failed: {e}", a.key, row.index);
            }
        }
        a.live().display_layer = layer;
        self.bus.emit("LayerChanged", json!({"id": a.key, "layer": layer}));
        Ok(())
    }

    pub fn add_layer(&self, dev: &str, slot: u8, parent: u32, name: String) -> Result<u32> {
        let id = {
            let mut st = self.store();
            let p = st.device_mut(dev)?.profile_mut(slot)?;
            if !p.layers.iter().any(|l| l.id == parent) {
                return invalid(format!("no layer {parent}"));
            }
            let id = p.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
            p.layers.push(DisplayLayer { id, parent: Some(parent), name });
            // The child layer gets a Back key on the first icon.
            let first = &tables::displaypad_layout()[0];
            p.put_binding(KeyBinding {
                fw_key: first.fw_key,
                led: first.led,
                kind: BindingType::DisplayBack,
                layer_id: id,
                alternate_name: Some("Back".into()),
                is_first_image_selected: true,
                ..Default::default()
            });
            st.save()?;
            id
        };
        self.bus.emit("StoreChanged", json!({"id": dev, "slot": slot}));
        Ok(id)
    }

    pub fn delete_layer(&self, dev: &str, slot: u8, id: u32) -> Result<()> {
        if id == 0 {
            return invalid("the root layer cannot be deleted");
        }
        {
            let mut st = self.store();
            let p = st.device_mut(dev)?.profile_mut(slot)?;
            let mut doomed = vec![id];
            let mut i = 0;
            while i < doomed.len() {
                let cur = doomed[i];
                doomed.extend(p.layers.iter().filter(|l| l.parent == Some(cur)).map(|l| l.id));
                i += 1;
            }
            p.layers.retain(|l| !doomed.contains(&l.id));
            p.bindings.retain(|b| {
                !doomed.contains(&b.layer_id) && !(b.kind == BindingType::DisplayFolder && doomed.iter().any(|d| b.value == d.to_string()))
            });
            st.save()?;
        }
        if let Some(a) = self.attached(dev) {
            if a.live().display_layer != 0 {
                self.draw_layer(&a, 0)?;
            }
        }
        self.bus.emit("StoreChanged", json!({"id": dev, "slot": slot}));
        Ok(())
    }

    // App-linked profiles -----------------------------------------------------------------

    fn app_watch_loop(self: Arc<Self>) {
        let mut sys = sysinfo::System::new();
        let mut w = AppWatcher::default();
        while self.running() {
            let links: Vec<appwatch::Link> = {
                let st = self.store();
                st.data
                    .devices
                    .iter()
                    .flat_map(|(k, c)| {
                        c.profiles.iter().filter_map(move |p| {
                            p.linked_exe.as_ref().map(|e| appwatch::Link {
                                device: k.clone(),
                                slot: p.slot,
                                exe: e.clone(),
                                revert: p.revert_on_exit,
                            })
                        })
                    })
                    .collect()
            };
            if !links.is_empty() {
                let running = appwatch::running_exes(&mut sys);
                let sel = |d: &str| -> Option<u8> {
                    self.attached(d)?;
                    self.store().device(d).ok().map(|c| c.selected_slot())
                };
                for s in w.tick(&running, &links, &sel) {
                    log::info!("app-linked switch: {} → profile {}", s.device, s.slot);
                    if let Err(e) = self.select_profile(&s.device, s.slot) {
                        log::warn!("app-linked switch failed: {e}");
                    }
                }
            }
            thread::sleep(self.opts.timing.app_watch);
        }
    }

    // Event loops -------------------------------------------------------------------------

    fn everest_event_loop(self: Arc<Self>, a: Arc<Attached>, events: Receiver<Packet>) {
        let mut ks = KeyState::default();
        let layout = a.layout();
        for p in events {
            if p[0] != 0x01 {
                continue; // live LED frames (02..0A) are only used by sync features
            }
            for (usage, pressed) in ks.update(&p) {
                if let Some(row) = tables::layout_by_usage(layout, usage) {
                    self.on_key(&a, row, pressed);
                }
            }
        }
        log::debug!("{}: event loop ended", a.key);
    }

    fn e60_loop(self: Arc<Self>, a: Arc<Attached>, d: Arc<Everest60Dev>) {
        let mut last: Option<(u8, u8, u8)> = None;
        while self.running() && d.is_alive() {
            match d.poll() {
                Ok(p) => {
                    let state = (p.numpad, p.profile, p.effect);
                    if let Some(prev) = last {
                        if prev.0 != state.0 {
                            a.live().numpad_side = e6::numpad_side(p.numpad).into();
                            self.bus.device("DeviceChanged", &a.key);
                        }
                        if prev.1 != state.1 && (1..=5).contains(&p.profile) {
                            self.device_profile_changed(&a, p.profile);
                        }
                        if prev.2 != state.2 {
                            a.live().effect = p.effect;
                            self.bus.device("LightingChangedOnDevice", &a.key);
                        }
                    } else {
                        a.live().numpad_side = e6::numpad_side(p.numpad).into();
                    }
                    last = Some(state);
                    for (code, pressed) in p.events {
                        self.on_e60_key(&a, code, pressed);
                    }
                }
                Err(Error::Disconnected) => break,
                Err(e) => log::debug!("{}: poll failed: {e}", a.key),
            }
            thread::sleep(self.opts.timing.e60_poll);
        }
    }

    fn makalu_loop(self: Arc<Self>, a: Arc<Attached>, rx: Receiver<mk::Event>) {
        for e in rx {
            self.on_makalu_event(&a, e);
        }
    }

    /// The device switched profile on its own (F-PRO-4, D-3).
    pub(crate) fn device_profile_changed(&self, a: &Attached, slot: u8) {
        a.live().current_profile = slot;
        {
            let mut st = self.store();
            if let Ok(cfg) = st.device_mut(&a.key) {
                if cfg.selected_slot() != slot && cfg.select(slot).is_ok() {
                    let _ = st.save();
                }
            }
        }
        self.bus.emit("ProfileChangedOnDevice", json!({"id": a.key, "slot": slot}));
    }

    // Dock feeder -------------------------------------------------------------------------

    fn dock_loop(self: Arc<Self>, a: Arc<Attached>) {
        let Driver::Everest(d) = &a.driver else { return };
        let t = self.opts.timing.clone();
        let start = Instant::now();
        let mut next_clock = start + t.clock_delay;
        let mut next_refresh = start + t.dock_refresh;
        let mut mon = SysMon::new();
        while self.running() && d.link.is_alive() {
            let now = Instant::now();
            if now >= next_clock {
                match d.clock_sync(now_clock(), None) {
                    Ok(()) => log::debug!("{}: clock synced", a.key),
                    Err(e) => log::warn!("{}: clock sync failed: {e}", a.key),
                }
                next_clock = now + t.clock_period;
            }
            if now >= next_refresh {
                if let Some(s) = d.link.try_session() {
                    if let Ok(r) = s.transact(&ev::ext_info_request(), ev::timeout::EXT_INFO) {
                        a.live().set_ext(&ev::parse_ext_info(&r));
                    }
                }
                next_refresh = now + t.dock_refresh;
            }
            let page = a.live().dock_menu;
            let packet = match page {
                ev::dock_menu::VOLUME => sysmon::master_volume().map(ev::volume),
                ev::dock_menu::CPU => Some(ev::pc_info(ev::pc_slot::CPU, mon.cpu())),
                ev::dock_menu::GPU => Some(ev::pc_info(ev::pc_slot::GPU, mon.gpu())),
                ev::dock_menu::DISK => Some(ev::pc_info(ev::pc_slot::DISK, mon.disk())),
                ev::dock_menu::NETWORK => Some(ev::pc_info(ev::pc_slot::NETWORK, mon.network())),
                ev::dock_menu::RAM => Some(ev::pc_info(ev::pc_slot::RAM, mon.ram())),
                ev::dock_menu::APM => Some(ev::pc_info(ev::pc_slot::APM, self.apm_value() as u32)),
                _ => None,
            };
            if let Some(p) = packet {
                if let Err(e) = d.push(&p) {
                    log::debug!("{}: dock push failed: {e}", a.key);
                }
            }
            thread::sleep(t.feed);
        }
    }

    /// Key releases in the last 60 s.
    pub fn apm_value(&self) -> usize {
        let mut q = self.apm.lock().unwrap();
        let cutoff = Instant::now().checked_sub(Duration::from_secs(60));
        if let Some(c) = cutoff {
            while q.front().is_some_and(|t| *t < c) {
                q.pop_front();
            }
        }
        q.len()
    }

    /// Re-read the dock page (after a `MENU Change` event).
    pub(crate) fn refresh_dock_menu(&self, a: &Attached) {
        if let Some(d) = a.driver.everest() {
            if let Ok(e) = d.ext_info() {
                a.live().set_ext(&e);
            }
        }
    }

    // Generic dispatcher ------------------------------------------------------------------

    pub fn set_settings(&self, s: Settings) -> Result<()> {
        let mut st = self.store();
        st.data.settings = s;
        st.save()?;
        drop(st);
        self.apply_settings();
        self.bus.emit("SettingsChanged", json!({}));
        Ok(())
    }

    pub fn status(&self) -> Value {
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "simulated": self.opts.simulated,
            "configDir": self.opts.config_dir.as_ref().map(|d| d.display().to_string()),
            "platform": std::env::consts::OS,
            "recording": self.recorder.lock().unwrap().is_some(),
        })
    }

    /// Dispatch one IPC request.
    pub fn call(&self, method: &str, p: &Value) -> Result<Value> {
        let dev = || param::<String>(p, "dev");
        let slot = || param::<u8>(p, "slot");
        let ok = |r: Result<()>| r.map(|_| Value::Bool(true));
        match method {
            "GetStatus" => Ok(self.status()),
            "ListDevices" => Ok(self.list_devices()),
            "GetDevice" => self.get_device(&dev()?),
            "GetLayout" => self.get_layout(&dev()?),
            "GetProfiles" => Ok(serde_json::to_value(&self.store().device(&dev()?)?.profiles)?),
            "SelectProfile" => ok(self.select_profile(&dev()?, slot()?)),
            "SetProfileEnabled" => ok(self.set_profile_enabled(&dev()?, slot()?, param(p, "enabled")?)),
            "UpdateProfile" => ok(self.update_profile(&dev()?, slot()?, p)),
            "ResetProfile" => ok(self.reset_profile(&dev()?, slot()?)),
            "CopyProfile" => ok(self.copy_profile(&dev()?, param(p, "from")?, param(p, "to")?)),
            "ExportProfile" => self.export_profile(&dev()?, slot()?),
            "ImportProfile" => ok(self.import_profile(&dev()?, slot()?, param(p, "data")?)),
            "SetLighting" => ok(self.set_lighting(&dev()?, slot()?, param(p, "lighting")?, opt_param(p, "activate")?.unwrap_or(false))),
            "PreviewLighting" => ok(self.preview_lighting(&dev()?, &param(p, "lighting")?)),
            "SetCustom" | "SetPerKey" => {
                ok(self.set_custom(&dev()?, slot()?, param(p, "custom")?, opt_param(p, "activate")?.unwrap_or(false)))
            }
            "SetBinding" => ok(self.set_binding(&dev()?, slot()?, param(p, "binding")?)),
            "ClearBinding" => ok(self.clear_binding(
                &dev()?,
                slot()?,
                param(p, "led")?,
                opt_param(p, "layer")?.unwrap_or(0),
                opt_param(p, "layerId")?.unwrap_or(0),
            )),
            "GetMacros" => Ok(serde_json::to_value(&self.store().data.macros)?),
            "PutMacro" => ok(self.put_macro(param(p, "macro")?, opt_param(p, "oldName")?)),
            "DeleteMacro" => ok(self.delete_macro(&param::<String>(p, "name")?)),
            "ExportMacros" => Ok(json!({"format": "sherpa-macros", "version": 1, "macros": self.store().data.macros})),
            "ImportMacros" => self.import_macros(param(p, "data")?).map(|n| json!(n)),
            "StartRecording" => {
                ok(self.start_recording(opt_param(p, "keyboard")?.unwrap_or(true), opt_param(p, "mouse")?.unwrap_or(false)))
            }
            "StopRecording" => self.stop_recording(),
            "SetDock" => ok(self.set_dock(&dev()?, param(p, "dock")?)),
            "GetDockState" => self.dock_state(&dev()?),
            "SetMode" => ok(self.set_mode(&dev()?, &param::<String>(p, "mode")?, param(p, "value")?)),
            "GetModes" => self.get_modes(&dev()?),
            "Reset" => ok(self.reset(&dev()?, &param::<String>(p, "what")?)),
            "SyncClock" => {
                let a = self.require(&dev()?)?;
                let d = a.driver.everest().ok_or_else(|| Error::Unsupported("clock".into()))?;
                ok(d.clock_sync(now_clock(), None))
            }
            "SetMouseSensor" => ok(self.set_mouse_sensor(&dev()?, slot()?, param(p, "sensor")?)),
            "SetMouseLighting" => {
                ok(self.set_mouse_lighting(&dev()?, slot()?, param(p, "lighting")?, opt_param(p, "live")?.unwrap_or(false)))
            }
            "SetMouseButton" => ok(self.set_mouse_button(&dev()?, slot()?, param(p, "button")?)),
            "Calibrate" => self.calibrate(&dev()?, &param::<String>(p, "step")?, opt_param(p, "surface")?),
            "PutImage" => {
                self.put_image(&param::<String>(p, "bgr")?, &opt_param::<String>(p, "preview")?.unwrap_or_default()).map(|id| json!(id))
            }
            "GetImage" => self.get_image(&param::<String>(p, "id")?).map(|s| json!(s)),
            "AddLayer" => self
                .add_layer(&dev()?, slot()?, opt_param(p, "parent")?.unwrap_or(0), opt_param(p, "name")?.unwrap_or_else(|| "Folder".into()))
                .map(|id| json!(id)),
            "DeleteLayer" => ok(self.delete_layer(&dev()?, slot()?, param(p, "layer")?)),
            "OpenLayer" => {
                let a = self.require(&dev()?)?;
                ok(self.draw_layer(&a, param(p, "layer")?))
            }
            "SetDisplayBrightness" => {
                let a = self.require(&dev()?)?;
                match &a.driver {
                    Driver::DisplayPad(d) => ok(d.set_brightness(param(p, "level")?)),
                    _ => invalid("not a DisplayPad"),
                }
            }
            "SetDisplaySleep" => {
                let a = self.require(&dev()?)?;
                match &a.driver {
                    Driver::DisplayPad(d) => ok(d.sleep_timer(param(p, "enabled")?, param(p, "seconds")?)),
                    _ => invalid("not a DisplayPad"),
                }
            }
            "GetSettings" => Ok(serde_json::to_value(&self.store().data.settings)?),
            "SetSettings" => ok(self.set_settings(param(p, "settings")?)),
            "ApproveLaunch" => {
                let path: String = param(p, "path")?;
                let mut st = self.store();
                if !st.data.approved_launches.contains(&path) {
                    st.data.approved_launches.push(path);
                }
                ok(st.save())
            }
            "RevokeLaunch" => {
                let path: String = param(p, "path")?;
                let mut st = self.store();
                st.data.approved_launches.retain(|x| *x != path);
                ok(st.save())
            }
            "ListApprovals" => Ok(json!(self.store().data.approved_launches)),
            "ObsQuery" => {
                let s = self.store().data.settings.clone();
                let obs = crate::actions::obs::Obs::new(crate::actions::obs::ObsConfig { port: s.obs_port, password: s.obs_password });
                obs.query(&param::<String>(p, "query")?).map(|v| json!(v))
            }
            "GetCatalog" => Ok(crate::host::catalog()),
            "SimulateKey" if self.opts.simulated => {
                let a = self.require(&dev()?)?;
                self.simulate_key(&a, param(p, "led")?, opt_param(p, "layer")?.unwrap_or(0));
                Ok(Value::Bool(true))
            }
            other => invalid(format!("unknown method {other:?}")),
        }
    }
}

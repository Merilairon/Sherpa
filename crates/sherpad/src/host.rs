//! Key events from devices and the host actions they trigger (spec 05).

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use sherpa_core::binding;
use sherpa_core::everest::{self as ev, KEY_TYPE_LAUNCH};
use sherpa_core::keys::{self, HostKey};
use sherpa_core::launch::{self, Launch};
use sherpa_core::makalu67 as mk;
use sherpa_core::model::{BindingType, KeyBinding, MacroEventType, MultiStep};
use sherpa_core::shortcut::Shortcut;
use sherpa_core::tables::{self, LayoutRow};
use sherpa_core::types::{DeviceKind, LaunchKind, MenuIndex};

use crate::actions::{self, Executor};
use crate::daemon::{Attached, Daemon, Driver};
use crate::error::{Error, Result};

const SHORTCUT_HOLD_MS: u64 = 50;
const MEDIA_KEYS: [u16; 4] = [26, 35, 53, 62];

/// Where a binding lives in the store.
#[derive(Debug, Clone, Copy)]
pub struct KeyRef {
    pub led: u16,
    pub layer: u8,
    pub layer_id: u32,
}

impl Daemon {
    fn selected_binding(&self, a: &Attached, k: KeyRef) -> Option<KeyBinding> {
        let st = self.store();
        st.device(&a.key).ok()?.selected().binding(k.led, k.layer, k.layer_id).cloned()
    }

    fn report(&self, a: &Attached, e: &Error) {
        log::warn!("{}: action failed: {e}", a.key);
        self.bus.emit("ActionFailed", json!({"id": a.key, "message": e.to_string()}));
    }

    /// Run work off the device thread (C-4).
    fn spawn_action(&self, a: &Arc<Attached>, f: impl FnOnce(&Daemon) -> Result<()> + Send + 'static) {
        let me = self.arc();
        let a = a.clone();
        thread::spawn(move || {
            if let Err(e) = f(&me) {
                me.report(&a, &e);
            }
        });
    }

    // -----------------------------------------------------------------------------------
    // Everest Max / MacroPad / DisplayPad bitmap events

    pub(crate) fn on_key(&self, a: &Arc<Attached>, row: &LayoutRow, pressed: bool) {
        if a.kind == DeviceKind::DisplayPad {
            self.on_displaypad_key(a, row, pressed);
            return;
        }
        if pressed {
            return; // events fire on release
        }
        if a.kind == DeviceKind::EverestMax {
            match row.index {
                107 => {
                    self.refresh_dock_menu(a);
                    if let Some(Ok(fw)) = a.driver.everest().map(|d| d.fw_info()) {
                        a.live().effect = fw.menu_index;
                    }
                    self.bus.device("LightingChangedOnDevice", &a.key);
                    return;
                }
                171..=175 => {
                    let slot = (row.index - 170) as u8;
                    if let Some(Ok(fw)) = a.driver.everest().map(|d| d.fw_info()) {
                        a.live().effect = fw.menu_index;
                    }
                    self.device_profile_changed(a, slot);
                    return;
                }
                176 => {
                    self.bus.device("DeviceReset", &a.key);
                    return;
                }
                177..=179 => {
                    self.bus.device("LightingChangedOnDevice", &a.key);
                    return;
                }
                180 => {
                    self.bus.device("ModesChangedOnDevice", &a.key);
                    return;
                }
                116 => {
                    self.exec.notify("Sherpa", "Dock timer finished");
                    return;
                }
                _ => {}
            }
            self.apm.lock().unwrap().push_back(Instant::now());
        }
        if let Err(e) = self.everest_release(a, row) {
            if e != Error::Disconnected {
                self.report(a, &e);
            }
        }
    }

    /// Spec 05 §2.2.
    fn everest_release(&self, a: &Arc<Attached>, row: &LayoutRow) -> Result<()> {
        let Some(d) = a.driver.everest() else { return Ok(()) };
        let fw = d.fw_info()?;
        let selected = self.store().device(&a.key)?.selected_slot();
        if (1..=5).contains(&fw.current_profile) && fw.current_profile != selected {
            self.device_profile_changed(a, fw.current_profile);
            return Ok(());
        }
        let Some(content) = d.key_content(fw.current_profile, row.led as u8)? else { return Ok(()) };
        if content.key_type != KEY_TYPE_LAUNCH {
            return Ok(());
        }
        if a.kind == DeviceKind::EverestMax && MEDIA_KEYS.contains(&row.index) {
            let menu = d.ext_info()?.menu_index;
            if menu != ev::dock_menu::MEDIA_PASSTHROUGH {
                return Ok(());
            }
        }
        let Some(l) = launch::decode(&content.data) else { return Ok(()) };
        self.execute_launch(a, KeyRef { led: row.led, layer: 0, layer_id: 0 }, l, false)
    }

    /// Kinds 1 and 4 from the device may only run if the exact path is bound to that key
    /// in this host's store, or the user approved it once (05 §3, security requirement).
    fn launch_allowed(&self, a: &Attached, k: Option<KeyRef>, l: &Launch) -> bool {
        let path = match l.kind() {
            Some(LaunchKind::Program) => actions::split_program(&l.text).0,
            Some(LaunchKind::Folder) => l.text.trim().to_string(),
            _ => return true,
        };
        let st = self.store();
        if st.data.approved_launches.contains(&path) {
            return true;
        }
        let Ok(cfg) = st.device(&a.key) else { return false };
        match k {
            Some(k) => cfg
                .selected()
                .binding(k.led, k.layer, k.layer_id)
                .is_some_and(|b| matches!(b.kind, BindingType::Program | BindingType::Folder) && b.value.trim() == path),
            // Makalu events do not name the key unambiguously: accept any program button.
            None => cfg
                .profiles
                .iter()
                .filter_map(|p| p.mouse.as_ref())
                .flat_map(|m| m.buttons.iter())
                .any(|b| b.kind == "program" && b.value.trim() == path),
        }
    }

    pub(crate) fn execute_launch(&self, a: &Arc<Attached>, k: KeyRef, l: Launch, trusted: bool) -> Result<()> {
        match l.kind() {
            Some(LaunchKind::HotkeySwitch) => return self.hotkey_switch(a, k),
            Some(LaunchKind::MultiAction) => return self.multi_action(a, k),
            Some(LaunchKind::Program) | Some(LaunchKind::Folder) if !trusted && !self.launch_allowed(a, Some(k), &l) => {
                self.ask_approval(a, &l);
                return Ok(());
            }
            None => return Err(Error::Invalid(format!("unknown launch kind {}", l.kind))),
            _ => {}
        }
        self.spawn_action(a, move |d| actions::run_launch(d.exec.as_ref(), &l));
        Ok(())
    }

    fn ask_approval(&self, a: &Attached, l: &Launch) {
        let path = if l.kind() == Some(LaunchKind::Program) { actions::split_program(&l.text).0 } else { l.text.clone() };
        log::warn!("{}: launch of {path:?} from device content needs approval", a.key);
        self.bus.emit("LaunchApprovalRequired", json!({"id": a.key, "kind": l.kind, "path": path}));
    }

    /// Hotkey switch: fire `value` and `value2` on alternate presses, starting with
    /// `value`, and persist which one fires next.
    pub(crate) fn hotkey_switch(&self, a: &Arc<Attached>, k: KeyRef) -> Result<()> {
        let Some(b) = self.selected_binding(a, k) else { return Ok(()) };
        let has_second = b.value2.as_deref().is_some_and(|v| !v.trim().is_empty());
        let use_second = b.toggle_state && has_second;
        let combo = if use_second { b.value2.clone().unwrap() } else { b.value.clone() };
        {
            let mut st = self.store();
            let slot = st.device(&a.key)?.selected_slot();
            if let Some(x) = st.device_mut(&a.key)?.profile_mut(slot)?.binding_mut(k.led, k.layer, k.layer_id) {
                x.toggle_state = has_second && !use_second;
            }
            st.save()?;
        }
        let sc = Shortcut::parse(&combo)?;
        self.spawn_action(a, move |d| actions::tap_shortcut(d.exec.as_ref(), &sc, SHORTCUT_HOLD_MS));
        Ok(())
    }

    /// Multi-action (05 §5). A second press while it runs is ignored.
    pub(crate) fn multi_action(&self, a: &Arc<Attached>, k: KeyRef) -> Result<()> {
        let Some(b) = self.selected_binding(a, k) else { return Ok(()) };
        let steps: Vec<MultiStep> = serde_json::from_str(&b.value)?;
        if !a.busy.lock().unwrap().insert(k.led) {
            log::debug!("{}: multi-action on key {} still running", a.key, k.led);
            return Ok(());
        }
        let me = self.arc();
        let a2 = a.clone();
        thread::spawn(move || {
            for s in &steps {
                if let Err(e) = me.run_step(&a2, s, 1) {
                    me.report(&a2, &e);
                }
                thread::sleep(Duration::from_millis(s.action_delay_ms));
            }
            a2.busy.lock().unwrap().remove(&k.led);
        });
        Ok(())
    }

    fn run_step(&self, a: &Arc<Attached>, s: &MultiStep, depth: u8) -> Result<()> {
        if s.kind == BindingType::Delay {
            thread::sleep(Duration::from_millis(s.value.trim().parse().unwrap_or(0)));
            return Ok(());
        }
        let b = KeyBinding {
            kind: s.kind,
            sub_type: s.sub_type.clone(),
            value: s.value.clone(),
            value2: s.value2.clone(),
            ..Default::default()
        };
        let hold = if s.key_press_delay_ms > 0 { s.key_press_delay_ms } else { SHORTCUT_HOLD_MS };
        self.run_binding_on_host(a, &b, hold, depth)
    }

    /// Execute any binding type on the host (DisplayPad keys and multi-action steps).
    /// Keyboard-executed types are emulated with synthetic input.
    pub(crate) fn run_binding_on_host(&self, a: &Arc<Attached>, b: &KeyBinding, hold_ms: u64, depth: u8) -> Result<()> {
        let x: &dyn Executor = self.exec.as_ref();
        let v = b.value.trim();
        match b.kind {
            BindingType::Default | BindingType::Disable | BindingType::Delay => Ok(()),
            BindingType::Shortcut => actions::tap_shortcut(x, &Shortcut::parse(v)?, hold_ms),
            BindingType::KeyboardFunction => actions::tap_key(x, host_key(v)?, hold_ms),
            BindingType::Media => actions::tap_key(x, media_key(v)?, hold_ms),
            BindingType::Mouse => match v.to_ascii_lowercase().as_str() {
                "left" => click(x, 1, hold_ms),
                "right" => click(x, 2, hold_ms),
                "middle" => click(x, 3, hold_ms),
                "back" => click(x, 4, hold_ms),
                "forward" => click(x, 5, hold_ms),
                "scroll up" => x.wheel(1, false),
                "scroll down" => x.wheel(-1, false),
                "scroll left" => x.wheel(-1, true),
                "scroll right" => x.wheel(1, true),
                other => Err(Error::Invalid(format!("unknown mouse action {other:?}"))),
            },
            BindingType::Profile => {
                let cur = self.store().device(&a.key)?.selected_slot();
                let slot = match v {
                    "Cycle" | "Next" => cur % 5 + 1,
                    "Previous" => (cur + 3) % 5 + 1,
                    p => p
                        .trim_start_matches("Profile")
                        .trim()
                        .parse()
                        .map_err(|_| Error::Invalid(format!("unknown profile action {p:?}")))?,
                };
                self.select_profile(&a.key, slot)
            }
            BindingType::OsCommand => match v {
                "Shutdown" => x.system("shutdown"),
                "Sleep" => x.system("sleep"),
                "Hibernate" => x.system("hibernate"),
                "Calculator" => x.system("calculator"),
                "Lock computer" => x.lock_session(),
                "Run explorer" => x.system("explorer"),
                "Task manager" => x.system("taskmanager"),
                other => Err(Error::Invalid(format!("unknown OS command {other:?}"))),
            },
            BindingType::Macro => self.play_macro_on_host(v),
            BindingType::HotkeySwitch => {
                let sc = Shortcut::parse(v)?;
                actions::tap_shortcut(x, &sc, hold_ms)
            }
            BindingType::MultiAction => {
                if depth > 2 {
                    return Err(Error::Invalid("multi-actions nested too deeply".into()));
                }
                let steps: Vec<MultiStep> = serde_json::from_str(v)?;
                for s in &steps {
                    self.run_step(a, s, depth + 1)?;
                    thread::sleep(Duration::from_millis(s.action_delay_ms));
                }
                Ok(())
            }
            BindingType::DisplayFolder | BindingType::DisplayBack => Ok(()),
            _ => {
                let payload = binding::launch_payload(b).ok_or_else(|| Error::Invalid("no launch content".into()))?;
                let l = launch::decode(&payload).ok_or_else(|| Error::Invalid("empty launch content".into()))?;
                actions::run_launch(x, &l)
            }
        }
    }

    /// Play a library macro with synthetic input (used where the device cannot store it).
    /// TODO(spec): hold/repeat playback needs key state and is played once on the host.
    pub(crate) fn play_macro_on_host(&self, name: &str) -> Result<()> {
        let m = self.store().macro_named(name).cloned().ok_or_else(|| Error::NotFound(format!("macro {name}")))?;
        let x = self.exec.as_ref();
        let mut m = m;
        m.normalize();
        for e in &m.events {
            match e.kind {
                MacroEventType::Keydown | MacroEventType::Keyup => {
                    if let Some(k) = sherpa_core::macros::host_key(e) {
                        x.key(k, e.kind == MacroEventType::Keydown)?;
                    }
                }
                MacroEventType::Mousedown | MacroEventType::Mouseup => {
                    x.mouse_button(e.button.unwrap_or(1), e.kind == MacroEventType::Mousedown)?
                }
                MacroEventType::Wheel => x.wheel(if e.rotation.unwrap_or(1) >= 0 { 1 } else { -1 }, false)?,
            }
            thread::sleep(Duration::from_millis(e.delay_ms as u64));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // DisplayPad (09 §5)

    fn on_displaypad_key(&self, a: &Arc<Attached>, row: &LayoutRow, pressed: bool) {
        let layer = a.live().display_layer;
        let k = KeyRef { led: row.led, layer: 0, layer_id: layer };
        let Some(b) = self.selected_binding(a, k) else { return };
        // Press feedback: show the "pressed" image while the key is down.
        if let (Some(img2), Driver::DisplayPad(d)) = (b.second_image.as_ref(), &a.driver) {
            if b.image.is_some() && b.is_first_image_selected && b.kind != BindingType::DisplayFolder {
                let show = if pressed { Some(img2.clone()) } else { b.image.clone() };
                if let Some(bgr) = show.and_then(|id| self.image_bytes(&id)) {
                    let _ = d.icon_live(row.index as u8, &bgr);
                }
            }
        }
        if pressed {
            return;
        }
        let res = match b.kind {
            BindingType::DisplayFolder => match b.value.trim().parse::<u32>() {
                Ok(child) => self.draw_layer(a, child),
                Err(_) => Err(Error::Invalid("folder key without a layer".into())),
            },
            BindingType::DisplayBack => {
                let parent = self
                    .store()
                    .device(&a.key)
                    .ok()
                    .and_then(|c| c.selected().layers.iter().find(|l| l.id == layer).and_then(|l| l.parent));
                let r = self.draw_layer(a, parent.unwrap_or(0));
                thread::sleep(Duration::from_millis(500));
                r
            }
            BindingType::HotkeySwitch => self.hotkey_switch(a, k),
            BindingType::MultiAction => self.multi_action(a, k),
            BindingType::Profile => self.run_binding_on_host(a, &b, SHORTCUT_HOLD_MS, 0),
            _ => {
                let b2 = b.clone();
                let a2 = a.clone();
                self.spawn_action(a, move |d| d.run_binding_on_host(&a2, &b2, SHORTCUT_HOLD_MS, 0));
                Ok(())
            }
        };
        if let Err(e) = res {
            self.report(a, &e);
        }
    }

    fn image_bytes(&self, id: &str) -> Option<Vec<u8>> {
        self.load_image(id).map(|(bgr, _)| bgr)
    }

    // -----------------------------------------------------------------------------------
    // Everest 60 (10 §2)

    pub(crate) fn on_e60_key(&self, a: &Arc<Attached>, code: u8, pressed: bool) {
        if code == 154 {
            // Fn: tracked for the Fn layer. Base Camp uses it as a profile-cycle trigger;
            // TODO(spec): the exact behaviour is unclear, so only the profile is re-read.
            a.live().fn_held = pressed;
            if !pressed {
                if let Driver::Everest60(d) = &a.driver {
                    if let Ok(fw) = d.fw_info() {
                        let slot = fw.current_profile as u8;
                        let selected = self.store().device(&a.key).map(|c| c.selected_slot()).unwrap_or(0);
                        if (1..=5).contains(&slot) && slot != selected {
                            self.device_profile_changed(a, slot);
                        }
                    }
                }
            }
            return;
        }
        if pressed {
            return;
        }
        let layer = if a.live().fn_held { 3 } else { 0 };
        let k = KeyRef { led: code as u16, layer, layer_id: 0 };
        // TODO(spec): the key-content read transfer (2D/2F/2E) layout is unknown, so the
        // host executes the store's binding for the key instead of reading it back.
        let Some(b) = self.selected_binding(a, k) else { return };
        if !b.kind.host_executed() {
            return;
        }
        let res = match b.kind {
            BindingType::HotkeySwitch => self.hotkey_switch(a, k),
            BindingType::MultiAction => self.multi_action(a, k),
            _ => match binding::launch_payload(&b).and_then(|p| launch::decode(&p)) {
                Some(l) => self.execute_launch(a, k, l, true),
                None => Ok(()),
            },
        };
        if let Err(e) = res {
            self.report(a, &e);
        }
    }

    // -----------------------------------------------------------------------------------
    // Makalu 67 (11 §6)

    pub(crate) fn on_makalu_event(&self, a: &Arc<Attached>, e: mk::Event) {
        let Driver::Makalu(d) = &a.driver else { return };
        let res = match e {
            mk::Event::RunProgram { e4, e5 } | mk::Event::RunBrowser { e4, e5 } => {
                let kind = if matches!(e, mk::Event::RunProgram { .. }) { LaunchKind::Program } else { LaunchKind::Url };
                d.bulk_text(e4, e5).map(|text| {
                    let l = Launch { kind: kind as u8, text };
                    if kind == LaunchKind::Program && !self.launch_allowed(a, None, &l) {
                        self.ask_approval(a, &l);
                        return;
                    }
                    self.spawn_action(a, move |d| actions::run_launch(d.exec.as_ref(), &l));
                })
            }
            mk::Event::ProfileChanged(p) => {
                if (1..=5).contains(&p) {
                    self.device_profile_changed(a, p);
                }
                Ok(())
            }
            mk::Event::DpiChanged => {
                self.bus.device("SensorChangedOnDevice", &a.key);
                Ok(())
            }
            mk::Event::LightingChanged => {
                self.bus.device("LightingChangedOnDevice", &a.key);
                Ok(())
            }
            mk::Event::OtherChanged => {
                self.bus.device("DeviceChanged", &a.key);
                Ok(())
            }
            mk::Event::Calibration(r) => {
                self.bus.emit("CalibrationResult", json!({"id": a.key, "result": r}));
                Ok(())
            }
            mk::Event::Unknown(_) => Ok(()),
        };
        if let Err(e) = res {
            self.report(a, &e);
        }
    }

    /// Press and release a key as if the device reported it (simulated devices only).
    pub(crate) fn simulate_key(&self, a: &Arc<Attached>, led: u16, layer: u8) {
        match a.kind {
            DeviceKind::Everest60 => {
                if layer == 3 {
                    self.on_e60_key(a, 154, true);
                }
                self.on_e60_key(a, led as u8, true);
                self.on_e60_key(a, led as u8, false);
                if layer == 3 {
                    a.live().fn_held = false;
                }
            }
            DeviceKind::Makalu67 => {}
            _ => {
                if let Some(row) = tables::layout_for(a.kind).iter().find(|r| r.led == led) {
                    self.on_key(a, row, true);
                    self.on_key(a, row, false);
                }
            }
        }
    }
}

fn click(x: &dyn Executor, button: u8, hold: u64) -> Result<()> {
    x.mouse_button(button, true)?;
    thread::sleep(Duration::from_millis(hold.min(100)));
    x.mouse_button(button, false)
}

fn host_key(name: &str) -> Result<&'static HostKey> {
    keys::by_name(name).ok_or_else(|| Error::Invalid(format!("unknown key {name:?}")))
}

fn media_key(v: &str) -> Result<&'static HostKey> {
    let name = match v {
        "Play/Pause" => "PlayPause",
        "Stop" => "StopMedia",
        "Previous" => "PrevTrack",
        "Next" => "NextTrack",
        "Volume up" => "VolumeUp",
        "Volume down" => "VolumeDown",
        "Mute" => "Mute",
        other => return Err(Error::Invalid(format!("unknown media action {other:?}"))),
    };
    host_key(name)
}

/// Option lists for UIs.
pub fn catalog() -> Value {
    fn names(t: &[(&'static str, u16)]) -> Vec<&'static str> {
        t.iter().map(|(n, _)| *n).collect()
    }
    json!({
        "menus": MenuIndex::ALL.iter().map(|m| json!({"index": m.value(), "id": m, "label": m.label()})).collect::<Vec<_>>(),
        "media": names(binding::MEDIA),
        "mouse": names(binding::MOUSE).into_iter().chain(binding::MOUSE_UNSPECIFIED.iter().copied()).collect::<Vec<_>>(),
        "profile": names(binding::PROFILE),
        "os": names(binding::OS_REMAP).into_iter().chain(binding::OS_OTHER.iter().copied()).collect::<Vec<_>>(),
        "keyboardFunctions": (13..=24).map(|n| format!("F{n}")).collect::<Vec<_>>(),
        "keys": keys::KEYS.iter().map(|k| k.name).collect::<Vec<_>>(),
        "obs": crate::actions::obs::COMMANDS,
        "obsQueries": crate::actions::obs::QUERIES,
        "twitch": ["chat message", "emote chat", "followers chat", "slow chat", "sub chat", "clear chat", "play ad", "create clip", "open last clip", "create stream marker", "viewers", "stream title / game"],
        "youtube": ["chat message"],
        "spotify": ["play/pause", "next track", "previous track", "volume up", "volume down", "volume set", "volume mute/unmute", "shuffle mode", "repeat mode", "liked song toggle", "save song to playlist", "remove song from playlist", "playlist start", "play uri", "song info", "change playback device"],
        "mouseButtonTypes": ["mouse", "wheel", "key", "macro", "profile", "dpi", "sniper", "brightness", "effect", "media", "program", "url", "disabled"],
        "mouseEffects": [
            {"value": 0, "label": "Off"}, {"value": 1, "label": "Static"}, {"value": 2, "label": "Color Wave"}, {"value": 3, "label": "Tornado"},
            {"value": 4, "label": "Breathing (1 colour)"}, {"value": 5, "label": "Breathing (2 colours)"}, {"value": 6, "label": "Breathing (7 colours)"},
            {"value": 7, "label": "Reactive"}, {"value": 8, "label": "YETI"}, {"value": 15, "label": "Custom"}
        ],
        "screenSaverTypes": [
            {"value": 0, "label": "Image"}, {"value": 1, "label": "Clock (analog)"}, {"value": 2, "label": "Clock (digital)"}, {"value": 3, "label": "Stopwatch"},
            {"value": 4, "label": "Timer"}, {"value": 7, "label": "Volume"}, {"value": 8, "label": "Brightness"}, {"value": 9, "label": "CPU"},
            {"value": 10, "label": "GPU"}, {"value": 11, "label": "HDD"}, {"value": 12, "label": "Internet"}, {"value": 13, "label": "RAM"}, {"value": 14, "label": "APM"}
        ],
    })
}

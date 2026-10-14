//! Logical data model (spec 03 §2). Serialised as camelCase JSON in the store and over IPC.

use serde::{Deserialize, Serialize};

use crate::types::{ColorType, DelayMode, MenuIndex, PlaybackMode, Rgb};

pub const LED_COUNT: usize = 126;
pub const PROFILE_SLOTS: u8 = 5;

/// Base Camp's seeded Custom-mode palette (F-LGT-4).
pub const DEFAULT_PALETTE: [&str; 10] =
    ["#5BDE62", "#98eb34", "#98eb35", "#B3357F", "#eb4034", "#DC29BC", "#00ccb5", "#0076cc", "#4700cc", "#cc0043"];

fn default_true() -> bool {
    true
}

/// One lighting menu entry of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Lighting {
    pub menu_index: MenuIndex,
    pub active: bool,
    pub color_type: ColorType,
    pub color1: Rgb,
    pub color2: Rgb,
    /// Background colour (Reactive, Matrix, YETI).
    pub color3: Rgb,
    /// 0..100
    pub speed: u8,
    /// 0..100
    pub brightness: u8,
    /// Tornado: 9 or 10. Color Wave: 0, 2, 4 or 6.
    pub direction: u8,
    pub sync_across_profiles: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting::new(MenuIndex::Static)
    }
}

impl Lighting {
    pub fn new(menu: MenuIndex) -> Self {
        Lighting {
            menu_index: menu,
            active: menu == MenuIndex::Static,
            color_type: ColorType::Single,
            color1: Rgb(0x00, 0xa0, 0xff),
            color2: Rgb(0xff, 0x00, 0x80),
            color3: Rgb::BLACK,
            speed: 50,
            brightness: 100,
            direction: match menu {
                MenuIndex::Tornado => 9,
                _ => 0,
            },
            sync_across_profiles: false,
        }
    }

    pub fn defaults() -> Vec<Lighting> {
        MenuIndex::ALL.iter().map(|&m| Lighting::new(m)).collect()
    }
}

/// Custom-mode key painting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomLighting {
    pub leds: Vec<Rgb>,
    pub per_led_effect: Vec<u8>,
    pub selected_sub_effect: String,
    pub palette: Vec<Rgb>,
}

impl Default for CustomLighting {
    fn default() -> Self {
        CustomLighting::with_leds(LED_COUNT)
    }
}

impl CustomLighting {
    pub fn with_leds(n: usize) -> Self {
        CustomLighting {
            leds: vec![Rgb::WHITE; n],
            per_led_effect: vec![0; n],
            selected_sub_effect: "static".into(),
            palette: DEFAULT_PALETTE.iter().filter_map(|s| Rgb::parse(s)).collect(),
        }
    }
}

/// Binding types of spec 01 §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingType {
    #[default]
    Default,
    Disable,
    Shortcut,
    KeyboardFunction,
    Media,
    Mouse,
    Profile,
    OsCommand,
    Macro,
    Program,
    Url,
    Obs,
    Folder,
    Twitch,
    YouTube,
    Spotify,
    HotkeySwitch,
    MultiAction,
    /// Multi-action step only: pause `value` ms.
    Delay,
    /// DisplayPad: opens the child layer whose id is `value`.
    DisplayFolder,
    /// DisplayPad: returns to the parent layer.
    DisplayBack,
}

impl BindingType {
    /// Executed by the host rather than the keyboard firmware.
    pub fn host_executed(self) -> bool {
        matches!(
            self,
            BindingType::Program
                | BindingType::Url
                | BindingType::Obs
                | BindingType::Folder
                | BindingType::Twitch
                | BindingType::YouTube
                | BindingType::Spotify
                | BindingType::HotkeySwitch
                | BindingType::MultiAction
                | BindingType::DisplayFolder
                | BindingType::DisplayBack
        )
    }
}

/// One key binding of a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyBinding {
    /// Source firmware key code.
    pub fw_key: u16,
    /// LED index (Everest Max) or matrix number (MacroPad/DisplayPad) or event key id
    /// (Everest 60). Used to find the binding when the key is released.
    pub led: u16,
    #[serde(rename = "type")]
    pub kind: BindingType,
    pub sub_type: Option<String>,
    pub value: String,
    pub value2: Option<String>,
    pub alternate_name: Option<String>,
    pub sync_across_profiles: bool,
    pub toggle_state: bool,
    /// Everest 60: 0 = base layer, 3 = Fn layer.
    pub layer: u8,
    /// DisplayPad: the layer (folder) this binding belongs to; 0 = root.
    pub layer_id: u32,
    /// DisplayPad: image references (file names in the image store) and captions.
    pub image: Option<String>,
    pub second_image: Option<String>,
    pub optional_text: Option<String>,
    pub second_optional_text: Option<String>,
    #[serde(default = "default_true")]
    pub is_first_image_selected: bool,
}

/// A multi-action step (03 §2, KeyBinding).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MultiStep {
    #[serde(rename = "type")]
    pub kind: BindingType,
    pub sub_type: Option<String>,
    pub value: String,
    pub value2: Option<String>,
    pub action_delay_ms: u64,
    pub key_press_delay_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MacroEventType {
    Keydown,
    Keyup,
    Mousedown,
    Mouseup,
    Wheel,
}

/// One recorded macro event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroEvent {
    #[serde(rename = "type")]
    pub kind: MacroEventType,
    /// Key name (canonical host key name) for key events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Windows VK code; accepted on import when `key` is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rawcode: Option<u32>,
    /// Mouse button: 1 left, 2 right, 3 middle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<u8>,
    /// Wheel: positive = up, negative = down.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<i8>,
    /// Milliseconds until the next event.
    #[serde(default)]
    pub delay_ms: u32,
}

impl MacroEvent {
    pub fn key(kind: MacroEventType, name: &str, delay_ms: u32) -> Self {
        MacroEvent { kind, key: Some(name.into()), rawcode: None, button: None, rotation: None, delay_ms }
    }
}

/// A macro from the global library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Macro {
    pub name: String,
    pub record_keyboard: bool,
    pub record_mouse: bool,
    pub delay_mode: DelayMode,
    pub custom_delay_ms: u32,
    pub playback: PlaybackMode,
    pub events: Vec<MacroEvent>,
    pub order: u32,
}

impl Default for Macro {
    fn default() -> Self {
        Macro {
            name: String::new(),
            record_keyboard: true,
            record_mouse: false,
            delay_mode: DelayMode::Recorded,
            custom_delay_ms: 50,
            playback: PlaybackMode::Once,
            events: Vec::new(),
            order: 0,
        }
    }
}

impl Macro {
    /// Apply the delay mode and force the last delay to 0 (F-MAC-3/4).
    pub fn normalize(&mut self) {
        if self.delay_mode == DelayMode::None {
            for e in &mut self.events {
                e.delay_ms = 0;
            }
        }
        if let Some(last) = self.events.last_mut() {
            last.delay_ms = 0;
        }
    }
}

/// Everest Max media dock settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DockSettings {
    /// bit0 clock, 1 profile, 2 lighting, 3 volume, 4 brightness, 5 PC info, 6 APM, 7 custom.
    pub pages_enabled: u8,
    pub clock_digital: bool,
    pub clock12h: bool,
    pub menu_color: Rgb,
    pub screen_saver_enabled: bool,
    pub turn_off_enabled: bool,
    pub screen_saver_type: u8,
    pub screen_saver_sec: u16,
    pub turn_off_sec: u16,
    pub profiles_shown: [u8; 5],
    /// Per profile; `None` keeps the keyboard's level, which cannot be read back.
    pub dock_brightness: [Option<u8>; 5],
    pub numpad_brightness: [Option<u8>; 5],
    pub pixel_shift_time: u8,
    pub double_click_ms: u8,
}

impl Default for DockSettings {
    fn default() -> Self {
        DockSettings {
            pages_enabled: 0xFF,
            clock_digital: true,
            clock12h: false,
            menu_color: Rgb(0x00, 0xa0, 0xff),
            screen_saver_enabled: false,
            turn_off_enabled: false,
            screen_saver_type: 1,
            screen_saver_sec: 300,
            turn_off_sec: 1800,
            profiles_shown: [1, 2, 3, 4, 5],
            dock_brightness: [None; 5],
            numpad_brightness: [None; 5],
            pixel_shift_time: 0,
            double_click_ms: 50,
        }
    }
}

/// Makalu 67 sensor block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MouseSensor {
    /// 125, 250, 500 or 1000.
    pub polling_hz: u16,
    pub response_ms: u8,
    pub angle_snapping: bool,
    pub lift_off_high: bool,
    /// 1..5 levels of (x, y) DPI.
    pub dpi_levels: Vec<(u16, u16)>,
    /// 1-based index into `dpi_levels`.
    pub current_level: u8,
}

impl Default for MouseSensor {
    fn default() -> Self {
        MouseSensor {
            polling_hz: 1000,
            response_ms: 4,
            angle_snapping: false,
            lift_off_high: false,
            dpi_levels: vec![(400, 400), (800, 800), (1600, 1600), (3200, 3200), (6400, 6400)],
            current_level: 2,
        }
    }
}

/// Makalu 67 lighting block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MouseLighting {
    /// 0 off, 1 static, 2 colour wave, 3 tornado, 4/5/6 breathing 1/2/7-colour,
    /// 7 reactive, 8 YETI, 15 custom.
    pub effect: u8,
    pub colors: [Rgb; 8],
    pub brightness: u8,
    /// 0 clockwise, 1 counter-clockwise.
    pub direction: u8,
    /// 0 slow, 1 normal, 2 fast.
    pub speed: u8,
}

impl Default for MouseLighting {
    fn default() -> Self {
        MouseLighting { effect: 2, colors: [Rgb(0x00, 0xa0, 0xff); 8], brightness: 100, direction: 0, speed: 1 }
    }
}

/// Makalu 67 button binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MouseButton {
    /// 1..6
    pub button: u8,
    /// `mouse`, `wheel`, `key`, `macro`, `profile`, `dpi`, `sniper`, `brightness`,
    /// `effect`, `media`, `program`, `url`, `disabled`.
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    pub value2: Option<String>,
}

impl Default for MouseButton {
    fn default() -> Self {
        MouseButton { button: 1, kind: "mouse".into(), value: "left".into(), value2: None }
    }
}

impl MouseButton {
    pub fn defaults() -> Vec<MouseButton> {
        ["left", "right", "middle", "back", "forward"]
            .iter()
            .enumerate()
            .map(|(i, v)| MouseButton { button: i as u8 + 1, kind: "mouse".into(), value: (*v).into(), value2: None })
            .chain(std::iter::once(MouseButton { button: 6, kind: "dpi".into(), value: "next".into(), value2: None }))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MouseProfile {
    pub sensor: MouseSensor,
    pub lighting: MouseLighting,
    pub buttons: Vec<MouseButton>,
}

/// DisplayPad layer (folder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DisplayLayer {
    pub id: u32,
    pub parent: Option<u32>,
    pub name: String,
}

/// A device profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    /// Firmware profile number 1..5.
    pub slot: u8,
    pub name: String,
    pub image: Option<String>,
    pub order: u32,
    pub selected: bool,
    /// Executable file name for app-linked switching.
    pub linked_exe: Option<String>,
    pub revert_on_exit: bool,
    pub lighting: Vec<Lighting>,
    pub custom: CustomLighting,
    pub bindings: Vec<KeyBinding>,
    pub mouse: Option<MouseProfile>,
    pub layers: Vec<DisplayLayer>,
    /// Store changes not yet written to the device (D-1).
    pub dirty: bool,
    /// Device modes kept per profile by the firmware (Everest Max game mode).
    pub modes: std::collections::BTreeMap<String, u8>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile::new(1, 126)
    }
}

impl Profile {
    pub fn new(slot: u8, leds: usize) -> Self {
        Profile {
            slot,
            name: format!("Profile {slot}"),
            image: None,
            order: slot as u32,
            selected: slot == 1,
            linked_exe: None,
            revert_on_exit: true,
            lighting: Lighting::defaults(),
            custom: CustomLighting::with_leds(leds),
            bindings: Vec::new(),
            mouse: None,
            layers: vec![DisplayLayer { id: 0, parent: None, name: "Root".into() }],
            dirty: false,
            modes: Default::default(),
        }
    }

    pub fn active_lighting(&self) -> Option<&Lighting> {
        self.lighting.iter().find(|l| l.active)
    }

    pub fn active_menu(&self) -> MenuIndex {
        self.active_lighting().map(|l| l.menu_index).unwrap_or(MenuIndex::Static)
    }

    pub fn binding(&self, led: u16, layer: u8, layer_id: u32) -> Option<&KeyBinding> {
        self.bindings.iter().find(|b| b.led == led && b.layer == layer && b.layer_id == layer_id)
    }

    pub fn binding_mut(&mut self, led: u16, layer: u8, layer_id: u32) -> Option<&mut KeyBinding> {
        self.bindings.iter_mut().find(|b| b.led == led && b.layer == layer && b.layer_id == layer_id)
    }

    /// Insert or replace a binding (keyed by led, layer and layer id).
    pub fn put_binding(&mut self, b: KeyBinding) {
        match self.binding_mut(b.led, b.layer, b.layer_id) {
            Some(slot) => *slot = b,
            None => self.bindings.push(b),
        }
    }
}

/// App-wide settings (spec 01 §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub start_on_boot: bool,
    pub start_minimized: bool,
    pub minimize_to_tray: bool,
    pub language: String,
    pub display_only_connected_devices: bool,
    pub obs_port: u16,
    pub obs_password: String,
    pub default_custom_delay_ms: u32,
    pub saved_colors: Vec<Rgb>,
    /// Re-apply the selected profile's lighting/bindings on attach (D-2).
    pub sync_on_attach: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            start_on_boot: false,
            start_minimized: false,
            minimize_to_tray: true,
            language: "en".into(),
            display_only_connected_devices: true,
            obs_port: 4455,
            obs_password: String::new(),
            default_custom_delay_ms: 50,
            saved_colors: Vec::new(),
            sync_on_attach: false,
        }
    }
}

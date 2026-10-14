# 03 · Data model

These are the entities the application must persist. Names are suggestions. Field
semantics and enum values are [P] unless tagged. You don't need Base Camp's database
schema; this is the logical model.

## 1. Enums

### EffectIndex (firmware lighting effect id) [P]
| Value | Effect |
|---|---|
| 0 | Static |
| 1 | Breathing |
| 3 | Reactive (variant a, used by the UI) |
| 4 | Color Wave |
| 5 | Reactive (variant b) |
| 6 | YETI |
| 7 | Tornado |
| 9 | Matrix |
| 10 | Custom |
| 11 | Reactive (variant c) |
| 12 | Off |

### MenuIndex (firmware effect slot; UI order) [P]
| Value | Entry |
|---|---|
| 0 | Static |
| 1 | Color Wave |
| 2 | Tornado |
| 3 | Breathing |
| 4 | Reactive |
| 5 | Matrix |
| 6 | Custom |
| 7 | YETI mode |
| 8 | Off |

### ColorType [P]
| Value | Meaning | Device `randColor` byte |
|---|---|---|
| 0 | single colour | 0 |
| 1 | dual colour (or colour + background for Reactive/Matrix/YETI) | 16 (dual) / 0 (with background) |
| 2 | random | 2 |

### PlaybackMode [P]
`once` → 1, `hold` → 4, `repeat` → 3 (device codes).

### DelayMode [P]
`recorded`, `custom`, `none`.

### Modifier mask [P]
Ctrl 1 · Shift 2 · Alt 4 · Win 8 (bitwise OR).

### LaunchKind (launch content) [P]
1 program · 2 URL · 3 OBS · 4 folder · 5 Twitch · 6 YouTube · 7 Spotify · 8 hotkey switch · 9 multi-action.

## 2. Entities

### Device
| Field | Notes |
|---|---|
| `pid`, `serial`/path | identity |
| `fwVersion` | from the device-info commands ([04](04-protocol-everest-max.md) §4) |
| `layout` | firmware layout id (`11 12` reply). Selects the keyboard language layout [I] |

### Profile
| Field | Type | Notes |
|---|---|---|
| `deviceType` | enum | everest-max, everest-60, macropad, … |
| `slot` | 1..5 | firmware profile number |
| `name` | text | |
| `image` | ref | optional |
| `order` | int | UI order |
| `selected` | bool | exactly one per device type |
| `linkedExe` | text? | executable file name for app-linked switching |
| `revertOnExit` | bool | |

### Lighting (one per profile × MenuIndex)
| Field | Type | Notes |
|---|---|---|
| `menuIndex` | MenuIndex | |
| `effectIndex` | EffectIndex | fixed per menu entry |
| `active` | bool | one per profile |
| `colorType` | ColorType | |
| `color1`, `color2`, `color3` | RGB | colour3 = background |
| `speed` | 0..100 | |
| `brightness` | 0..100 | |
| `direction` | int | Tornado and Color Wave |
| `syncAcrossProfiles` | bool | |
| `custom` | CustomLighting? | only for menu 6 |

### CustomLighting (Custom mode)
| Field | Type | Notes |
|---|---|---|
| `leds` | RGB[126] | indexed by LED index |
| `perLedEffect` | EffectIndex[126] | 0 = static colour from `leds` [I] |
| `selectedSubEffect` | text | which sub-effect the user last edited |
| `palette` | RGB[10] | user swatches |

### KeyBinding (one per profile × key)
| Field | Type | Notes |
|---|---|---|
| `fwKey` | int | source key (key table) |
| `led` | int | LED index (key table) |
| `type` | text | one of the binding types in [01](01-functional-spec.md) §5 |
| `subType` | text? | e.g. a Spotify action |
| `value` | text | main argument (path, URL, key combo "Ctrl+Shift+X", macro name, …) |
| `value2` | text? | secondary argument (second hotkey, OBS argument, stream game, …) |
| `alternateName` | text? | UI label |
| `syncAcrossProfiles` | bool | |
| `toggleState` | bool | hotkey-switch runtime state (which of the two shortcuts fires next) |

Multi-action `value` is a JSON array of steps:
```json
[{"type": "...", "subType": "...", "value": "...", "value2": "...", "actionDelayMs": 0, "keyPressDelayMs": 50}]
```
A step type of `"Delay"` with `value` = milliseconds is a pause [P].

### Macro (global library)
| Field | Type | Notes |
|---|---|---|
| `name` | text, unique | bindings reference it by name |
| `recordKeyboard`, `recordMouse` | bool | |
| `delayMode` | DelayMode | |
| `customDelayMs` | int | |
| `playback` | PlaybackMode | |
| `events` | MacroEvent[] | ≤ 999 (the device adds a terminator) |
| `order` | int | |

MacroEvent:
```json
{"type": "keydown|keyup|mousedown|mouseup|wheel", "key": "<key-table name>", "button": 1, "rotation": 1, "delayMs": 20}
```
On Linux, record keys as key-table names (or Linux keycodes mapped to them). Base Camp's own
file stores Windows VK codes in `rawcode`. Accept that on import, using the key table's VK
column [P].

### DockSettings (Everest Max media dock, per device)
| Field | Type | Device field |
|---|---|---|
| `pagesEnabled` | bitmask, 8 bits | `showMenu`: bit0 clock, 1 profile, 2 lighting, 3 volume, 4 brightness, 5 PC info, 6 APM, 7 custom [P] |
| `clockDigital`, `clock12h` | bool | clock command ([04](04-protocol-everest-max.md) §6.3) |
| `menuColor` | RGB | `menuColor` |
| `screenSaverEnabled` | bool | `screenSetup` bit0 [P] |
| `turnOffEnabled` | bool | `screenSetup` bit1 [P] |
| `screenSaverType` | enum | see below; bit position in `screenSetup` [U] |
| `screenSaverSec` | u16 | `screenSaver` (unit [U], probably seconds) |
| `turnOffSec` | u16 | `turnOff` |
| `profilesShown` | u8[5] | `showProfile` (the device's `0x10` "disabled" flags are kept on write [H]) |
| `dockBrightness[5]`, `numpadBrightness[5]` | u8 or null | `exBrightness`. [H] null = not set (`00`, the keyboard default); a level is `0x80 \| level`. Imported on attach, so saving keeps what is on the keyboard |

Screen saver types [P]: 0 Image, 1/2 Clock, 3 Stopwatch, 4 Timer, 7 Volume, 8 Brightness,
9 CPU, 10 GPU, 11 HDD, 12 Internet, 13 RAM, 14 APM.

### Settings (app-wide)
The fields are in [01](01-functional-spec.md) §2, plus integration credentials (keyring).

## 3. Persistence rules

**D-1** Persist to the store first, then write to the device. If the device write fails,
mark the item "dirty" and retry on the next attach.
**D-2** On attach, the store is authoritative for lighting, bindings and macros of the
*selected* profile. Re-apply them only if `syncOnAttach` is enabled (default off, because
writing flash wears it).
**D-3** The device is authoritative for which profile is active (F-PRO-4).
**D-4** Never persist telemetry.

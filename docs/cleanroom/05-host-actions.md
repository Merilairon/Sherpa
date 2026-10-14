# 05 · Key events and host actions

Some bindings are executed by the host, not the keyboard. Their content is stored **on the
keyboard** (launch content, [04](04-protocol-everest-max.md) §7.5). When the key is
released, the host reads that content back and acts on it [P]. A few bindings (hotkey
switch, multi-action) also need data from the host's store.

## 1. Event source [P]

1. Read input reports on the vendor collection. Each report starting with `01` is a full
   key-state bitmap ([04](04-protocol-everest-max.md) §10).
2. Diff it against the previous bitmap. Each changed bit gives *(usage, pressed)*.
3. Translate the usage to a **layout index** with `basecamp_linux/data/everest_max_layout.csv`
   (`hid_usage` → `index`). For every physical key, the layout index equals its **LED index**.

The layout table has one row per button:
`index, hid_usage, ui_row, ui_col, led, fw_key, label, sync_pos`. Rows with `ui_row = 696`
and the rows for indices 171–180 are virtual: device-generated events rather than physical
keys.

**Events fire on release** (`pressed = false`) [P]. Presses are only tracked for state.

## 2. Dispatch on release

### 2.1 Device-state events [P]
| Index | Label | Host reaction |
|---|---|---|
| 107 | MENU Change | The dock or lighting menu changed on the device. Re-read the active effect (`11 00`, then the effect getters) and update the UI. |
| 171–175 | Profile 1–5 | The device switched profile. Re-read `11 00`, mark that slot selected in the store, signal the UI. |
| 176 | Reset Default | The device reset to defaults. Re-read everything. |
| 177 / 178 | Brightness + / − | Re-read the brightness and update the UI. |
| 179 | Change Effect | Re-read the active effect and update the UI. |
| 180 | Change GameMode | Re-read the game-mode status. |
| 116 | Timer End | Dock timer finished. Optional desktop notification. |

### 2.2 All other keys [P]
1. **APM:** append *(index, now)* to a rolling 60-second window. Its size is the APM value
   ([01](01-functional-spec.md) F-DCK-4).
2. Read FW info (`11 00`). If the device's current profile is not the profile selected in the
   store, treat this as a device-side profile change (§2.1 row 171–175) and stop.
3. Read the key's content (`11 02 00 01 <profile> <index> 02`). Stop if there is none.
4. If `keyType ≠ 7` (not launch content), stop: the keyboard handled it.
5. If the key is a media key (index 26 Play/Pause, 35 Mute, 53 Previous, 62 Next), act only
   while the dock's menu index (`11 14` reply, offset 2) is `130`. Otherwise stop.
6. Decode the payload: drop zero bytes, then `kind = data[0]` and `text = UTF-8(data[1:])`
   trimmed. Run the action in §3.

## 3. Actions by launch kind

| Kind | Text format | Action | Validation (minimum) [P] |
|---|---|---|---|
| 1 Program | absolute path | start the program as the user, without a shell | path must exist |
| 2 URL | `http(s)://…` | open in the default browser (`xdg-open`) | scheme must be http or https |
| 3 OBS | `Command` or `Command~Arg` | send the obs-websocket request (§4.1) | OBS connected |
| 4 Folder | absolute path | open in the file manager | directory must exist |
| 5 Twitch | `SubFunction[~Value[~Value2]]` | §4.2 | account linked |
| 6 YouTube | `SubFunction[~Value]` | §4.3 | account linked |
| 7 Spotify | `SubFunction[~Value[~Value2]]` | §4.4 | account linked |
| 8 Hotkey switch | (`Shortcut1[~Shortcut2]`) | Look up the binding for this LED index in the active profile in the **store**. Fire `value` and `value2` on alternate presses, starting with `value`, and persist which one fires next. | binding exists |
| 9 Multi-action | (JSON) | Look up the binding in the store and run its steps (§5) | binding exists |

**Security requirement (not in Base Camp).** Launch content comes from the device, and the
device may have been programmed on another machine. Kinds 1 and 4 may only run if the exact
path also appears in this host's store for that key, or if the user approved it once. Never
pass the text to a shell.

### 3.1 Shortcut strings [P]
- Shortcuts are written as `Ctrl+Shift+X`: modifiers `ctrl`, `shift`, `alt`, `win` (case-insensitive) joined with `+`.
- The final token is a key name.
- A single `+` means the plus key itself.
- `Win+L` must lock the session (`loginctl lock-session`), not send keys.
- Press-and-release timing: 50 ms hold.

Inject keys on Linux through a `uinput` virtual keyboard.

## 4. Integration vocabularies [P]

### 4.1 OBS commands (exact strings)
- **Profiles:** `Next Profile`, `Previous Profile`, `Set Current Profile` (arg: profile name)
- **Scenes:** `Next Scene`, `Previous Scene`, `Set Current Scene` (arg: scene name)
- **Sources:** `Set Current Source` (arg: source name)
- **Transitions:** `Next Transition`, `Previous Transition`, `Set Current TransitionName` (arg), `Set Transition Duration` (arg: ms)
- **Audio:** `Mic Volume +`, `Mic Volume -`, `Mute Mic`, `Unmute Mic`, `Set Mic Volume` (arg), `Desktop Volume +`, `Desktop Volume –` (U+2013 EN DASH, sic), `Mute Desktop Volume`, `Unmute Desktop Volume`, `Set Desktop Volume` (arg)
- **Output:** `Start Streaming`, `Stop Streaming`, `Start Recording`, `Stop Recording`, `Pause Recording`, `Resume Recording`, `Start Reply Buffer`, `Stop Reply Buffer`, `Save Reply Buffer`
- **Studio:** `Enable Studio Mode`, `Disable Studio Mode`, `Open Projector`
- **Media:** `Play Media`, `Pause Media`, `Stop Media`, `Next Media`, `Previous Media`
- **Query (UI pickers only):** `Get Profiles`, `Get Scenes`, `Get Scenes Data`, `Get Sources`, `Get Sources List`, `Get Transitions`

Map each one to the matching obs-websocket v5 request yourself.

### 4.2 Twitch sub-functions
`chat message` (value: text), `emote chat`, `followers chat`, `slow chat`, `sub chat`,
`clear chat`, `play ad` (value: length), `create clip`, `open last clip`,
`create stream marker`, `viewers`, `stream title / game` (value: title, value2: game).

### 4.3 YouTube sub-functions
`chat message` (value: text).

### 4.4 Spotify sub-functions
`play/pause`, `next track`, `previous track`, `volume up`, `volume down`, `volume set`
(value), `volume mute/unmute`, `shuffle mode`, `repeat mode`, `liked song toggle`,
`save song to playlist` (value: playlist), `remove song from playlist` (value: playlist),
`playlist start` (value: playlist), `play uri` (value: URI), `song info`,
`change playback device` (value: device).

## 5. Multi-action execution [P]
A binding's `value` is a JSON array of steps ([03](03-data-model.md)). Run them in order:

1. If the step is `Delay`, sleep `value` ms.
2. If the step is `Media` with Previous/Next/Volume up/Volume down, send the matching media
   key through uinput, holding it for `keyPressDelayMs`.
3. Otherwise run the step as if it were a single binding of that type. Keyboard-executed
   types (shortcut, macro …) are emulated on the host with uinput.
4. Sleep `actionDelayMs`.

Steps run on a worker thread. Base Camp doesn't say what happens on a second press while a
multi-action is still running. Recommended: ignore the second press.

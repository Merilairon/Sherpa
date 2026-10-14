# 04 · Everest Max protocol (VID `3282`, PID `0001`)

**Conventions.**
- Every request and reply is a **64-byte payload**, written `b0 b1 … b63`.
- Unless a command says otherwise, the request is **zero-filled** and multi-byte integers
  are **little-endian**.
- `u16@2` means a 16-bit value at bytes 2–3.
- Bytes not mentioned are 0.

## 1. Transport [P]

| Item | Value |
|---|---|
| Interface | The HID top-level collection with **Usage Page `0xFF00`, Usage `0x01`**. On Linux, scan `/sys/class/hidraw/*/device/report_descriptor` for it. |
| Output (commands) | Write `reportId ‖ payload[64]`. `reportId` is the Report ID declared in that collection, or `0x00` if the collection declares none [P for the rule; actual value U]. |
| Input (replies, events) | Read input reports from the same collection. Strip the leading report-ID byte if the descriptor uses report IDs. |
| Feature channel | Used only by the `AA 55` command family (§4.1) through HID SetFeature/GetFeature on the same collection [P]. |
| Bootloader | `060B:7801`. Out of scope. |

## 2. Request/response engine [P]

1. Hold a per-device lock for the whole command, including every chunk.
2. Send the request.
3. Wait for an input report whose **bytes 0–3 equal the request's bytes 0–3**: that is the
   acknowledgement or reply. While waiting, skip input reports that start with `0x01`–`0x0A`
   (events, §10) and any other non-matching report.
4. If an input report starts with `FF AA`, the request was **rejected** (NAK).
5. **Timeout** is 600 ms unless the command table says otherwise. A timeout is a failure.
6. Multi-packet *writes* send each chunk only after the previous chunk's acknowledgement.
7. Multi-packet *reads* use the reassembly rules in §9.

Implementation note [I]: some getters reply with the request header followed by values.
For a getter, "reply" means that matching report.

## 3. Command map (overview)

| b0 | Family |
|---|---|
| `11` | Device info getters/setters, dock data (PC info, EQ, volume, clock), extended info |
| `12` | Toggles: brightness on/off, key function, game mode, indicator, sync across profiles, YETI, sync effect |
| `13` | Persistence: save flash, resets |
| `14` | Profiles, lighting, key remap, macro header |
| `15` | Macro data chunks |
| `16` | Single-XY effect [U] |
| `17` | Launch content chunks |
| `19` / `1A` / `1B` | Raw flash read / write / erase. **Do not implement.** |
| `1C` | Numpad display-key pictures selection |
| `AA 55` (feature) | Device identity, sub-device info, reset |

## 4. Identity and status

### 4.1 Feature-channel commands [P structure, I reply offsets]
| Request (feature report) | Purpose | Reply |
|---|---|---|
| `AA 55 80` | Device info | `vid u16@4, pid u16@6, fwVer u16@8, bootloaderVer u16@10` [I] |
| `AA 55 21 <dev>` | Sub-device (dock/numpad) firmware version | version [U layout] |
| `AA 55 7F` | Reset device (reboot) | none. **Do not use casually.** |

### 4.2 Output-channel getters
| Request | Reply fields |
|---|---|
| `11 00` (FW info) | `fwVer u16@4` (display it as hex digits, e.g. `0x0105` → "105"), `u16@6` reserved, `b8` profile count + 1 (Base Camp subtracts 1 when it is non-zero), `effectModeIndex b9`, `currentProfile b10` (1-based, compared directly with slot numbers), `effectMenuIndex b11` [P] |
| `11 12` (layout) | `layout b4` [P] |
| `12 06 00 00` (game mode) | `b4` [P] |
| `12 07 00 00` (indicator LEDs) | `b4` ≠ 0 → on [P] |
| `12 08 00 00` (sync across profiles) | `b4` ≠ 0 → on [P] |
| `12 09 00 00` (game-mode status) | `b4` ≠ 0 → on [P] |
| `12 0A 00 00` (YETI status) | `b4` ≠ 0 → on [P] |
| `11 84 00 00` (clock format) | `b11` digital, `b12` 12-hour (≠ 0 → true) [P] |
| `1C 00 00 01` (display-key pics) | `b4..b7` = 4 picture ids [P] |
| `11 14 00 00` (extended info) | `b4..b32` = ExtendInfo (§6.4) [P] |

## 5. Lighting

### 5.1 Set effect: `14 2C` + EffectData[62] (bytes 2–63) [P]
| Byte | Field | Rule |
|---|---|---|
| 2 | effectIndex | send only for {0, 1, 3, 6, 9, 11, 12}; others use §5.3 |
| 3 | applyAll | 1 as sent by the app [P]; **send 0** [H] (see below) |
| 4 | speed | see §5.2; Static → `FF` |
| 5 | brightness | 0..100 |
| 6 | randColor | ColorType mapping ([03](03-data-model.md)); force 0 for Static (unless 2) and for YETI; `FF` for Off |
| 7 | direction | always `FF` |
| 8 | width | always `FF` |
| 9–11, 12–14, 15–17 | colour 1..3 | R, G, B |
| 18–20 | background colour | R, G, B |
| 21–63 | reserved | 0 |

Afterwards, persist it with Save (§8) using the entry's MenuIndex.

**applyAll [H].** Effects only work with applyAll 0. With applyAll 1:
- **Static** starts `14 2C 00 01`, which is the per-key colour chunk header (§5.5). The
  firmware parses it as chunk number `speed`: speed `FF` is out of range, so the request is
  NAKed. Speed 0..6 is ACKed, writes the following bytes over that chunk of per-key colours
  and switches the keyboard to Custom.
- **Breathing** is ACKed, but the keyboard switches to Custom. A following Save stores
  Custom into the menu slot.
- **Color Wave** (BlockData, §5.3) is ACKed and ignored.

With **applyAll 0** each effect is ACKed and shown immediately, and FW info b9 reports it.
Save then stores it into the menu slot, and the getters (§5.7) read it back. The firmware
normalises what it stores:
- applyAll `00`;
- direction and width `FF` → `00` (Tornado's width → `02`);
- Off's randColor `FF` → `00`, and Static's speed → `00`;
- Custom in the custom-mode layout (§5.4);
- with randColor 2 (random), the colour bytes are not updated and keep their previous
  values.

### 5.2 Speed mapping [P]
The app's speed is 0..100. Convert it before sending:

| UI speed bucket | <13 | 13–37 | 38–62 | 63–87 | ≥88 |
|---|---|---|---|---|---|
| Breathing, Reactive (1, 3, 5, 11) | 5 | 4 | 3 | 1 | 0 |
| Color Wave, Tornado (4, 7) | 10 | 9 | 8 | 7 | 6 |
| Matrix (9) | 20 | 15 | 10 | 5 | 0 |
| YETI (6) | 10 | 7 | 5 | 3 | 0 |
| others | value unchanged | | | | |

### 5.3 Block effects (Color Wave, Tornado): `14 2C` + BlockData[62] [P]
BlockData, at payload bytes 2–63:

| Byte | Field |
|---|---|
| 2 | effectIndex: 4 Color Wave, 5, 7 Tornado |
| 3 | applyAll: send 0, as for §5.1 [H] |
| 4 | speed (remapped as in §5.2) |
| 5 | brightness |
| 6 | randColor |
| 7 | direction |
| 8 | width |
| 9 | stopCount |
| 10–13, 14–17, 18–21, 22–25, … | up to 7 colour stops `{pos, r, g, b}` (`pos` 0..100, `FF` = unused) |
| 38–40 | background RGB |
| 41–63 | zero |

The library validates and rewrites the struct before sending [P]:

1. **Per-effect pre-checks:**
   - **Color Wave (4):** set stop0.pos = 100 and stop1.pos = `FF`. Direction must be 0, 2, 4 or 6; anything else is rejected (nothing is sent).
   - **Effect 5:** set stop0.pos, stop1.pos and direction to `FF`.
   - **Tornado (7):** set stop0.pos = 100 and stop1.pos = `FF`. Direction must be 9 or 10; anything else is rejected.
   - **Any other effect index:** rejected.
2. **If randColor = 2 (random):** stop0.pos = `FF`, stopCount = 0, width = 2.
3. **Otherwise, for Color Wave:** randColor = 0.
   - Two colours (the caller passed randColor 16): stops become colour1@25, colour2@50, colour1@75, colour2@100, stopCount = 4, width = 2.
   - Single colour: stopCount = 1, width = 0.
4. **Otherwise, for Tornado:** randColor = 0, width = 0, stopCount = 1.
5. **Otherwise:** width = 2.
6. Remap speed (§5.2), then send `14 2C` + the 62 bytes. Persist with Save (§8) using MenuIndex 1 (Color Wave) or 2 (Tornado).

### 5.4 Switch to Custom mode [P]
`14 2C 0A 00 FF <brightness>`, with **bytes 6–63 = `FF`**. This is the only `FF`-filled request.

### 5.5 Per-key static colours: 7 chunks [P]
The colour array is 126 × RGB (378 bytes), indexed by **LED index** (key table). Chunk *i*
(0..6) is `14 2C 00 01 <i> 4B 00` followed by bytes `[57i, 57i+57)` of the array at payload
offset 7. The last chunk carries 36 bytes.

### 5.6 Per-LED effect table: 3 chunks [P]
126 bytes, one EffectIndex per LED. Chunk *i* (0..2) is `14 A0 <i> 01` + bytes `[60i, 60i+60)`
at offset 4. The last chunk carries 6 bytes.

**Applying a Custom static layout (Base Camp's order)** [P]: Switch to Custom (§5.4) →
per-key colours (§5.5) → per-LED effect table with 0 for statically painted LEDs (§5.6) →
Save with MenuIndex 6.

### 5.7 Getters [P structure, I layouts]

[H] On an enabled profile (§6.4), these getters return what the last Save stored: the
effect data of that menu slot, the per-key colours and the per-LED table. Changes that were
sent but not saved are not visible. Writes to a disabled profile are dropped, so its
getters keep their old values.

| Request | Reply |
|---|---|
| `11 01 00 00 02` | multi-part form A: effect menu table: profile count, effect count, then 5 × {9 menu bytes, current index} |
| `11 01 00 01 <profile> <menu>` | single reply carrying the EffectData (62 bytes) of that slot; start offset [U] |
| `11 01 00 02 <profile> 02` | multi-part form B: per-key static colours (126 RGB) |
| `11 01 00 02 <profile> <menu> 02` | multi-part form B: per-key custom effect content [U] |
| `11 01 00 03 <profile> 02` | multi-part form B: per-LED effect table (126 bytes) |
| `11 01 00 04 <profile> <effect>` | single reply carrying EffectData for an effect value; start offset [U] |
| `11 01 00 05 <profile> 02` | multi-part form B: light-bar data [U]. If b4 is `0A` or `FF`, `remaining` is at b5 and the payload starts at b6 instead. |

### 5.8 Per-key custom effect: `ChangeCustomizeEffect` [P]
This command sets animated per-key effects in Custom mode (the static case is §5.5). The
input is a list of up to 171 entries `{led, r, g, b}`, sent 14 entries per chunk.

Each chunk is `14 2C 0A 00 <preview> <brightness> 00 <seq>` followed by up to 14 × 4 bytes
from b8:
- `preview`: `FF` for a live preview, `00` to apply.
- `seq`: 0 if the whole list is under 15 entries; otherwise 1 for the first chunk, then 3 if
  `total ≤ offset + 14`, else 2.

Unused entry slots in the last chunk are `FF`. Base Camp waits 1 ms between chunks and 3 ms
after the last one when not previewing.

### 5.9 Light bar, single-XY [U]
`14 2D` + BarData sets a light-bar effect. `14 2D 0A 00 FF …` (7 chunks of 57) sets bar
custom colours. `16 81 …` is a single-XY effect. Semantics unknown; skip.

## 6. Device settings

### 6.1 Toggles [P]
| Request | Meaning |
|---|---|
| `11 80 00 00 <0/1>` | "AP enable": host-assisted mode. Base Camp sends 1 before clock sync. Effect [U]. |
| `12 02` / `12 03` | main lighting off / on |
| `12 04` / `12 05` | key functions off / on [I: whether firmware handles Fn functions] |
| `12 06 00 01 <v>` | game mode value |
| `12 07 00 01 <0/1>` | indicator LEDs |
| `12 08 00 01 <0/1>` | sync key bindings across profiles |
| `12 09 00 01 <0/1>` | game-mode status |
| `12 0A 00 01 <0/1>` | YETI mode |
| `12 <0/1> 00 00 <delayMs>` | sync effect (live streaming mode) off/on; timeout 1000 ms |

### 6.2 Dock data feed [P]
| Request | Meaning |
|---|---|
| `11 81 <slot u16@2> <value u16@4>` | PC info value. Slots: 0 CPU, 1 GPU, 2 disk, 3 network, 4 RAM, 5 APM. Values are clamped to 0..100, except slot 5 which is clamped to 0..999. |
| `11 82 …` | audio EQ bars (21 bytes) for a dock visualiser. Layout [U]; skip. |
| `11 83 00 00 <volume>` | master volume 0..100 |

### 6.3 Clock [P]
`11 84 00 01 00 00 <month> <day> <hour> <minute> <second> <digital> <12h>` (bytes 6–12).
Read the current format first with `11 84 00 00` (§4.2) and send it back unchanged with the
new time.

### 6.4 Extended info: `11 14 00 01` + ExtendInfo (bytes 4–32); timeout 2800 ms [P]
ExtendInfo is **29 packed bytes**:

| Off | Field | Notes |
|---|---|---|
| 0 | dockPlug | read: bit0 dock attached on the right, bit1 attached on the left. Write: 2 [P as sent] |
| 1 | showMenu | page-enable bitmask ([03](03-data-model.md) DockSettings) |
| 2 | menuIndex | read: page the dock currently shows (§6.5). Write: 0 |
| 3–5 | menuColor | R, G, B |
| 6 | screenSetup | bit0 screen saver on, bit1 auto turn-off on; type bits [U] |
| 7–8 | screenSaver u16 | ≤ 1800 |
| 9–10 | turnOff u16 | ≤ 1800 |
| 11–15 | showProfile[5] | each non-zero, with a distinct low nibble, or the library rejects the whole write. Low nibble = profile number; bit `0x10` = profile **disabled** [H] (see below) |
| 16 | numpadPlug | read: bit0 right, bit1 left. Write: 2 |
| 17 | pixelShiftTime | |
| 18 | dockDoubleClick | ≤ 200; Base Camp writes 50 |
| 19–28 | brightness[5] × {dock, numpad} | `0x80 \| level`, level ∈ {0, 25, 50, 75, 100}; the library quantises other values to the nearest level. Reads back as `00`; a written byte without `0x80` keeps the current level [H] |

Validation is done host-side by the library [P]. Do it in your code too.

**Observed on hardware [H]:**
- **Read-modify-write is safe:** rewriting the block as read, with the write rules applied
  (dockPlug 2, menuIndex 0, numpadPlug 2) and the brightness bytes left at `00`, changes
  nothing visible. Writing menuIndex 0 does not change the page the dock shows.
- **Disabled profiles:** a keyboard set up with one profile reads `showProfile = 01 12 13 14 15`,
  so profiles 2–5 carry `0x10`. Clearing `0x10` enables the profile: switching to it then
  loads its lighting. Setting it disables the profile again. See §8 for what a disabled
  profile does.

### 6.5 Dock menu index values seen [P]
65 volume page · 97 CPU · 98 GPU · 99 disk · 100 network · 101 RAM · 113 APM ·
130 media-key passthrough mode (§[05](05-host-actions.md) §2.2).

### 6.6 Numpad display-key pictures [P]
`1C 00 00 00 <p1> <p2> <p3> <p4>` selects which stored picture each of the 4 display keys
shows. Uploading pictures is out of scope.

## 7. Keys

### 7.1 Remap: `14 20 <src u16@2> <dst u16@4>` [P]
`src` is the source fwKey. `dst` is a fwKey (plain remap) or a function target code (§7.2).
`dst = 255` restores the default; `dst = 0` disables the key.

### 7.2 Function target codes [P]
| Code | Function | Code | Function |
|---|---|---|---|
| 128 | Shutdown | 129 | Sleep / Hibernate |
| 195 | Calculator | 160..164 | Profile 1..5 |
| 210 | Profile cycle | 211 / 212 | Previous / next profile |
| 180 | Next track | 181 | Previous track |
| 182 | Stop | 183 | Play/Pause |
| 184 | Mute | 187 / 188 | Volume up / down |
| 230..241 | F13..F24 | 257 / 258 / 259 | Mouse left / right / middle |
| 260 / 261 | Mouse forward / back | 304 / 305 | Wheel up / down |

### 7.3 Shortcut remap: `14 21 <src u16@2> <dstKey b4> <mods b5>` [P]
`mods` is the modifier mask (Ctrl 1, Shift 2, Alt 4, Win 8). `dstKey` is a fwKey.

### 7.4 Macro write [P]
- Header: `14 18 <playback> 00 <repeat=1>`.
- Then ⌈n/15⌉ chunks: `15 <src fwKey> <count ≤ 15> <0 for the first chunk, 1 after> <count × event>`.
- Event (4 bytes): `key`, `(page << 4) | action`, `delay u16`.
  - page: 0 keyboard, 1 mouse.
  - action: 1 release, 2 press, 3 wheel.
  - key: fwKey for keyboard; mouse buttons 1/2/3; wheel 48 = down, 49 = up.
- The last event must be the terminator `FF FF 00 00`. At most 1000 events including it.
- `playback`: 1 once, 4 hold, 3 repeat.
- The macro is stored against `src` in the active profile [I].

### 7.5 Launch content write: chunks of `17 <src fwKey> <len b2> <seq b3> <data[len] @4>` [P]
- Data is `<LaunchKind byte> ‖ UTF-8 text`; arguments inside the text are separated by `~`.
- Chunk data is ≤ 60 bytes.
- `seq` [P, exact rule]: if the total length ≤ 60 then 0; otherwise the chunk at offset 0
  gets 1, and every later chunk gets 3 if `total < offset + 60`, else 2. So a payload whose
  length is an exact multiple of 60 (≥ 120) ends with a 2, not a 3. Reproduce this exactly.
- Base Camp waits about 1 ms between chunks.

### 7.6 Key content read: `11 02 00 01 <profile> <key> <interval>`; timeout 1000 ms [P]
- The multi-part reply (§9, form B) reassembles into
  `{size u16, keyType u8, idx1 u8, idx2 u8, events/data…}`.
- `keyType == 7` means launch content. Its data bytes are the payload written in §7.5:
  strip zero bytes, then read the leading kind byte.
- A NAK `FF AA FF 00` means the key has no content.
- Base Camp passes the **LED index** of the released key as `<key>` and uses `interval = 2`
  [P]. Whether the firmware expects an LED index or a fwKey here is **[U]**. Try the LED
  index first, as Base Camp does.

## 8. Profiles and persistence [P]
| Request | Meaning | Timeout |
|---|---|---|
| `14 00 00 00 <slot 1..5> <menuIndex 0..8>` | switch profile and select the active lighting entry (menuIndex is 0-based; 0 = Static is valid [H]) | 950 ms |
| `13 55 00 00 <menuIndex>` | save current state to flash | 1300 ms |
| `13 40 00 00 <all 0/1>` | reset flash (factory) | 12000 ms |
| `13 41 00 00 <profileMask>` | reset dock pictures | 5000 ms |
| `13 42 00 00 <p1..p5>` | reset numpad pictures per profile | 5000 ms |
| `13 60` | reset all key bindings | 3000 ms |
| `13 61` | reset all lighting effects | 3000 ms |

**Disabled profiles [H].** Switching to a profile whose `showProfile` entry has `0x10` set
(§6.4):
- the switch is ACKed and FW info reports that profile, but effect and menu read `FF`;
- the keyboard keeps showing the previous lighting;
- lighting, key and Save writes are ACKed but dropped.

Check the flag before switching or writing, and enable the profile first. Slot 6 is also
accepted. It maps to profile 0 of the lighting getters (§5.7) and holds Off in menus 1–8.

## 9. Multi-part replies [P]
- The first matching reply tells how many parts follow (a countdown). Each part's position is
  `index = total − remaining − 1`, with `total = first.remaining + 1`.
- Concatenate the payload slices in index order. The read is complete when the part with
  `remaining = 0` arrives.

| Form | Requests | `remaining` at | Payload slice |
|---|---|---|---|
| A | `11 01 00 00` | b6 | b7..b63 (57 bytes) |
| B | `11 02 …`, `11 01 00 02`, `11 01 00 03`, `11 01 00 05` | b4 | b5..b63 (59 bytes) |
| C | `19 …` (flash read) | b2 | b4..b63 (60 bytes); do not implement |

## 10. Unsolicited input reports [P]
| b0 | Meaning |
|---|---|
| `01` | Key-state bitmap. Bit *u* (byte `1 + u/8`, bit `u % 8`, counted from b0) is set while the key with HID usage *u* is down. Map usages to keys with `everest_max_layout.csv` (column `hid_usage` → `index`). See [05](05-host-actions.md). |
| `02`..`0A` | 9 chunks (63 bytes each) of the live LED colour frame. Used only for sync features. |

Some layout entries have usages above 503 (for example Profile 1..5 = 773..777). Those don't
fit a 64-byte bitmap, so how these events arrive is **[U]**. Capture a USB trace while pressing
profile keys.

# 10 · Everest 60 (VID `3282`, PIDs `0005`, `0006`)

The Everest 60 is a 60 % keyboard with an optional detachable numpad, a base layer and an
Fn layer. Feature-wise it mirrors the Everest Max (lighting, remaps, macros, launch
content, profiles, game/YETI modes). Its **wire protocol is different**: fixed-format
feature reports with 32-bit arguments, and key events are polled.

## 1. Transport [P]

| Item | Value |
|---|---|
| Interface | USB interface **2** (`mi_02`). Fall back to any interface of the device if interface 2 is missing. |
| Request | HID **SetFeature**, report ID `0`, 64-byte payload |
| Reply | HID **GetFeature**, report ID `0`, 64-byte payload. Read it after the per-command delay (§3). |
| Bootloader | `0C45:7040` (firmware update only; out of scope) |

**Request payload:**

| Bytes | Content |
|---|---|
| 0 | command code (§4) |
| 1–3 | constant `46 23 EA` |
| 4–7, 8–11, 12–15, 16–19 | arguments `a0..a3`, each u32 little-endian (unused = 0) |
| 20–63 | 0 |

**Data variant:** same bytes 0–3, followed by up to 60 raw data bytes at 4–63.

**Reply payload:** b0 echoes the command code; a mismatch means failure. Results are u32
little-endian values at b4, b8, b12, … (`r0`, `r1`, `r2`, …). Setters report success as
`r0 = 1`.

Run all commands through one worker queue per device: one in flight, in order.

## 2. Key events: polling [P]

The device does not push key reports. The host polls with command `08` (no arguments).
Reply:

| Byte | Meaning |
|---|---|
| 4 | numpad status: 0 none, 1 attached left, 2 attached right |
| 5 | *n* = number of key events (valid when 1..26) |
| 6 + 2*i*, 7 + 2*i* | event *i*: key code, state (1 = pressed, otherwise released) |
| 60, 61 | status bytes [U] |
| 62 | current profile |
| 63 | current effect / menu |

- Poll continuously, about every 10–20 ms [I]. Base Camp's interval constant isn't recovered.
- A change in b62 or b63 means the profile or effect changed on the device. Refresh the UI.
- A change in b4 means the numpad was attached or detached.
- **Key codes** in events are the keyboard's own key ids, the same space as `DLLMatrixIndex`
  in [`data/everest60_keys.csv`](../../data/everest60_keys.csv) [I].
- Host actions run on release, as on the Everest Max ([05](05-host-actions.md) §2.2).
  - Code **154** (Fn) is handled by the host as a profile-cycle trigger in Base Camp [P].
  - Fn-layer keys carry Fn-layer launch content (§4.6).

## 3. Timing [P]
Each command has a fixed delay between SetFeature and GetFeature. Base Camp uses one
default delay for most commands; begin/end commands of multi-part transfers wait twice as
long. The default value is **[U]**. Use 20 ms, and 40 ms for transfer ends, then adjust on
hardware.

## 4. Commands [P codes; argument meanings as noted]

### 4.1 Device and session
| Code | Operation | Arguments → result |
|---|---|---|
| `01` | sub-device info (numpad) | → r0, r1, r2 (version fields [I]) |
| `02` | device info | → r0..r3 (vid/pid/fw/bootloader [I]) |
| `03` / `04` | AP (host-assisted mode) on / off | → r0 |
| `05` | get layout | → r0 = layout id |
| `06` | set layout | a0 = layout id → r0 = 1 |
| `07` | FW info | → r0 = fwVer, r1 = reserved, r2 = profile count, r3 = effect mode, r4 = current profile, r5 = effect menu (one field per u32 slot; profile count as on the Max, see [04](04-protocol-everest-max.md) §4.2) |
| `08` | status / key-event poll | §2 |
| `0D` | reset device (reboot) | — |
| `20` | key functions on/off | a0 |

### 4.2 Toggles and modes
| Code | Operation |
|---|---|
| `0C` | main lighting on/off (a0) |
| `0E` / `0F` | set / get game-mode status |
| `10` / `11` | set / get indicator LEDs |
| `13` / `12` | set / get YETI mode |
| `15` / `14` | set / get game mode |
| `30` / `31` | set / get sync across profiles |
| `44` / `43` | set / get "show profiles" (set uses the data variant, 60 bytes) |

### 4.3 Profiles
| Code | Operation | Arguments |
|---|---|---|
| `16` | switch profile | a0 = profile (1..5), a1 = effect menu index → r0 = 1 |
| `40` | exchange two profiles | a0, a1 = profiles [I] |
| `41` | delete profile | a0 = profile [I] |
| `42` | insert profile | a0 = position [I] |
| `1A` | save to flash | a0 = menu index [I] |
| `1B` | reset flash (factory) | a0 = all [I] |
| `1C` | reset effects | — |
| `21` | reset keys | — |

### 4.4 Lighting
| Code | Operation | Payload |
|---|---|---|
| `17` | set effect | data variant: the first 60 bytes of EffectData (same field order as Everest Max EffectData, [04](04-protocol-everest-max.md) §5.1, starting at payload b4) |
| `18` | set block effect (Color Wave/Tornado) | data variant: BlockData [I same layout as Max] |
| `19` | set light-bar effect | data variant [U] |
| `1D` | get profile effect table | → reply data |
| `1E` / `1F` | get effect content / current effect content | → reply data |
| `24` | get per-key custom content | a0 = profile [I] |
| `32`, `34`, `35`, `36` | per-key custom effect transfer (begin / data / end variants) | [U] layout |
| `28` | get live colour frame | — |

**Everest 60 effect ids** [P]: Static 1, Colorwave 2, Tornado 3, Breathing 4, Reactive 5,
Matrix 6, Custom 7, Yeti 8, Off 9. These differ from the Everest Max ids. The UI offers
speed and brightness in steps of 0, 25, 50, 75, 100 [P].

### 4.5 Keys
| Code | Operation | Arguments |
|---|---|---|
| `22` | remap (base layer) | a0 = source key, a1 = target code [I order] |
| `4F` | remap (Fn layer) | as `22` |
| `23` | shortcut remap | a0 = source, a1 = target key, a2 = modifier mask [I] |
| `29` | single-event macro | a0..a3 = source, key type, data index 1, data index 2 [I] |
| `45` | single-event macro, Fn layer | as `29` |

Remap targets and modifier masks use the same values as the Everest Max
([04](04-protocol-everest-max.md) §7.2–7.3) [I].

### 4.6 Multi-part transfers [P]
Each transfer is a **begin** command, then **data** chunks, then an **end** command (which
uses the longer delay):

| Transfer | Begin | Data | End |
|---|---|---|---|
| Macro, base layer | `25` | `26` | `27` |
| Macro, Fn layer | `49` | `4A` | `4B` |
| Launch content, base layer | `2A` | `2B` | `2C` |
| Launch content, Fn layer | `4C` | `4D` | `4E` |
| Read key content, base layer | `2D`, then `2F` | `2E` (repeated) | – |
| Read key content, Fn layer | `46`, then `48` | `47` (repeated) | – |

- **Macro begin:** a0 = source key (1..255), a1 = playback type (≤ 4; values as on the Max:
  1 once, 4 hold, 3 repeat), a2 = repeat count (1), a3 = total event count including the
  terminator.
- **Macro data** (data variant): u32 count of events in this chunk (14 for full chunks [P]; the
  value for a shorter last chunk is [I] the actual count), then
  `count × 4` event bytes in the Everest Max event format ([04](04-protocol-everest-max.md)
  §7.4).
- **Launch begin:** a0 = source key, a1 = total byte length.
- **Launch data** (data variant): u32 chunk length (56 for full chunks [P]; shorter last chunk
  [I]), then the bytes. The payload format
  (kind byte + UTF-8 text) is as on the Max.
- **Reads** [P codes and call order, I roles]: Base Camp sends `2D` and then `2F` (`46`, `48` on
  the Fn layer), and then repeats `2E` (`47`) until it has the total size (1..255 events).
  The first replies probably give the size; each repeated reply returns the next chunk.

## 5. Keys and data

- [`data/everest60_keys.csv`](../../data/everest60_keys.csv): base layer, 116 keys.
- [`data/everest60_fn_keys.csv`](../../data/everest60_fn_keys.csv): Fn layer, 116 keys,
  `LayerType = 3`.

Columns are as in the Everest Max table. `DLLKeyId` is the source/target key id,
`DLLMatrixIndex` is the event/LED key id (up to 231), and `KeyId` is the Windows VK.

## 6. Host behaviour [P]
- **Attach notification** reports the main keyboard and numpad position (none, left,
  right). Show this in the UI.
- Clock sync, PC info and the media dock **do not apply**: the Everest 60 has no dock.
- App-linked profiles, macros library, host actions, hotkey switch and multi-action work as
  on the Everest Max.

## 7. Acceptance vectors

| # | Operation | Expected payload |
|---|---|---|
| E1 | Status poll | `08 46 23 ea` + 60 × `00` |
| E2 | Switch to profile 3, menu 2 | `16 46 23 ea 03 00 00 00 02 00 00 00 …00` |
| E3 | Set layout 1 | `06 46 23 ea 01 00 00 00 …00` |
| E4 | Macro begin: src 30, once, 7 events | `25 46 23 ea 1e 00 00 00 01 00 00 00 01 00 00 00 07 00 00 00 …00` |
| E5 | Parse poll reply `08 … b4=02 b5=01 b6=1e b7=01 … b62=02 b63=04` | numpad right; key 30 pressed; profile 2; effect 4 |

# 11 · Makalu 67 (VID `3282`, PIDs `0003`, `0004`)

The Makalu 67 is a 6-button gaming mouse with up to 5 on-board profiles. Each profile holds
a sensor block (polling rate, response time, angle snapping, lift-off distance, 5 DPI
levels), a lighting block (8 LEDs) and 6 button bindings. Everything is stored on the
mouse. The host only reacts to "run program/URL" buttons and to state-change
notifications.

Which PID corresponds to which product variant is **[U]**. Treat both the same way.

## 1. Transport [P]

| Item | Value |
|---|---|
| Interface | USB interface **0** (`MI_00`) |
| Command channel | HID **SetFeature**, report ID `A1`, 64 bytes including the ID: `A1 <group> <sub> 00 00 <args…>` |
| Status | HID **GetFeature**, report ID `A1`, 64 bytes. **b1 = 1** means success; **b1 = 3 or 4** means busy, so poll again. Values are returned from **b16**. |
| Bulk channel | HID **SetFeature / GetFeature**, report ID `A0`, **1041 bytes** including the ID (§5) |
| Events | 9-byte input reports on the same interface (§6) |

**Write-then-read cycle:**
1. SetFeature `A1 …`.
2. Sleep the command's delay (table below).
3. GetFeature `A1` and check b1.

If the SetFeature itself fails with a transient OS error, retry it up to 3 more times, 50 ms
apart. All commands are serialised by one lock.

Byte numbering below counts the report ID as **b0**.

## 2. Commands [P]

| Group, sub | Operation | Request fields | Reply (from b16) | Delay |
|---|---|---|---|---|
| `02 00` | firmware version (also used for "info" and bootloader version) | – | b16–b17 = version u16 | 80 ms (50 ms for info/bootloader) |
| `1A 00` | MCU type | – | b16 | 50 ms |
| `0B 01` | reset profile to default | b5 = profile | – | 50 ms |
| `0B 02` | sync profile | b5 = profile | – | 50 ms |
| `0B 03` | get current profile | – | b16 = profile (1..5) | 60 ms |
| `0B 04` | set current profile | b5 = profile | – | 100 ms |
| `0B 05` | add profile | b5 = profile | – | 50 ms |
| `0B 06` | delete profile | b5 = profile | – | 50 ms |
| `0B 07` | read profile data | b5 = profile | full profile record, layout **[U]** | 80 ms |
| `0B 09` | number of profiles | – | b16 (1..5) | 60 ms |
| `0B 0A` | copy profile | b5 = source, b6 = target | – | 50 ms |
| `0D 01` | polling rate | b5 = profile, b6 = code: **1** = 1000 Hz, **2** = 500, **4** = 250, **8** = 125 | – | 50 ms |
| `0D 02` | button response time | b5 = profile, b6 = time (ms [I]) | – | 50 ms |
| `0D 03` | angle snapping | b5 = profile, b6 = 0 off / 1 on | – | 30 ms |
| `0D 04` | lift-off distance | b5 = profile, b6 = 0 low / 1 high | – | 50 ms |
| `0D 0A` | DPI settings | b5 = profile, b6 = level count (1..5), b7 = current level; b8–b15 = 0; b16–b35 = 5 × (X u16, Y u16) | – | 50 ms |
| `0D 0F` | write the whole sensor block | b5 = profile; b16 polling code, b17 response time, b18 angle snapping, b19 lift-off, b20–b41 DPI block (count, current, 5 × X/Y) | – | 50 ms |
| `0D A4` | start surface calibration | – | – | 70 ms |
| `0D A5` | set surface | b6–b7 = surface u16 (`{varA, varB}`) | – | 100 ms |
| `0D A6` | reset surface | – | – | 70 ms |
| `0D A7` | get calibration result | – | result, then surface `{varA, varB}` (b16–b18 [I offsets]) | 100 ms |
| `0C 00` | lighting, **saved** | b5 = profile; lighting block at b16 (§3) | – | 100 ms |
| `3C 00` | lighting, **live only** (not saved) | lighting block at b16 | – | 10 ms |
| `0A 00` | button binding | b5 = profile, b6 = button (1..6); button record at b16 (§4) | – | 50 ms |
| `0E 00` | factory reset | – | – | 50 ms |

Profile ids are 1..5. Some calls also accept 6 as "all profiles" (`ALL_PROFILE`) [P enum, I behaviour].

## 3. Lighting block [P]

| Byte | Field | Values |
|---|---|---|
| b16 | effect | 0 off, 1 static, 2 colour wave, 3 tornado, 4 breathing 1-colour, 5 breathing 2-colour, 6 breathing 7-colour, 7 reactive, 8 YETI, 15 custom (per-LED) |
| b17–b40 | 8 × RGB | LED 0..7 colours |
| b41 | brightness | 0, 25, 50, 75, 100 |
| b42 | direction | 0 clockwise, 1 counter-clockwise |
| b43 | speed | 0 slow, 1 normal, 2 fast |

## 4. Button record (`0A 00`, b16–b22) [P]

| Byte | Field |
|---|---|
| b16 | function type (table below) |
| b17 | parameter |
| b18–b19 | aux u16 |
| b20–b21 | aux2 u16 |
| b22 | `0F` |

| Binding | Type | Parameter / aux |
|---|---|---|
| Mouse button | `00` | parameter = bitmask: 1 left, 2 right, 4 middle, 8 back, 16 forward |
| Wheel | `01` | parameter = 1 up, `FF` down |
| Keyboard key | `02` | parameter = modifier byte [I]; aux = key code (HID usage [I]) |
| Macro | `03` | parameter: playback 0 → `01`, 1 → `F1`, 2 → `F2` [P mapping, I meaning: once / hold / toggle]; aux = loop count; aux2 = event count. Events go through the bulk channel (§5). |
| Profile | `08` | parameter `F1` = next, `F3` = previous |
| DPI | `09` | parameter `F1` = next, `F3` = previous |
| DPI "sniper" (hold for fixed DPI) | `0C` | aux = aux2 = sniper DPI |
| Brightness cycle | `21` | – |
| Effect cycle | `22` | – |
| Media | `20` | parameter = HID consumer usage: Play/Pause `CD`, Stop `B7`, Previous `B6`, Next `B5`, Volume+ `E9`, Volume− `EA`, Mute `E2`; Media select: aux = `0183`. Mute mic [U]. |
| OS command | `18` | parameter `01`; carries a built-in macro (e.g. task manager, lock) through the bulk channel. Per-command event lists are **[U]**; build them as macros yourself. |
| Run program | `23` | aux = path length; the path goes through the bulk channel (§5) |
| Run browser | `24` | aux = URL length; the URL goes through the bulk channel |
| Disabled | `FF` | – |

**Macro events (bulk payload)** [P]:
- Each event is 7 bytes: `kind`, `code u16`, `make`, `delay` (3 bytes, ms [I]).
- `kind`: `10`, `11` (mouse classes [I]), `01` keyboard, `00` other.
- `make`: 1 = press, 0 = release [I].
- The list ends with 7 zero bytes.
- At most 140 events.

## 5. Bulk channel (report `A0`) [P]

**Write** (SetFeature, 1041 bytes):

| Byte | Content |
|---|---|
| 0 | `A0` |
| 1 | `21` |
| 2 | button (1..6) |
| 3–4 | 0 |
| 5 | profile |
| 6 | 0 |
| 7–8 | checksum u16 = 16-bit sum of bytes 16..1039 |
| 9–15 | 0 |
| 16–1039 | 1024 data bytes (macro events, or a path/URL as text) |
| 1040 | 0 |

After the write, sleep 100 ms, then read `A1` status. Base Camp sends the record (`0A 00`)
first. If the status is OK and the type is `03` or `18`, it uploads the bulk block, retrying
once on failure.

**Read:** send `A1 25 <event b4> 00 00 <event b5>`, then GetFeature `A0` (1041 bytes). The data
starts at b16. The host uses this to fetch a button's stored path or URL.

## 6. Events (9-byte input reports) [P]

| b1 | Meaning | Host action |
|---|---|---|
| `23` | "run program" button pressed; b4, b5 identify the stored entry (profile, button [I order]) and are passed unchanged into the bulk read request | read the path (§5 read), then start it as the user. A leading `~` separates arguments [I]. |
| `24` | "run browser" button pressed | read the URL; open it only if the scheme is http or https |
| `01` | profile changed on the mouse; b2 = new profile | update the selected profile and refresh the UI |
| `02` | DPI changed on the mouse | refresh the sensor/DPI view |
| `03`, `04` | lighting changed (brightness or effect cycle) | refresh the lighting view |
| `05` | other setting changed [U] | refresh the profile |
| `06` | calibration result: b2, b3, b4 | finish the calibration flow |

Throttle event handling to one every 50 ms (as Base Camp does).

## 7. Functional notes [P]

- **DPI levels:** 1..5 levels per profile, each with separate X and Y DPI. The UI can add,
  delete and select the active level.
- **Surface calibration flow:** start (`0D A4`), the user moves the mouse, read the result
  (`0D A7`), then save it (`0D A5`). Reset with `0D A6`.
- **Live lighting preview** uses `3C 00`. Apply with `0C 00`.

## 8. Acceptance vectors

| # | Operation | Expected (b0 = report ID) |
|---|---|---|
| K1 | Polling 500 Hz, profile 1 | `a1 0d 01 00 00 01 02 00 …00` (64 bytes) |
| K2 | DPI, profile 2, 3 levels, current 2, (800,800) (1600,1600) (3200,3200) | `a1 0d 0a 00 00 02 03 02` + 8 × `00` + `20 03 20 03 40 06 40 06 80 0c 80 0c` + 8 × `00` + rest 0 |
| K3 | Set current profile 4 | `a1 0b 04 00 00 04 00 …00` |
| K4 | Live lighting: static, all LEDs red, 100 %, cw, normal | `a1 3c 00 00 …` with b16 = `01`, b17–b40 = `ff 00 00` × 8, b41 = `64`, b42 = `00`, b43 = `01` |
| K5 | Button 4, profile 1 → media Next | `a1 0a 00 00 00 01 04 …` with b16 = `20`, b17 = `b5`, b22 = `0f` |
| K6 | Status reply b1 = 3 | busy → poll again |

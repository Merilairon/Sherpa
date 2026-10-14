# Everest Max HID protocol (from Base Camp 1.9.10)

Source: `SDKDLL.dll` (x86, MFC) shipped in `BaseCamp_V1.9.10.msi`, decompiled with
Ghidra 11.4.2 and cross-checked in radare2 where Ghidra lost register arguments.
Addresses are virtual addresses in that DLL. None of this has been checked against
a physical keyboard yet; see [the report](reverse-engineering-report.md#what-remains-hidden).

## Transport

| Item | Value | Evidence |
|---|---|---|
| VID / PID | `3282:0001` | ctor `FUN_10007f10`: `+0x538 = 0x3282`, `+0x53c = 1` |
| Collection | usage page `0xFF00`, usage `0x0001` | ctor: `+0x540 = 1`, `+0x544 = 0xff00`; selected in `FUN_10008880` |
| Write | `WriteFile(caps.ReportID ‖ 64-byte payload)` | `FUN_10030570` copies 64 bytes after the report-ID byte |
| Completion | next input report whose bytes 0–3 equal the request's bytes 0–3 | reader `FUN_100077b0` compares `+0x4c8..0x4cb` |
| Reject | input report `FF AA …` | `FUN_1000be60`, reader |
| Timeout | 600 ms (1300 ms for SaveFlash) | `FUN_1000be60` default, `FUN_10009e10` |
| Serialisation | one critical section around every command | `+0x478` |
| Bootloader (IAP) | `060B:7801` | `FUN_10005220` (`IAP_CUSBDriver`) |

Unsolicited input reports on the same collection:

* `01 …`: key-state bitmap. The SDK diffs it against the previous state and calls the
  `SetKeyCallBack` function, which is how Base Camp runs host-side actions (launch app, OBS, media).
* `02 … 0A`: nine chunks of the live LED colour frame (`GetColorData`).

All payloads are 64 bytes, zero-filled unless noted.

## Commands

### Session / persistence

| Export | Bytes | Notes |
|---|---|---|
| `APEnable(on)` → `FUN_10008f60` | `11 80 00 00 <on>` | Base Camp sends `1` before host-driven features (clock) |
| `EnableKeyFunc(on)` → `FUN_100096a0` | `12 <04 \| 05>` | `05` = on |
| `SaveFlash(menu)` → `FUN_10009e10` | `13 55 00 00 <menu>` | `menu` = `Lighting.MenuIndex` 0..8 |

### Lighting

`Lighting.EffectIndex`: Static 0, Breathing 1, Reactive 3, ColorWave 4, Reactive-b 5, Yeti 6,
Tornado 7, Matrix 9, Custom 10, Reactive-c 11, Off 12.
`Lighting.MenuIndex` (SaveFlash slot): Static 0, Colorwave 1, Tornado 2, Breathing 3,
Reactive 4, Matrix 5, Custom 6, Yeti 7, Off 8.

**ChangeEffect** (`0x10006c60` → `FUN_1000a310`): `14 2C` + `EffData` (62 bytes)

| Off | EffData field | Value Base Camp sends |
|---|---|---|
| 2 | `byEffectIndex` | EffectIndex; the SDK drops the call unless ∈ {0,1,3,6,9,11,12} |
| 3 | `byAll` | 1 from the lighting page |
| 4 | `bySpeed` | UI 0..100, **remapped inside the SDK** (`FUN_10005950`); Static → `FF` |
| 5 | `byLightness` | brightness 0..100 |
| 6 | `byRandColor` | 0 = one colour, 16 = two colours, 2 = random (SDK forces 0 for Static/Yeti, `FF` for Off) |
| 7 | `byDirection` | forced `FF` by SDK |
| 8 | `byWidth` | forced `FF` by SDK |
| 9–17 | `colorLv[3]` | RGB × 3 |
| 18–20 | `bkColor` | RGB (Reactive/Matrix/Yeti background) |
| 21–63 | `byData[43]` | zero |

Speed remap (`FUN_10005950`): input buckets `<13, <38, <63, <88, ≥88` map to

| Effect | buckets → firmware value |
|---|---|
| Breathing, Reactive (1,3,5,11) | 5, 4, 3, 1, 0 |
| ColorWave, Tornado (4,7) | 10, 9, 8, 7, 6 |
| Matrix (9) | 20, 15, 10, 5, 0 |
| Yeti (6) | 10, 7, 5, 3, 0 |

**ChangeBlockEffect** (`0x10006d40`) carries Color Wave and Tornado (`BlockData`: up to
two positioned colour stops `{pos,r,g,b}`). The SDK rewrites several fields per effect;
it is not implemented in the Linux tool yet.

**SwitchToCustomizeEffect(brightness)** (`FUN_1000a3d0`), `FF`-filled:

```
14 2C 0A 00 FF <brightness> FF FF …
```

**ChangeCustomizeStatic** (`FUN_1000a700`): 126 RGB triplets indexed by `DLLMatrixIndex`
(378 bytes) in 7 chunks of 57:

```
14 2C 00 01 <chunk 0..6> 4B 00 <57 bytes RGB…>
```

**SetCustomizeTable** (`FUN_1000a480`): one effect id per LED (126 bytes) in 3 chunks of 60:

```
14 A0 <chunk 0..2> 01 <60 bytes>
```

Base Camp applies a static per-key layout as: SwitchToCustomizeEffect → ChangeCustomizeStatic →
SetCustomizeTable → SaveFlash(6).

### Key remap and macros

**ChangeKey(src, dst)** → `FUN_1000b4e0`: `14 20 …`. Arguments are passed in registers and
were not recovered. Base Camp resets a key with `ChangeKey(src, 255)`.

**SetFullMacroData(src, type, times, n, events)** (`0x100074b0` → `FUN_1000b9d0`), `n ≤ 1000`:

```
header:  14 18 <type> 00 <times>
chunks:  15 <src> <count ≤ 15> <0 first | 1 continuation> <count × 4-byte event>
```

`KEYMAP_EVENT` (4 bytes):

| Byte | Meaning |
|---|---|
| 0 | key: firmware key code (`DLLKeyId`), mouse button 1/2/3, wheel 48 down / 49 up, `FF` end |
| 1 | `page << 4 \| action`. Page 0 = keyboard, 1 = mouse, F = end. Action 1 = release, 2 = press, 3 = wheel, F = end |
| 2–3 | delay before the next event, ms, little-endian |

Base Camp always appends the terminator `FF FF 00 00`. `type`: 1 = "play-one", 4 = "play-two",
3 = anything else (`GetMacroInputContent`). `times` is always 1.
`src` is the `DLLKeyId` of the key that triggers the macro. There is no profile argument.

## Key table

`basecamp_linux/data/everest_max_keys.csv` (126 keys, US layout) comes from
`EverestDataHelper.GetEverestKeys_English`. Columns: firmware key code, LED index,
Windows VK code (what Base Camp's recorder stores), name.

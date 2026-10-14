# 08 · MacroPad (VID `3282`, PID `0008`)

The MacroPad is a 12-key keypad with per-key RGB. It speaks the **same protocol as the
Everest Max** ([04](04-protocol-everest-max.md)): same transport, request/response engine,
opcodes and reply forms. This document lists only the differences. Anything not mentioned
here behaves as in 04.

## 1. Transport and addressing [P]

| Item | Value |
|---|---|
| Interface | top-level collection Usage Page `FF00`, Usage `01` (as in 04 §1) |
| Several devices | Several MacroPads can be attached at once. The host gives each attached unit a small integer id on attach and remembers a persistent identity string (device instance path) for it. Profiles are stored per identity (`Profile.deviceId` / `deviceGuid`). |
| Ids on the wire | **None.** The id only selects which open handle a command goes to. Packets are identical to the Everest Max ones. On Linux, use one hidraw node per unit and key it by its `HID_UNIQ` / USB serial or physical path. |

## 2. Keys [P]

Data: [`data/macropad_layout.csv`](../../data/macropad_layout.csv).

| Key | Index (lighting) | Matrix (bindings, events) | fwKey | HID usage (bitmap bit) |
|---|---|---|---|---|
| M1 | 0 | 8 | 170 | 329 |
| M2 | 1 | 17 | 171 | 330 |
| M3 | 2 | 26 | 172 | 331 |
| M4 | 3 | 35 | 173 | 332 |
| M5 | 4 | 44 | 174 | 333 |
| M6 | 5 | 53 | 175 | 334 |
| M7 | 6 | 62 | 176 | 335 |
| M8 | 7 | 71 | 177 | 368 |
| M9 | 8 | 80 | 178 | 369 |
| M10 | 9 | 89 | 179 | 370 |
| M11 | 10 | 98 | 220 | 371 |
| M12 | 11 | 125 | 221 | 372 |

There are three numbering spaces, and each command uses a specific one:
- **index 0..11**: per-key colour and effect arrays (§3);
- **matrix**: key events, launch-content readback, and binding lookups in the host store;
- **fwKey**: remap source/target, macro source, launch-content source.

Macros recorded from a keyboard use the Everest Max key table
([`data/everest_max_keys.csv`](../../data/everest_max_keys.csv)) to turn recorded keys into
fwKeys, so a MacroPad macro can type any keyboard key [P].

## 3. Differences from the Everest Max protocol

| Command | MacroPad behaviour [P] |
|---|---|
| Per-key static colours (04 §5.5) | 12 × RGB = 36 bytes in a **single** chunk: `14 2C 00 01 00 4B 00` + 36 bytes |
| Per-LED effect table (04 §5.6) | 12 bytes in a **single** chunk: `14 A0 00 01` + 12 bytes |
| Per-key custom effect (04 §5.8) | at most 12 entries |
| Extended info (04 §6.4) | 16 bytes: 11 unused bytes, then `showProfile[5]`. Same opcode `11 14 00 01` with the data at b4. |
| Not present | media dock, numpad, clock, PC info, volume, EQ, display-key pictures, game mode, YETI mode, indicator LEDs |
| Present | FW info/layout, AP enable, sync effect, main brightness, key functions, sync across profiles, save/reset flash, reset effects/keys, profile switch, all lighting commands, remap, shortcut remap, launch content, macros, key-content read |

## 4. Host behaviour [P]

- **Attach and detach:** notify the UI with the unit id and its identity. Profiles and
  bindings load per identity.
- **Key release:** the same flow as [05](05-host-actions.md) §2.2, using the MacroPad's own
  matrix numbers.
  - There are no device-state codes (§2.1 of 05 does not apply).
  - The device's current profile is still compared with the store first.
- Launch-content kinds, hotkey switch and multi-action all behave as on the Everest Max.

## 5. Acceptance vectors

| # | Operation | Expected payload |
|---|---|---|
| MP1 | All 12 keys red | `14 2c 00 01 00 4b 00` + `ff 00 00` × 12, rest 0 |
| MP2 | Effect table, all static | `14 a0 00 01` + `00` × 12, rest 0 |
| MP3 | Remap M1 → Mute | `14 20 aa 00 b8 00 …00` |
| MP4 | Extended info, showProfile 1..5 | `11 14 00 01` + `00` × 11 + `01 02 03 04 05`, rest 0 |

# 07 · Acceptance tests

An implementation is acceptable when every golden vector below is reproduced byte for byte,
and every behavioural test passes against a fake device. Packets are shown as payload
bytes. "…00" means the rest of the 64 bytes are zero.

## 1. Golden request packets

| # | Operation | Expected payload |
|---|---|---|
| G1 | Switch to profile slot 2 with menu index 3 | `14 00 00 00 02 03 …00` |
| G2 | Remap Esc (fwKey 110) → Mute (184) | `14 20 6e 00 b8 00 …00` |
| G3 | Remap Esc → mouse left (257) | `14 20 6e 00 01 01 …00` |
| G4 | Esc → Win+L (fwKey 39, mods 8) | `14 21 6e 00 27 08 …00` |
| G5 | Restore Esc default | `14 20 6e 00 ff 00 …00` |
| G6 | Clock 14 Oct, 13:05:09, digital, 24 h | `11 84 00 01 00 00 0a 0e 0d 05 09 01 00 …00` |
| G7 | PC info APM = 321 | `11 81 05 00 41 01 …00` |
| G8 | PC info CPU = 150 (clamped) | `11 81 00 00 64 00 …00` |
| G9 | Volume 37 | `11 83 00 00 25 …00` |
| G10 | Game mode on | `12 06 00 01 01 …00` |
| G11 | Indicator LEDs off | `12 07 00 01 00 …00` |
| G12 | Save flash, menu 6 | `13 55 00 00 06 …00` |
| G13 | Switch to Custom, brightness 80 | `14 2c 0a 00 ff 50` then `ff` × 58 |
| G14 | Static red, brightness 100 | `14 2c 00 00 ff 64 00 ff ff ff 00 00 …00` (applyAll 0, see 04 §5.1 [H]) |
| G15 | Breathing, speed 50, colour `00a0ff`, brightness 100 | `14 2c 01 00 03 64 00 ff ff 00 a0 ff …00` (applyAll 0, see 04 §5.1 [H]) |
| G16 | Launch content F1 (fwKey 112) → URL `https://mountain.gg` | `17 70 14 00 02 68 74 74 70 73 3a 2f 2f 6d 6f 75 6e 74 61 69 6e 2e 67 67 …00` |
| G17 | Launch content, 130-byte payload | 3 chunks with b2/b3 = `3c 01`, `3c 02`, `0a 03` |
| G18 | Launch content, 120-byte payload | 2 chunks with b2/b3 = `3c 01`, `3c 02` |
| G19 | Extended info (see below) | `11 14 00 01 02 09 00 ff 80 00 03 2c 01 08 07 01 02 03 04 05 02 00 32 e4 b2 e4 b2 e4 b2 e4 b2 e4 b2 …00` |

G19 input: dockPlug 2, pages = clock + volume (`0x09`), menuIndex 0, colour `ff8000`,
screen saver and turn-off enabled, screen saver 300, turn-off 1800, showProfile 1..5,
numpadPlug 2, pixelShift 0, doubleClick 50, dock brightness 100 % and numpad brightness 50 %
for all profiles.

### 1.1 Per-key colours (G20)
126 LEDs where LED *i* = (i, i, i):
- 7 packets; packet *k* starts `14 2c 00 01 <k> 4b 00`.
- Packet 1 begins its data with `13 13 13` (LED 19).
- Packet 6 carries 36 data bytes, then zeros.

### 1.2 Per-LED effect table (G21)
Entries 0..125 = their own index: 3 packets `14 a0 <k> 01`. Packet 2's data is
`78 79 7a 7b 7c 7d` followed by zeros.

### 1.3 Macro (G22)
Source F12 (fwKey 123 = `7b`), play once, 16 identical events (key 36 press, delay 0x1234):
- `14 18 01 00 01 …00`
- `15 7b 0f 00` + 15 × `24 02 34 12`
- `15 7b 02 01 24 02 34 12 ff ff 00 00 …00`

## 2. Reply parsing vectors

| # | Input | Expected |
|---|---|---|
| R1 | Reply to `11 00`: `11 00 00 00 05 01 00 00 06 02 03 04 …` | fwVer "105", profile count 5, effect mode 2, current profile 3, menu index 4 |
| R2 | Reply to `11 84 00 00` with b11 = 1, b12 = 0 | digital, 24 h |
| R3 | Any reply `ff aa …` | NAK → operation fails |
| R4 | No matching reply within 600 ms | timeout → operation fails |
| R5 | Form-B reply parts with b4 = 2, 1, 0 and 59-byte payloads P0, P1, P2 | assembled = P0‖P1‖P2 |
| R6 | Form-B parts that arrive with b4 = 1 then 0, after a first part with b4 = 2 | slices placed at indices 1 and 2 |
| R7 | Input report `01` with payload bit for usage `0x29` set (b6 = `0x02`) | key-down for layout index 0 (Esc) |
| R8 | Next `01` report with that bit clear | key-up for index 0 → host dispatch runs |
| R9 | Key content with keyType 7 and data `02 68 74 74 70 3a 2f 2f 61 00 00` | open URL `http://a` |
| R10 | Key content kind 1 with a path that doesn't exist | nothing runs |
| R11 | Kind 2 with `file:///etc/passwd` | nothing runs |

## 3. Behavioural tests (fake device)

| # | Scenario | Pass condition |
|---|---|---|
| B1 | Two clients issue commands at the same time | packets on the wire never interleave; every multi-chunk write stays contiguous |
| B2 | A chunk of a multi-chunk write gets a NAK | the remaining chunks are not sent; the operation reports failure; the store is unchanged |
| B3 | Device attaches | clock sync is sent about 10 s later; then every 30 min |
| B4 | Dock reports menu index 97 | CPU % is pushed every 1 s; it stops when the menu index changes |
| B5 | Save macro "M" bound to keys in two profiles | both keys receive a fresh macro write |
| B6 | Process `foo` starts (linked to slot 3, revert on) and then exits | `14 00 00 00 03 <menu>` is sent; on exit the previous slot is restored |
| B7 | Device sends profile-key event 172 | the store's selected profile becomes slot 2; the UI is notified |
| B8 | Hotkey switch, 3 releases | fires value, value2, value |
| B9 | Media key (index 35) with launch content, dock menu index ≠ 130 | no host action |
| B10 | Extended info with duplicate showProfile low nibbles | the write is rejected before it reaches the device |
| B11 | Daemon IPC from another UID | refused |

## 4. Device-specific vectors

Each device document ends with its own vectors: MacroPad ([08](08-macropad.md) §5),
DisplayPad ([09](09-displaypad.md) §7), Everest 60 ([10](10-everest-60.md) §7) and
Makalu 67 ([11](11-makalu-67.md) §8). They are part of the acceptance criteria.

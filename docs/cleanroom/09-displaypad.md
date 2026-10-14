# 09 · DisplayPad (VID `3282`, PID `0009`)

The DisplayPad has 12 keys, each with its own LCD (102×102 pixels), and the key surface
forms one 800×240 panel. It has no RGB lighting. All actions run **on the host**: the device
reports key events, and the host draws icons and runs bindings.

## 1. Transport [P]

The device exposes two vendor collections, and both must be opened:

| Channel | Collection | Report size | Used for |
|---|---|---|---|
| Command | Usage Page `FF00`, Usage `01` | 64-byte payload | Everest-style commands and replies ([04](04-protocol-everest-max.md) §1–2 rules apply) |
| Bulk | Usage Page `FF01`, Usage `02` | **1024-byte** output reports | image pixel data |

Several DisplayPads can be attached at once. Addressing works as on the MacroPad
([08](08-macropad.md) §1).

## 2. Keys [P]

Data: [`data/displaypad_layout.csv`](../../data/displaypad_layout.csv), which is identical
to the MacroPad's.

| Key | Icon / layout index | Matrix | fwKey | HID usage |
|---|---|---|---|---|
| 1..12 | 0..11 | 8, 17, 26, 35, 44, 53, 62, 71, 80, 89, 98, 125 | 170..179, 220, 221 | 329..335, 368..372 |

- Icon commands address keys by **icon index 0..11**.
- Key events and the host store use the **matrix** number. The mapping is the table row.
- Index 0 is the top-left key [I] (the order the UI draws them).

## 3. Commands

### 3.1 Shared with the Everest Max [P]
These are identical to [04](04-protocol-everest-max.md):
- device info (`AA 55 80`), FW info `11 00`, layout `11 12`;
- extended info `11 14` (DisplayPad layout [U]);
- AP enable `11 80`;
- save/reset flash, reset keys, key functions, sync across profiles, profile switch `14 00`;
- remap `14 20`, shortcut remap `14 21`, launch content `17`, macros `14 18`/`15`, key-content read `11 02`.

### 3.2 DisplayPad-only commands (command channel) [P]
| Request | Meaning | Reply |
|---|---|---|
| `12 03 00 00 <level>` | set main brightness | echo |
| `12 00 00 01` | get main brightness | level in the reply [I offset b4] |
| `22 <enable> 00 00 <hh> <mm> <ss>` | LCD sleep timer on/off with timeout | echo |
| `22 00 00 01` | get LCD sleep timer | enable, hh, mm, ss [I offsets] |
| `23 <enable>` | show the built-in logo | echo |
| `11 03` | free icon slots in flash | count [I offset b4] |
| `11 04` | check icon storage | restore count [I offset b4] |
| `11 05 00 00 <a> <b>` | stored length of a button image | [U] |
| `13 61` | reset all pictures | echo |
| `21 01 00 00 <profile<<4 \| key> … b14=FF` | reset one key's stored picture | completion `21 01 02 00` within 10 s |

## 4. Images [P]

### 4.1 Pixel format
- Each image is a rectangle of 24-bit pixels, row-major, **top row first**, left to right,
  with **3 bytes per pixel in B, G, R order**.
- A key icon is 102 × 102 = **31,212 bytes**.
- The full panel is 800 × 240. Its rect is `(0, 0)`–`(799, 239)`.

### 4.2 Key icon upload
Header (command channel), 64 bytes, zero-filled:

| Byte | Field |
|---|---|
| 0 | `21` |
| 1 | `00` = live (show now, not stored); `01` = store into a profile |
| 2–3 | `00 00` |
| 4 | live: icon index. Stored: `(profile << 4) \| (icon & 0x0F)` |
| 5 | number of 512-byte blocks = ⌈len / 512⌉ (61 for a full icon) |
| 6, 7, 8, 9 | left, top, right, bottom (inclusive; 0, 0, 101, 101 for a full icon) |
| 10 | stored only: frame count (1 for a still image) |
| 11 | stored only: frame index |
| 12–13 | stored only: frame interval u16 (ms [I]) |

Sequence:
1. Send the header and wait for its echo. Bytes 2–3 of the echo must be `00 00`; otherwise
   the device is not ready, so fail.
2. Split the pixel data into ⌈len / 1024⌉ blocks. Write every block except the last as a
   1024-byte output report on the **bulk** channel, with no per-block acknowledgement.
3. Zero-pad the last block to 1024 bytes and write it.
4. Wait for the completion reply on the command channel:

   | Mode | Completion bytes 2–3 | Timeout |
   |---|---|---|
   | Live | `FF FF` | 600 ms |
   | Stored | `02 00` | 10 s if the frame index is 1, otherwise 2 s |

   `FF AA` means rejected.
5. Hold the per-device lock for the whole sequence.

**Live vs stored [P]:**
- Live mode (b1 = `00`, profile 0) is what the host uses for folders/layers and for
  per-press feedback: icons are swapped on the fly.
- Stored mode writes the icon into the profile's flash, so the device shows it without
  the host.

**Animated icons [I]:** an animation is uploaded as several stored frames. Each frame has
the same icon, a frame count, its frame index and an interval. Base Camp also has a
pre-packed frame upload (`SetIconPacket`, header b4 = icon, b5 = `3D`, b8–b9 = `65 65`);
its exact layout is **[U]**.

**Solid-colour icon [I]:** Base Camp can fill an icon with one colour (`SetIconColor`).
This is equivalent to uploading a 102×102 buffer filled with that B, G, R value.

### 4.3 Panel image (whole surface)
Header: `21 00 00 01 <blocks512 u16@4> <left u16@6> <top u16@8> <right u16@10> <bottom u16@12>`.
Data is the rect's pixels (B, G, R), sent as 1024-byte bulk blocks with the last one
zero-padded. Completion follows the live rule (600 ms) [P].

Saving the panel image to flash (`SavePanelImage`) uses a different header (`… 65 04 …`,
rect 0, 0, 799, 239). It is **[U]**; implement the live panel image only.

## 5. Host behaviour [P]

1. **On attach:** upload a default black panel image (live), then apply the selected
   profile: switch the profile and draw its root layer's icons.
2. **Layers (folders):**
   - Bindings form a tree. A key bound to `Create Folder` opens a child layer, and the child
     layer has a `Back` key.
   - Pressing a folder key uploads the child layer's icons live and makes it current.
   - `Back` uploads the parent layer's icons. Wait about 500 ms before handling the next
     action.
3. **Press feedback:** when a key with an image is pressed, the host may upload its
   "pressed" image live, if it has one (two images per key: `image` and `secondImage`).
   This is skipped for keys showing live data (clock, PC info, Twitch/YouTube viewers) and
   for GIF images.
4. **Actions:** every binding type in [01](01-functional-spec.md) §5 is **executed by the
   host**. That includes keyboard shortcuts, which are sent through uinput, and
   multi-actions. Bindings are looked up by matrix number in the current layer.
5. **Live tiles:** keys bound to clock, PC info, or Twitch/YouTube viewer counts are
   periodically redrawn by the host with live icon uploads [I].
6. **Session lock:** when the user's session locks, Base Camp can blank the panel; it
   restores the profile on unlock [I].

## 6. Data model additions

A DisplayPad binding (one per layer × key) adds these fields to [03](03-data-model.md)
KeyBinding:
- `parentId`: the layer tree;
- `isActive`;
- `image`, `secondImage`, each with an optional caption (`optionalText`, `secondOptionalText`);
- `isFirstImageSelected`;
- deleted flags for each image;
- `isHardwarePress`.

## 7. Acceptance vectors

| # | Operation | Expected |
|---|---|---|
| DP1 | Live icon, key 0, full size | header `21 00 00 00 00 3d 00 00 65 65 …00`; then 30 bulk blocks of 1024 bytes and a 31st block holding 492 bytes + zero padding |
| DP2 | Stored icon, profile 2, key 5, single frame | header `21 01 00 00 25 3d 00 00 65 65 01 00 00 00 …00` |
| DP3 | Pixel (x=1, y=0) is pure red | bytes 3–5 of the data are `00 00 ff` |
| DP4 | Panel rect 0,0–799,239 (576,000 bytes) | header `21 00 00 01 65 04 00 00 00 00 1f 03 ef 00 …00` (1125 blocks of 512); then 562 full bulk blocks and a 563rd holding 512 bytes + zero padding |

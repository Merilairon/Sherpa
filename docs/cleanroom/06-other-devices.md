# 06 · Other devices (survey)

What is known so far. Nothing here is specified at byte level unless stated. Each section
lists what an implementer can rely on and what still needs analysis before implementation.

## Everest 60 (`3282:0005`, `3282:0006`)
- **Transport [P]:** HID interface **2** (`mi_02`). Falls back to interface 0 if it isn't
  there. 64-byte reports; the same opcode family as the Everest Max (`11`, `13`, `14`, `15`,
  `19` …).
- **Feature set [P]:** like the Everest Max, plus:
  - an **Fn layer**: bindings carry `layerType` (3 = Fn layer), and macros/launch content
    for Fn-layer keys use separate commands;
  - profile management on the device (insert, delete, exchange);
  - a "show profiles" setting;
  - `USBKeyboardType`, `SetFWLayout`.
- **Lighting effect ids [P]:** Static 1, Colorwave 2, Tornado 3, Breathing 4, Reactive 5,
  Matrix 6, Custom 7, Yeti 8, Off 9. These differ from the Everest Max ids.
- **Needs before implementing:** the per-command payload layouts. The library queues
  commands to a worker, so they weren't traced. Also the Fn-layer packet layouts.

## MacroPad (`3282:0008`)
- **Transport [P]:** vendor collection `FF00:0001`, the same as the Everest Max.
- **Command set [P]:** the Everest Max lighting/key/macro/launch subset, addressed per
  device. Several MacroPads can be attached at once, and each command is aimed at one of
  them. There is no dock, clock or PC info.
- **Key events [P]:** like the Everest Max, plus a device id.
- **Needs before implementing:** confirm that the payloads match the Everest Max; LED count
  and key table.

## DisplayPad (`3282:0009`)
- **Transport [P]:** vendor collection `FF00:0001`.
- **Feature set [P]:**
  - LCD keys with per-key icons (two images per key, optional text; key count [U]);
  - layers and sub-layers (folders);
  - a full-panel image;
  - TFT sleep time;
  - main brightness;
  - macros and launch content like the keyboards;
  - icon colour.
- **Needs before implementing:** the icon upload protocol (`SetIconPic`, `SetIconPacket`,
  `SetPanelImage`), the bitmap format and storage limits. All are **[U]**.

## Makalu (`3282:0002`)
- **Transport [P]:** HID **feature reports**: a 9-byte command channel on interface 0, and a
  bulk channel on top-level collection 3 (2053-byte feature report, or 33 bytes on some
  firmware).
- **Feature set [P]:**
  - DPI levels (list with selected level);
  - polling rate, sensitivity, click speed, button response time;
  - angle snapping, lift-off distance;
  - sleep and deep-sleep timeouts;
  - lighting (effect, colour type, single/dual colours, speed, brightness, direction);
  - button bindings;
  - battery percentage (wireless).
- **Needs before implementing:** the command layouts. They are in managed code and easy to
  recover in a follow-up pass.

## Makalu 67 (`3282:0003`, `3282:0004`)
- **Transport [P]:** hidapi, interface 0.
- **Feature set [P]:**
  - profiles (add, delete, copy, set current, read profile data);
  - DPI settings;
  - angle snapping, polling rate, button response time, lift-off distance;
  - surface calibration (lift-off distance);
  - lighting (persisted, or live without saving);
  - button settings, including "run program" and "run browser" callbacks to the host;
  - factory reset;
  - device-change notifications.
- **Needs before implementing:** the payloads.

## Cross-device behaviour [P]
- Profiles, app-linked switching, macros (global library) and host actions work the same
  way for every device type ([01](01-functional-spec.md)).
- "Sync across devices" is a lighting flag. Its behaviour is **[U]**.

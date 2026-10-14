# 06 · Device index

| Device | PID | Protocol family | Specification |
|---|---|---|---|
| Everest Max | `0001` | 64-byte output/input reports on `FF00:01`; echo acknowledgement | [04](04-protocol-everest-max.md) |
| MacroPad | `0008` | same as the Everest Max | [08](08-macropad.md) |
| DisplayPad | `0009` | Everest family on `FF00:01`, plus 1024-byte bulk image reports on `FF01:02` | [09](09-displaypad.md) |
| Everest 60 | `0005`, `0006` | 64-byte feature reports `<cmd> 46 23 EA` + u32 arguments; polled key events | [10](10-everest-60.md) |
| Makalu 67 | `0003`, `0004` | feature reports `A1` (commands) and `A0` (1041-byte bulk); 9-byte event reports | [11](11-makalu-67.md) |
| Makalu | `0002` | feature reports, see below | survey only |

Cross-device behaviour is common to all of them: profiles, app-linked switching, the global
macro library and host actions ([01](01-functional-spec.md), [05](05-host-actions.md)).
"Sync across devices" is a lighting flag whose behaviour is **[U]**.

## Makalu (`3282:0002`), survey only

- **Transport [P]:** HID feature reports. A 9-byte command channel on interface 0, and a
  bulk channel on top-level collection 3 (a 2053-byte feature report, or 33 bytes on some
  firmware).
- **Feature set [P]:**
  - DPI levels (a list with one selected);
  - polling rate, sensitivity, click speed, button response time;
  - angle snapping, lift-off distance;
  - sleep and deep-sleep timeouts;
  - lighting: effect, colour type, single or dual colours, speed, brightness, direction;
  - button bindings;
  - battery percentage (wireless).
- **Needs before implementing:** the command layouts. They are in managed code and can be
  recovered in a follow-up pass, as was done for the Makalu 67.

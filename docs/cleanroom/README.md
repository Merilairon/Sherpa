# Base Camp cleanroom specification

This folder specifies what Mountain Base Camp (Windows, v1.9.10) does and how it talks
to Mountain peripherals. It is detailed enough for an implementer who has never seen
Base Camp's binaries or decompiled code to build a compatible replacement.

## Cleanroom rules for the implementing agent

1. **These documents are your only source about Base Camp internals.** Do not download,
   open, decompile or search Base Camp binaries, installers, decompiled sources or the
   analysis scratch files. Public material (USB HID specs, Linux hidraw docs, OBS/Spotify/
   Twitch/YouTube public APIs) is fine.
2. Everything here is written as facts, behaviour, data layouts and constants, not code.
   Write your own code from it. Constants, byte layouts, enum values and the key table are
   interoperability data; reuse them as-is.
3. If something you need is marked **[U]** (unknown) or the spec is silent, do not guess
   from Base Camp. Implement the safe behaviour stated, leave a `TODO(spec)` and note it in
   your PR. The person who wrote this spec can investigate and extend it.

## Confidence tags

| Tag | Meaning |
|---|---|
| **[P]** | Proven: read directly from Base Camp's shipped code or data. |
| **[I]** | Inferred: consistent with the code, but not observed on a device. |
| **[U]** | Unknown: not recoverable from the application. Needs a device or a USB capture. |

None of this has been observed on the wire yet. "Proven" means proven *about what Base Camp
sends and expects*, not that the device behaves as described.

## Document map

| File | Contents |
|---|---|
| [01-functional-spec.md](01-functional-spec.md) | Features and user-visible behaviour, per device and across devices |
| [02-architecture.md](02-architecture.md) | Base Camp's original architecture (for reference) and the recommended Linux architecture |
| [03-data-model.md](03-data-model.md) | Entities, fields, enums and persistence rules |
| [04-protocol-everest-max.md](04-protocol-everest-max.md) | Complete HID command reference for the Everest Max keyboard |
| [05-host-actions.md](05-host-actions.md) | Key events from the device and the actions the host runs for them |
| [06-other-devices.md](06-other-devices.md) | Device index and the original Makalu (survey only) |
| [07-acceptance.md](07-acceptance.md) | Golden packets and behavioural acceptance tests (Everest Max and cross-device) |
| [08-macropad.md](08-macropad.md) | MacroPad: differences from the Everest Max protocol, keys, vectors |
| [09-displaypad.md](09-displaypad.md) | DisplayPad: dual-channel transport, icon and panel images, layers, vectors |
| [10-everest-60.md](10-everest-60.md) | Everest 60: feature-report protocol, polling, Fn layer, transfers, vectors |
| [11-makalu-67.md](11-makalu-67.md) | Makalu 67: feature-report protocol, sensor, lighting, buttons, bulk channel, vectors |
| [`../../data/`](../../data/) | Key and layout tables (data; reuse as-is), described in [the data README](../../data/README.md) |

## Glossary

| Term | Meaning |
|---|---|
| **Firmware key code** (`fwKey`) | The keyboard's own id for a physical key or function. Used as the macro source key, remap source/target and launch-content key. Column `DLLKeyId` in the key table. |
| **LED index** | Position of a key's LED in the 126-entry per-key colour array. Column `DLLMatrixIndex`. |
| **VK** | Windows virtual-key code. Base Camp's macro recorder stores keys as VK; the key table maps VK to `fwKey`. |
| **Firmware profile / slot** | One of 5 on-board profiles (1..5). Each stores lighting, remaps, macros and launch content. |
| **Effect index** | Firmware lighting effect id (`EffectIndex` enum). |
| **Menu index** | Firmware slot an effect's settings are saved into (`MenuIndex` enum). |
| **Launch content** | A payload stored on the keyboard against a key. The host reads it back when the key is released and performs the action (run program, open URL, OBS command …). |
| **Media dock** (MMDock) | The detachable dial/display module of the Everest Max. |
| **Numpad** | The detachable numpad module of the Everest Max. |
| **Payload** | The 64 bytes of a HID report after the report-ID byte. |

## Scope priorities for a first implementation

1. Everest Max: transport, device info, lighting (presets, per-key), profiles, remaps,
   macros, save/reset.
2. Host actions daemon (launch content + key events).
3. Media dock: clock, volume, PC info feed, extended settings.
4. Everything else in [01-functional-spec.md](01-functional-spec.md).
5. MacroPad ([08](08-macropad.md)), then Everest 60 ([10](10-everest-60.md)), Makalu 67 ([11](11-makalu-67.md)) and DisplayPad ([09](09-displaypad.md)).

Out of scope until specified: firmware update, Everest Max dock/numpad picture upload, raw flash access, telemetry.

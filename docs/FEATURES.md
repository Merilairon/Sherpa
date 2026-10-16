# Feature overview

One-page status of what Sherpa does today and what is still open. Details live in
[IMPLEMENTATION.md](IMPLEMENTATION.md) (how it works, `TODO(spec)` guesses) and
[ROADMAP.md](ROADMAP.md) (prioritised work list).

**Caveat:** nothing has been verified on real hardware yet; "implemented" means built to the
spec and passing the tests against simulated devices. See [SMOKETEST.md](SMOKETEST.md).

Legend: ✅ implemented · ⚠️ implemented with a guess or limitation · ❌ not implemented

## 1. Platform and core

| Feature | Status | Notes |
|---|---|---|
| Rust daemon `sherpad` (per-user service) + Electron app | ✅ | Linux (hidraw, systemd user unit, udev) and Windows (HID API, named pipe) |
| Local-only IPC, same-user check | ✅ | Unix socket with `SO_PEERCRED`; Windows pipe with client SID check |
| Atomic JSON config store, recovery | ✅ | `store.json`, temp file + rename |
| Offline edits re-applied on attach | ✅ | dirty flag + `syncOnAttach` |
| Device wins on the active profile | ✅ | |
| Disabled-profile detection (no writes/switches) | ✅ | |
| Lighting and dock settings imported from the keyboard on attach | ✅ | |
| Tray, autostart, daemon respawn | ✅ | |
| Simulated devices (`--fake`) and dry-run mode | ✅ | |
| Packages: .deb, AppImage, tar.gz, NSIS installer, zip | ✅ | Flatpak/Snap, Arch/Fedora packages ❌ |
| UI localisation (Base Camp ships 17 locales) | ⚠️ | English, Nederlands, Deutsch, Français, Español; language picker in Settings. Translations are unreviewed by native speakers; tray menu and daemon-supplied option names (media, OS, OBS, Twitch/Spotify actions, key names) stay English; 12 locales missing. Add a language: `app/src/renderer/locales/` |
| Settings audit against spec 01 §2 | ❌ | default custom delay, saved colours, start minimised |

## 2. Devices

| Device | Status | Implemented | Open |
|---|---|---|---|
| **Everest Max** (+ dock, numpad) `3282:0001` | ✅ | Profiles, all lighting effects, per-key Custom, all binding types, macros, launch content, host actions, media dock (clock, pages, screen saver, brightness, PC info/volume/APM), game/YETI/indicator modes, resets, per-profile game mode | Game-mode key set and YETI semantics; background colour of Reactive/Matrix/YETI ⚠️; screen-saver type bits ⚠️; whether bindings need Save `13 55` ⚠️; keys with usage > 503 ⚠️; "sync across devices" flag |
| **MacroPad** `3282:0008` | ✅ | Same as Everest Max minus the dock | — |
| **DisplayPad** `3282:0009` | ⚠️ | Profiles, folders/layers with Back keys, per-key icons with captions and "pressed" image, host-run bindings, brightness, sleep timer | Live tiles (clock, PC info, viewers) ❌; session-lock blanking ❌; panel save to flash ❌; stored/animated icons exist in `sherpa-core` but are unused; hold/repeat macros played once |
| **Everest 60** `3282:0005/0006` | ⚠️ | Profiles, lighting, base + Fn layer bindings, macros, launch content, polled key events, numpad detection, modes | Per-key Custom transfer, speed/direction semantics, light bar, Fn-layer shortcut command, key-content read layout, real command delays, Fn profile-cycle trigger |
| **Makalu 67** `3282:0003/0004` | ⚠️ | Profiles, DPI levels, polling rate, response time, angle snapping, lift-off, surface calibration, lighting with live preview, all six buttons, run program/URL buttons | Scroll left/right binding; macro mouse classes `10`/`11`; keyboard modifier byte; event b4/b5 order; OS-command macros |
| **Makalu (original)** `3282:0002` | ❌ | — | Only surveyed in the spec; needs a capture pass |

## 3. Bindings and host actions

| Feature | Status | Notes |
|---|---|---|
| Keyboard, media, OS-command, launch (program/URL/folder) bindings | ✅ | never through a shell |
| Launch approval for device-originated launches | ✅ | stricter than the spec: kind and arguments must match |
| Macros: create, edit, record, import/export | ✅ | recording needs `input` group on Linux; own JSON formats |
| Multi-actions | ⚠️ | second press ignored; hold/repeat steps played once |
| OBS (obs-websocket) | ⚠️ | "Set Current Source" toggles visibility in the program scene |
| OS command Sleep vs Hibernate | ⚠️ | one firmware target for both |
| Spotify, Twitch, YouTube | ❌ | bindings can be stored, report "not configured"; need OAuth apps and keyring storage |
| Razer Chroma sync | ❌ | wanted; Windows-only, mirrors Chroma broadcast colours via the device's "sync effect" streaming mode (spec 01 §2, §8); protocol not specified |
| Adobe / DaVinci Resolve / Zoom presets | ❌ | no preset data in the spec; use keyboard-shortcut bindings |
| Profile export/import | ✅ | `sherpa-profile` format |

## 4. Media dock

| Feature | Status | Notes |
|---|---|---|
| Clock, pages, screen saver, brightness | ✅ | |
| PC info feed: CPU, RAM, volume, APM | ✅ | |
| GPU load | ⚠️ | AMD/Intel via sysfs only; NVIDIA (NVML) and Windows (PDH) report 0 |
| Network value unit | ⚠️ | Mbit/s clamped to 0..100 |
| Dock/numpad picture upload | ❌ | wanted; protocol not specified, needs a capture pass |

## 5. Out of scope

Firmware updates and bootloader, raw flash access, telemetry, update checks.

## 6. Next steps (summary)

1. Hardware validation and fixing guessed behaviour.
2. Cheap spec-complete items: NVIDIA/Windows GPU load, settings audit, hold/repeat host macros.
3. DisplayPad live tiles and session-lock blanking.
4. Capture pass for Everest 60, Makalu 67 and original Makalu gaps, then implement.
5. Integrations (Spotify first) and app presets.

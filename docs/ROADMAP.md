# Roadmap

Features that are specified but not yet implemented, derived from the README, the
`TODO(spec)` table in [IMPLEMENTATION.md](IMPLEMENTATION.md) and the clean-room
specification ([cleanroom/](cleanroom/README.md)). Ordered by priority.

Legend: **Spec** = the spec is enough to implement it; **Blocked** = needs information the
spec lacks (capture/reverse-engineering pass, or a third-party app registration).

## 0. Hardware validation (highest priority)

Nothing has been verified on real hardware; everything is tested against simulated devices
only.

- [ ] Smoke-test every device (Everest Max, MacroPad, DisplayPad, Everest 60, Makalu 67) on
      real hardware on Linux and Windows. *Spec*
- [ ] Re-verify each `TODO(spec)` guess below against a real device and fix the code or
      tests accordingly. *Spec*
- [x] Step-by-step smoke-test checklist: [SMOKETEST.md](SMOKETEST.md).

## 1. Features the spec fully describes

| Item | Spec | Notes |
|---|---|---|
| DisplayPad live tiles (clock, PC info, Twitch/YouTube viewer counts) | 09 §5.5 | periodic live icon uploads from the host |
| DisplayPad session-lock blanking and restore on unlock | 09 §5.6 | needs session-lock events on Linux (logind) and Windows |
| Host playback of *Hold* and *Repeat* macros (DisplayPad, multi-actions) | 01 §6 | currently played once; needs key-state tracking |
| GPU load for the dock: NVIDIA (NVML) and Windows (PDH counters) | 01 F-DCK-4 | currently AMD/Intel sysfs only, otherwise 0 |
| Linux installer polish: Flatpak/Snap, Arch/Fedora packages | README | .deb, AppImage and tar.gz exist |
| Settings completeness check against 01 §2 (language / locales, default custom delay, saved picker colours, start minimized) | 01 §2 | verify each is exposed in the UI; add UI localisation (Base Camp ships 17 locales) |
| Keyring storage for integration tokens | 01 F-INT-1 | prerequisite for section 3 |

## 2. Blocked on missing protocol information

These need a further capture/analysis pass (as was done for the Makalu 67) before they
can be built.

- [ ] **Makalu (original, `3282:0002`)**: only surveyed (06). Needs command layouts. Scope:
      DPI levels, polling rate, sensitivity, click speed, debounce, angle snapping,
      lift-off, sleep timeouts, lighting, button bindings, battery.
- [ ] **Mouse binding Scroll left/right** (01 §5): wheel macro key codes unknown.
- [ ] **Everest 60 gaps** (10): per-key Custom lighting transfer (cmds 32/34/35/36), speed and
      direction semantics, light bar, Fn-layer shortcut command, key-content read layout
      (2D/2F/2E), real command delays, Fn "profile-cycle trigger" semantics.
- [ ] **Makalu 67 gaps** (11): macro mouse classes `10`/`11`, keyboard modifier byte, event
      byte b4/b5 order, OS-command macros.
- [ ] **DisplayPad panel save to flash** (09 §4.3) and pre-packed animation upload
      (`SetIconPacket`); animations/stored icons exist in `sherpa-core` but are unused.
- [ ] **Everest Max unknowns**: Reactive/Matrix/YETI background vs colour 3 (04 §5.1),
      screen-saver type bits in `screenSetup` (04 §6.4), whether key bindings need
      Save `13 55`, `11 02` argument (LED index vs fwKey), keys with usage > 503.
- [ ] **Dock network value unit** (F-DCK-4): currently Mbit/s clamped to 0..100.
- [ ] **Game mode key set and YETI mode semantics** (01 §8).
- [ ] **"Sync across devices"** lighting flag behaviour (06).

## 3. Integrations (optional, F-INT-2)

Each needs its own registered OAuth application and tokens in the platform keyring.
Bindings can already be stored but report "not configured".

- [ ] Spotify (Web API): play/pause, next/prev, volume, shuffle, repeat, like, playlist, play URI
- [ ] Twitch (Helix + chat): chat message, ad, slow/followers-only chat, title/game
- [ ] YouTube (Data API): chat message and related actions
- [ ] OBS "Set Current Source": exact semantics (currently toggles visibility in the program scene)

## 4. App-specific presets

- [ ] Adobe, DaVinci Resolve and Zoom preset bindings. The spec has no preset data; build a
      preset table of public shortcuts, or keep using the keyboard-shortcut binding.

## 5. Explicitly out of scope (per spec)

Firmware updates and the bootloader (`060B:7801`), dock/numpad picture upload, raw flash
access, telemetry, update checks, Razer Chroma sync.

## Suggested order

1. Hardware validation and fixing guessed behaviour (section 0).
2. Cheap spec-complete items: GPU load, settings audit, Hold/Repeat host macros.
3. DisplayPad live tiles and session-lock blanking.
4. Capture pass for Everest 60 / Makalu 67 / Makalu original gaps, then implement.
5. Integrations (Spotify first) and presets.

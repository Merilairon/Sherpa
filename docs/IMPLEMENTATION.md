# Implementation notes

How the clean-room specification ([cleanroom/](cleanroom/README.md)) is implemented, and
where the implementation had to choose because the spec is silent or marked **[U]**.

## Architecture

The implementation follows the recommended architecture of
[02-architecture.md](cleanroom/02-architecture.md) §2, on both Linux and Windows.

```
┌───────────────────────┐  JSON lines over a Unix socket (Linux)  ┌──────────────────────────┐
│ Electron app (app/)   │ ───── or a named pipe (Windows) ──────▶ │ basecampd (user process) │
│  main: tray, autostart│ ◀──────────── signals ───────────────── │  device manager          │
│  renderer: plain DOM  │                                          │  config store (JSON)     │
└───────────────────────┘                                          │  host actions            │
                                                                   │  dock feeder, app watcher│
                                                                   └────────────┬─────────────┘
                                                      Linux /dev/hidraw* · Windows HID API
```

| Spec requirement | Where |
|---|---|
| Vendor collection `FF00:01`, report-ID rule | `basecamp-hid` (descriptor parser), `devices/mod.rs::pick` |
| One command in flight, chunks atomic, ack per packet (C-1, C-2) | `link.rs` (`Session` holds the per-device lock) |
| Timer pushes skipped when busy (C-3) | `Link::try_session`, `EverestDev::push` |
| Host actions off the device thread (C-4) | `host.rs::spawn_action` |
| Timeout → retry once → fail; NAK → fail (§2.3) | `Session::transact` |
| Store left unchanged when a device write fails (B2) | `daemon.rs::edit_profile` |
| Detached edits marked dirty and re-applied on attach (D-1) | `edit_profile`, `initial_sync` |
| Device wins on the active profile (D-3) | `initial_sync`, `device_profile_changed` |
| Re-apply on attach only if `syncOnAttach` (D-2) | `initial_sync` |
| Atomic store writes | `store.rs::save` (temp file + rename, mode 0600) |
| IPC never on TCP; same-user only (B11) | `ipc/unix.rs` (mode 0600 + `SO_PEERCRED`), `ipc/windows.rs` (local-only pipe + client SID check) |
| Launch content from the device: kinds 1/4 only if bound on this host or approved (05 §3) | `host.rs::launch_allowed`; the UI asks the user (`LaunchApprovalRequired`) |
| Never through a shell | `actions/system.rs` (`Command` without a shell, `ShellExecuteW`/`xdg-open` for URLs and folders) |

On Linux the daemon is meant to run as a systemd **user** service (or is started by the
app); udev `uaccess` gives it the hidraw nodes. On Windows it runs in the user session;
Windows HID needs no driver for vendor collections.

## IPC API

Request `{"id": n, "method": "...", "params": {...}}`, response `{"id": n, "result": ...}`
or `{"id": n, "error": {"message": "..."}}`, and signals `{"signal": "...", "data": {...}}`.
`basecampd --call <Method> '<json>'` is a minimal client.

| Area | Methods |
|---|---|
| General | `GetStatus`, `GetCatalog`, `GetSettings`, `SetSettings`, `Shutdown` |
| Devices | `ListDevices`, `GetDevice`, `GetLayout`, `SetMode`, `GetModes`, `Reset`, `SyncClock` |
| Profiles | `GetProfiles`, `SelectProfile`, `UpdateProfile`, `ResetProfile`, `CopyProfile`, `ExportProfile`, `ImportProfile` |
| Lighting | `SetLighting`, `PreviewLighting`, `SetCustom` (alias `SetPerKey`) |
| Keys | `SetBinding`, `ClearBinding` |
| Macros | `GetMacros`, `PutMacro`, `DeleteMacro`, `ExportMacros`, `ImportMacros`, `StartRecording`, `StopRecording` |
| Media dock | `SetDock`, `GetDockState` |
| Makalu 67 | `SetMouseSensor`, `SetMouseLighting`, `SetMouseButton`, `Calibrate` |
| DisplayPad | `PutImage`, `GetImage`, `AddLayer`, `DeleteLayer`, `OpenLayer`, `SetDisplayBrightness`, `SetDisplaySleep` |
| Security | `ApproveLaunch`, `RevokeLaunch`, `ListApprovals` |
| Integrations | `ObsQuery` |
| Simulation only | `SimulateKey` (with `--fake`) |

Signals: `DeviceAttached`, `DeviceDetached`, `DeviceChanged`, `ProfileChanged`,
`ProfileChangedOnDevice`, `LightingChangedOnDevice`, `SensorChangedOnDevice`,
`ModesChangedOnDevice`, `DeviceReset`, `StoreChanged`, `MacrosChanged`, `SettingsChanged`,
`LayerChanged`, `LaunchApprovalRequired`, `ActionFailed`, `CalibrationResult`.

## Tests

`cargo test --workspace` runs:

| Spec | Tests |
|---|---|
| 07 §1 golden packets G1–G22 | `basecamp-core/src/everest.rs` (`golden_*`) |
| 07 §2 reply parsing R1–R11 | `everest.rs`, `multipart.rs`, `bitmap.rs`, `launch.rs`, `actions/mod.rs`, `tests/behaviour.rs` (`r3_*`, `r4_*`, `r9_*`–`r11_*`) |
| 07 §3 behavioural B1–B11 | `basecampd/tests/behaviour.rs` (`b1_*`…`b11_*`), against simulated devices (`fake.rs`); B6 starts a real process, B11 impersonates another UID when run as root |
| 08 §5 MacroPad MP1–MP4 | `everest.rs::macropad_vectors`, `behaviour.rs::macropad_vectors_through_daemon` |
| 09 §7 DisplayPad DP1–DP4 | `displaypad.rs`, `behaviour.rs::displaypad_init_layers_and_actions` |
| 10 §7 Everest 60 E1–E5 | `everest60.rs`, `behaviour.rs::everest60_profile_poll_and_keys` |
| 11 §8 Makalu 67 K1–K6 | `makalu67.rs`, `behaviour.rs::makalu_button_and_events` |

The same suites pass when built for `x86_64-pc-windows-gnu` and run under Wine.

## Where the spec is silent (`TODO(spec)`)

Each item below is marked `TODO(spec)` in the code. The safe behaviour stated by the spec,
or the least invasive choice, is implemented.

| Topic | What the code does |
|---|---|
| Mouse binding *Scroll left/right* (01 §5) | The wheel macro key codes are not specified; the binding is rejected with a clear message. |
| Do key bindings need `13 55` (Save) to persist? | Not sent (spares the flash). |
| Background colour of Reactive/Matrix/YETI (04 §5.1) | Written to bytes 18–20 ("background"); the colour-3 slot stays 0. |
| `screenSetup` screen-saver type bits (04 §6.4) | Only the two enable bits are written; the type is kept in the store. |
| Network value unit of the dock (F-DCK-4) | Mbit/s, clamped to 0..100. |
| GPU load | Linux AMD/Intel via sysfs `gpu_busy_percent`; other GPUs and Windows report 0. |
| Profile keys with HID usages > 503 (04 §10) | Cannot arrive in the 64-byte bitmap; profile changes are also detected through `11 00` on every key release (05 §2.2 step 2). |
| `11 02` key argument: LED index or fwKey? (04 §7.6) | LED index, as Base Camp does. |
| Everest 60 command delays (10 §3) | 20 ms, 40 ms for transfer ends. |
| Everest 60 speed/direction semantics, light bar, per-key custom transfer (10 §4.4) | Everest Max field layout reused; Custom sends only the effect id and brightness. |
| Everest 60 Fn-layer shortcut command | `23` is used for both layers. |
| Everest 60 key-content read layout (10 §4.6) | Host actions use the store's binding for the key instead of reading it back. |
| Everest 60 Fn (154) "profile-cycle trigger" | Fn is tracked for the Fn layer; on release the active profile is re-read. |
| Makalu 67 keyboard modifier byte, macro mouse classes `10`/`11` | HID modifier bitmap; `10` buttons (mask), `11` wheel. |
| Makalu 67 event bytes b4/b5 order | Passed through unchanged to the bulk read; the program check accepts any program button of the device. |
| Makalu 67 OS-command macros | Not offered (event lists unknown). |
| DisplayPad stored icons, animations, panel save (09 §4) | Icons are uploaded live by the host (layers need that anyway); stored/animated uploads are implemented in `basecamp-core` but not used; panel save is not implemented. |
| DisplayPad live tiles (clock, PC info, viewers) and session-lock blanking | Not implemented. |
| Host playback of hold/repeat macros (DisplayPad, multi-actions) | Played once. |
| OBS "Set Current Source" | Toggles the source's visibility in the current program scene. |
| Twitch, YouTube, Spotify (F-INT-1/2) | Need their own OAuth apps; the bindings can be stored but report "not configured". |
| Adobe / DaVinci / Zoom presets | No preset data in the spec; use a keyboard shortcut binding. |
| Profile export/import, macro export/import format | Own JSON formats (`basecamp-linux-profile`, `basecamp-linux-macros`); macro import also accepts Windows VK `rawcode`s. |
| Second press of a running multi-action | Ignored (as recommended). |
| Makalu (`3282:0002`) | Not implemented (spec is a survey only). |

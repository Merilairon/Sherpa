# Base Camp Linux

A clean-room, cross-platform replacement for Mountain Base Camp: configure Mountain
keyboards, keypads and mice on **Linux and Windows**.

- **`basecampd`** (Rust): a per-user background service that owns the devices, the
  configuration store and the host actions (run program, open website, OBS, macros…).
- **Base Camp Linux** (Electron): the desktop app. It holds no device state; it talks to
  `basecampd` over a local socket (Linux) or named pipe (Windows).

It is implemented only from the specification in [docs/cleanroom/](docs/cleanroom/README.md)
and the tables in [data/](data/README.md), plus public platform documentation (Linux
hidraw/uinput, Win32 HID/SendInput, obs-websocket).

> **Hardware status:** nothing in the specification has been verified on real hardware
> yet, so this has only been exercised against simulated devices. Expect rough edges on
> first contact with a device. Please report what you see.

## Supported devices

| Device | USB id | Implemented |
|---|---|---|
| Everest Max (+ media dock, numpad) | `3282:0001` | Profiles, all lighting effects, per-key Custom painting, every key-binding type, macros, launch content and host actions, media dock (clock, pages, screen saver, brightness, PC info/volume/APM feed), game/YETI/indicator modes, resets |
| MacroPad | `3282:0008` | Same as the Everest Max, minus the dock |
| DisplayPad | `3282:0009` | Profiles, folders/layers with Back keys, per-key icons with captions and a "pressed" image, all bindings run on the host, brightness, sleep timer |
| Everest 60 | `3282:0005`, `0006` | Profiles, lighting, base and Fn layer bindings, macros, launch content, polled key events, numpad detection, modes |
| Makalu 67 | `3282:0003`, `0004` | Profiles, DPI levels, polling rate, response time, angle snapping, lift-off, surface calibration, lighting (with live preview), all six buttons, run program/URL buttons |
| Makalu (original) | `3282:0002` | Not yet: only surveyed by the specification |

Out of scope (as in the spec): firmware updates, dock/numpad picture upload, raw flash
access, telemetry, Razer Chroma sync, update checks.

## Install

Release packages are produced by CI (see `.github/workflows/ci.yml`) or locally with
`scripts/build.sh`.

### Linux

- **Debian/Ubuntu:** install `basecamp-linux_<version>_amd64.deb`. It installs the app, puts
  `basecampd` on the `PATH`, installs the udev rules and a systemd user unit.
  Enable the service so host actions work without the window open:
  `systemctl --user enable --now basecampd`
- **AppImage / tar.gz:** run the app directly. For device access install the udev rules
  and (optionally) the service from the bundled `resources/linux/` folder:
  `sh resources/linux/install-local.sh resources/bin/basecampd`

The udev rules:

| File | Purpose |
|---|---|
| `70-basecamp.rules` | gives the logged-in user access to `hidraw` nodes of vendor `3282` (required) |
| `71-basecamp-uinput.rules` | lets the logged-in user create a virtual keyboard/mouse (`/dev/uinput`), needed for host-executed shortcuts, multi-actions and DisplayPad keys (optional) |

Macro *recording* reads `/dev/input/event*`; add yourself to the `input` group to use it
(you can always enter macro events by hand in the editor).

### Windows

Run `Base Camp Linux Setup <version>.exe` (or unzip the `-win.zip`). No driver or
administrator rights are needed: the device service runs as your user.

The app starts `basecampd` automatically if it is not already running. Closing the window
keeps the app in the tray; the tray menu can also stop the device service.

## Build from source

Requirements: Rust (stable), Node.js 20+.

```sh
cargo test --workspace                      # protocol vectors + behavioural tests
cargo build --release -p basecampd

cd app && npm ci && npm start               # run the UI against your devices
npm run start:fake                          # …or against simulated devices
```

Packages:

```sh
scripts/build.sh linux    # AppImage, deb, tar.gz        → dist/app/
scripts/build.sh win      # NSIS installer and zip (cross-built; needs mingw-w64 and wine)
scripts/build.ps1         # on Windows: installer and zip
```

## Using the daemon directly

```sh
basecampd                     # run the service (normally started by systemd or the app)
basecampd --fake              # simulated Everest Max, MacroPad, DisplayPad, Everest 60, Makalu 67
basecampd --dry-run           # log host actions instead of running them
basecampd --call ListDevices
basecampd --call SelectProfile '{"dev":"everest-max","slot":2}'
basecampd --print-socket      # IPC endpoint
```

Configuration lives in `$XDG_CONFIG_HOME/basecamp-linux/store.json` (Linux) or
`%APPDATA%\basecamp-linux\store.json` (Windows), written atomically.

## Documentation

- [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md): architecture, IPC API, how the spec maps
  to the code, test coverage, and every place where the spec is silent (`TODO(spec)`).
- [docs/cleanroom/](docs/cleanroom/README.md): the specification this is built from.

## Repository layout

| Path | Contents |
|---|---|
| `crates/basecamp-core` | Pure protocol code: packet builders/parsers for every device, key tables, data model |
| `crates/basecamp-hid` | HID transport: native Linux hidraw (no libudev/libusb), Windows HID via `hidapi` |
| `crates/basecampd` | The service: device manager, store, host actions, dock feeder, IPC, simulated devices |
| `app/` | Electron desktop app |
| `packaging/linux/` | udev rules, systemd user unit, package scripts |
| `docs/cleanroom/`, `data/` | Specification and interoperability tables |

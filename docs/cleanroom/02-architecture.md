# 02 · Architecture

## 1. Base Camp's original architecture (reference only) [P]

Knowing this explains *why* the behaviour in the other documents looks the way it does.
Do not copy it.

| Process | Role |
|---|---|
| Desktop shell | Electron window hosting a local web UI. Also hosts the global input hook used to record macros. |
| UI server | Local web application on `http://localhost:13488`. Owns the SQLite database and all user-facing logic. Builds device structs and sends them to the service. |
| Device service | Windows service running as LocalSystem, auto-start. Loads the per-device SDK libraries, owns the USB handles, runs timers (clock, PC info), reacts to key events and runs host actions. |
| Worker | Helper that starts programs, browsers and folders in the logged-in user's session (the service runs outside it). |
| DisplayPad worker | Separate process for the DisplayPad and for multi-action / hotkey-switch execution. |

**IPC.**
- *UI → service:* a TCP connection on a random loopback port. Each request is a JSON object
  `{"Class", "MethodName", "Parameters": [...]}` terminated by `<EOF>`. The response is a
  plain string.
- *Service → UI:* HTTP GET callbacks to the UI server (device connected, profile changed on
  device, upload progress).
- *SDK library → service:* window messages.

**Weaknesses to avoid.**
- The service accepts unauthenticated RPC from any local process and runs it as
  LocalSystem, including raw flash writes and firmware update.
- UI and service share one database file with no locking discipline.

## 2. Recommended Linux architecture

```
┌──────────────────────┐   D-Bus (session bus) or Unix socket   ┌─────────────────────────┐
│ UI (GTK / web / CLI) │ ─────────────────────────────────────▶ │ basecampd  (user daemon) │
└──────────────────────┘ ◀──────── signals / events ─────────── │  • device manager        │
                                                                 │  • profile/config store  │
                                                                 │  • host-action runner    │
                                                                 │  • dock feeder (timers)  │
                                                                 └───────────┬─────────────┘
                                                                             │ /dev/hidraw*
                                                                    udev: TAG+="uaccess"
```

| Component | Requirements |
|---|---|
| **udev rule** | Give the logged-in user access to hidraw nodes with `idVendor == 3282`. No root daemon. |
| **basecampd** | One per user session, started by a systemd user unit. The only process that opens the device. |
| ↳ device manager | Enumerate hidraw nodes and pick the vendor collection ([04](04-protocol-everest-max.md) §1). Run the request/response engine, with exactly one command in flight per device. Handle hot-plug through udev monitor or polling, and re-apply state on attach. |
| ↳ config store | The source of truth for profiles, lighting, bindings and macros ([03](03-data-model.md)). Store a JSON or SQLite file in `$XDG_CONFIG_HOME/basecamp-linux/`. Write atomically (write to a temp file, then rename). |
| ↳ host-action runner | Consume key events from the device and run actions ([05](05-host-actions.md)) in the user's session. |
| ↳ dock feeder | Clock sync on attach (+10 s) and every 30 min; volume and PC info every 1 s, but only while the dock shows that page ([01](01-functional-spec.md) §7). |
| **IPC** | D-Bus interface `gg.mountain.BaseCamp1` or a Unix socket in `$XDG_RUNTIME_DIR` with mode 0600. Check `SO_PEERCRED`. Never listen on TCP. |
| **UI** | Any front end. It holds no device state; it calls the daemon and listens to its signals. |

### 2.1 Daemon API (suggested)

| Method | Effect |
|---|---|
| `ListDevices() → [{id, pid, name, fw, layout}]` | |
| `GetProfiles(dev)`, `SelectProfile(dev, slot)` | `SelectProfile` sends the profile-switch command and updates the store |
| `SetLighting(dev, slot, menuIndex, settings)` | send, then save to flash |
| `SetPerKey(dev, slot, leds[126], perLedEffect[126], brightness)` | |
| `SetBinding(dev, slot, fwKey, binding)` | §5 of [01](01-functional-spec.md) decides which device command is used |
| `PutMacro(macro)`, `DeleteMacro(name)` | rewrites every bound key (F-MAC-7) |
| `SetDock(dev, dockSettings)`, `SetMode(dev, mode, on)` | |
| Signal `DeviceAttached/Detached`, `ProfileChangedOnDevice`, `LightingChangedOnDevice` | |

### 2.2 Concurrency rules

**C-1** Serialize all device I/O for a device through one queue. Never interleave two
commands' packets. Multi-packet commands (chunks) are atomic [P].
**C-2** Wait for each packet's acknowledgement before sending the next one
([04](04-protocol-everest-max.md) §2) [P].
**C-3** Timers (dock feeder) push into the same queue. Drop a timer tick if the previous
one is still queued.
**C-4** Host actions run off the device thread, so a slow action never blocks device I/O.

### 2.3 Failure handling

| Situation | Behaviour |
|---|---|
| No reply within the timeout | Retry the packet once, then fail the whole operation. Don't update the store. |
| NAK (`FF AA`) | Fail the operation. Report the command and its arguments. |
| Device detached mid-operation | Abort the operation. On re-attach, re-apply the selected profile from the store. |
| Store and device disagree on the active profile | The device wins: update the store and signal the UI [P] (Base Camp does this when a key event arrives). |

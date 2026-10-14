# Hardware smoke test

A step-by-step checklist for trying Sherpa on a real device. Nothing in the spec has been
verified on hardware yet, so please run through it and report what you see.

Do **one device at a time**. Tick each box; on a failure, note the step number and collect
the log (see "Reporting" at the end).

> Risk note: this is untested software writing to keyboard/mouse flash. Don't test on a
> device you can't afford to reset. Factory resets (step 4.9) are always available from the
> device itself and via `Reset`.

## 0. Before you start

- [ ] Note OS + version, Sherpa version, device name, PID (`lsusb -d 3282:`, or Device
      Manager on Windows) and firmware version if the app shows it.
- [ ] Close the original Mountain Base Camp (it competes for the same HID interfaces).
- [ ] Optional: export your current device settings with Base Camp first, as a backup.

## 1. Install and sanity check (no device needed)

- [ ] Install the package (see README), or build: `cargo build --release -p sherpad` and
      `cd app && npm ci`.
- [ ] `cargo test --workspace` passes (source builds only).
- [ ] **Simulated run:** `sherpad --fake` (or `npm run start:fake` in `app/`) shows all five
      simulated devices in the app. This confirms the app and service work before touching
      hardware.
- [ ] `sherpad --call GetStatus` returns JSON (service is reachable).

**Linux only**
- [ ] udev rules installed: `ls /etc/udev/rules.d/ /usr/lib/udev/rules.d/ | grep sherpa`
      shows `70-sherpa.rules` (and `71-sherpa-uinput.rules`).
- [ ] Reload and re-plug: `sudo udevadm control --reload && sudo udevadm trigger`.
- [ ] Your user can open the device: `ls -l /dev/hidraw*` shows an ACL/owner for you
      (`getfacl /dev/hidrawN`).
- [ ] Optional features: `/dev/uinput` writable (host shortcuts, multi-actions, DisplayPad);
      member of the `input` group (macro recording).
- [ ] Service running: `systemctl --user enable --now sherpad`, then
      `systemctl --user status sherpad` is active.

**Windows only**
- [ ] The app starts without asking for administrator rights; `sherpad.exe` appears in
      Task Manager; the tray icon is present.

## 2. Detection

Run `sherpad -v` in a terminal (stop the service first) to see debug logs.

- [ ] Plug the device in: it appears in the app within a few seconds, with the right name.
- [ ] `sherpad --call ListDevices` lists it with the expected USB id.
- [ ] No errors in the log about permissions (`Permission denied` → udev rules problem).
- [ ] Unplug: it disappears without restarting the app. Re-plug: it comes back.
- [ ] Start the app **with the device already connected**: it is found.
- [ ] Two devices of the same type (if you have them, MacroPad/DisplayPad): both appear.

## 3. Profiles (all devices)

- [ ] The profile list shows the expected slots (Everest Max/MacroPad: 5).
- [ ] Select another profile in the app: the device switches (profile LED/indicator or
      lighting changes).
- [ ] Switch the profile **on the device** (profile key): the app selection follows.
- [ ] Rename a profile; restart the app; the name persists.
- [ ] Duplicate a profile, export it, delete/reset it, import it back.
- [ ] Unplug, edit a profile in the app, re-plug: the edit is applied (detached edits
      re-sync on attach).
- [ ] App-linked profile: link a profile to a program (e.g. `xterm`/`notepad.exe`), start
      it → the device switches; close it with "revert on close" set → it switches back.

## 4. Per device

### 4.1 Everest Max (`3282:0001`) and MacroPad (`3282:0008`)

Lighting
- [ ] Each lighting effect applies on the keyboard: Static, Color Wave, Tornado, Breathing,
      Reactive, Matrix, YETI, Off.
- [ ] Colour 1/2, colour type (single/dual/random), speed, brightness and direction take
      effect and look right.
- [ ] Reactive/Matrix background colour: note what you see (spec is unsure).
- [ ] Custom mode: paint a few keys with static colours and an animated effect; they
      survive unplug/re-plug and reboot of the PC.
- [ ] Device-side effect/brightness keys change lighting and the app follows.

Key bindings (test each in a text editor / `xev` / `wev` / `evtest`)
- [ ] Disable a key, then restore it to Default.
- [ ] Remap a key to another key; to a shortcut with modifiers (e.g. Ctrl+Shift+A); to
      F13–F24.
- [ ] Media keys (play/pause, volume, mute).
- [ ] Mouse button binding (left/right/middle/back/forward, scroll up/down).
- [ ] Profile key bindings (profile 1–5, next, previous, cycle).
- [ ] OS commands: Calculator, Lock computer, Run explorer, Task manager. (Skip
      Shutdown/Sleep/Hibernate unless you are ready for the PC to power off.)
- [ ] Run program (`xterm`/`notepad.exe`): the program starts. The app asks for approval the
      first time the key content comes from the device.
- [ ] Open website (http/https): the browser opens. Open folder: the file manager opens.
- [ ] Hotkey switch alternates between two shortcuts on each press.
- [ ] Multi-action with delays runs in order; pressing it again while running is ignored.
- [ ] OBS (if installed): set port/password in settings, then try Start/Stop recording, a
      scene switch.

Macros
- [ ] Create a macro by hand (events + delays), bind it, run it on the keyboard.
- [ ] Record a macro (Linux: needs `input` group). Check the timings.
- [ ] Try playback modes: Play once, Hold, Repeat.
- [ ] Edit the macro: every key bound to it updates.
- [ ] Export and import the macro library.

Media dock (Everest Max with dock only)
- [ ] The clock syncs ~10 s after attach; analog/digital and 12/24 h settings apply.
- [ ] Enable/disable menu pages; the dock shows only the enabled ones.
- [ ] Volume page tracks the system volume.
- [ ] PC info pages (CPU, GPU, disk, network, RAM, APM) update about once a second.
      GPU shows 0 on NVIDIA / Windows (known gap).
- [ ] Screen saver and auto turn-off timeouts work; brightness for dock/numpad applies.

Modes and resets
- [ ] Game mode on/off (note which keys get disabled).
- [ ] YETI mode, indicator LEDs on/off.
- [ ] Reset keys, reset effects (only when you are happy to lose them).

### 4.2 DisplayPad (`3282:0009`)

- [ ] On attach the panel goes black, then shows the selected profile's root layer icons.
- [ ] Upload an icon (with caption) to a key; it appears at the right position and size.
- [ ] Set a "pressed" image; it shows on press.
- [ ] Create a folder: pressing it shows the child layer with a Back key; Back returns.
      Allow ~0.5 s between presses.
- [ ] Each binding type runs on the host (shortcut via uinput, run program, website,
      multi-action).
- [ ] Brightness and sleep timer apply.
- [ ] Profile switch redraws all icons.

### 4.3 Everest 60 (`3282:0005` / `0006`)

- [ ] Lighting effects apply; note speed/direction behaviour (spec is inferred).
- [ ] Base layer and Fn layer bindings work; compare Fn-layer shortcuts.
- [ ] Key events are detected (polled): host actions fire within a perceptible delay.
- [ ] Macro runs; numpad detection if a numpad is attached.
- [ ] Game mode and other modes.
- [ ] Per-key Custom lighting: only effect + brightness is sent (known gap).

### 4.4 Makalu 67 (`3282:0003` / `0004`)

- [ ] DPI levels: add/change levels, select one; the pointer speed changes.
- [ ] Polling rate, response time, angle snapping, lift-off distance apply.
- [ ] Surface calibration: run it, check the `CalibrationResult` signal / UI message.
- [ ] Lighting effects with live preview; settings persist.
- [ ] All six buttons: remap each; test left/right/middle/back/forward/DPI.
- [ ] Run program / URL buttons start on the host.
- [ ] Scroll left/right binding is rejected with a clear message (known gap).

## 5. Robustness

- [ ] Unplug mid-operation (while uploading an icon or applying a profile): the app does not
      crash and recovers on re-plug.
- [ ] Suspend/resume the PC: the device is re-detected and re-synced.
- [ ] Kill the service (`systemctl --user stop sherpad` / end `sherpad.exe`) and restart:
      state is restored from `store.json`; the app reconnects.
- [ ] Close the window: the app stays in the tray (setting-dependent). Quit from the tray.
- [ ] Restart the PC: with start-on-boot enabled the service and (optionally) the app start.
- [ ] Check `store.json` exists (`~/.config/sherpa/` or `%APPDATA%\sherpa\`) and is valid
      JSON.

## 6. Reporting

For every failed step, open an issue with:

- the step number, expected vs actual behaviour;
- OS, device, PID, firmware version;
- the log: `sherpad -vv 2>&1 | tee sherpa.log` (add `--dry-run` if host actions are involved),
  or `journalctl --user -u sherpad` on Linux;
- if possible a USB capture or a screenshot/photo of the device.

Add `[x]` results to a copy of this file and attach it. Please also say which steps you
**skipped**.

# 01 · Functional specification

What a user can do and what the application must do in response. Wire formats are in
[04](04-protocol-everest-max.md); data shapes in [03](03-data-model.md).

## 1. Devices

All devices use USB vendor ID `0x3282` [P].

| Device | PID(s) | Class | Spec depth |
|---|---|---|---|
| Everest Max (keyboard + media dock + numpad) | `0x0001` | keyboard | full ([04](04-protocol-everest-max.md)) |
| Makalu (mouse) | `0x0002` | mouse | survey ([06](06-other-devices.md)) |
| Makalu 67 (mouse) | `0x0003`, `0x0004` | mouse | survey |
| Everest 60 | `0x0005`, `0x0006` | keyboard | survey |
| MacroPad | `0x0008` | keypad (several may be attached) | survey |
| DisplayPad | `0x0009` | LCD keypad | survey |
| Everest Max bootloader | `060B:7801` | firmware update only | out of scope |

**F-DEV-1** The app lists only connected devices by default (setting
`DisplayOnlyConnectedDevices = 1`) [P].
**F-DEV-2** It must notice plug and unplug at runtime and update the UI without a restart [P].
**F-DEV-3** On every attach of an Everest Max, after about 10 s, the app pushes the current
date and time to the keyboard ("clock sync", §6) [P].

## 2. Application-wide settings

| Setting | Behaviour |
|---|---|
| Start on system boot | Launch at login. |
| Start minimized | Start without showing the main window. |
| Minimize to tray | Closing or minimizing keeps the app running in the tray. |
| Language | UI language. Base Camp ships 17 locales. |
| Check for updates / beta channel | Base Camp polls `basecamp.mountain.gg`. **Do not implement** (no public API). |
| Send user data | Telemetry opt-in. **Do not implement**. |
| Razer sync (Chroma) | Mirrors Razer Chroma broadcast colours onto the keyboard. Windows-only; out of scope. |
| OBS port / password | obs-websocket connection settings (§9). |
| Default custom delay | Default per-step delay (ms) for macros in "Custom" delay mode. |
| Saved picker colours | User palette shared by colour pickers. |

## 3. Profiles

**F-PRO-1** Each device type has its own list of profiles. The Everest Max has exactly
**5 firmware profile slots** (1..5) [P]. Profiles are stored both in the app's database
and on the keyboard.
**F-PRO-2** Profile fields: name, image (built-in or user upload), order, slot number,
"selected" flag, optional linked executable, and a "revert when the app closes" flag.
**F-PRO-3** Selecting a profile in the UI switches the keyboard to that slot, telling it
which lighting menu index is active in that profile [P].
**F-PRO-4** Switching profiles on the keyboard itself (profile keys or a dock gesture) must
update the UI selection [P]. See [05](05-host-actions.md) §3.
**F-PRO-5 App-linked profiles:** when a process whose executable file name matches a
profile's linked executable starts, switch the device to that profile. When it exits, and
the profile has "revert on close" set, switch back to the profile that was selected before
[P]. Matching is on file name only, not full path [P].
**F-PRO-6** Duplicate, reset to default, export and import a profile. Base Camp's export file
format is not specified; define your own.
**F-PRO-7** "Sync across profiles": a lighting effect or key binding flagged this way is
applied to every profile of that device [P]. The keyboard also has a hardware flag for it
(`12 08`, [04](04-protocol-everest-max.md)).

## 4. Lighting (Everest Max)

**F-LGT-1** Each profile has one settings record for each of 9 menu entries, shown in this
order: Static, Color Wave, Tornado, Breathing, Reactive, Matrix, Custom, YETI mode, Off.
Exactly one is active [P].
**F-LGT-2** Per-entry options [P]:

| Option | Applies to | Values |
|---|---|---|
| Colour type | all except Off/Custom | single colour, dual colour, random |
| Colour 1 / Colour 2 | per colour type | RGB |
| Background colour | Reactive, Matrix, YETI | RGB (shown as colour 3) |
| Speed | all animated | 0..100 (the device uses 5 steps, see [04](04-protocol-everest-max.md) §5.2) |
| Brightness | all | 0..100 |
| Direction | Tornado, Color Wave | effect-specific |

**F-LGT-3** Applying an entry sends it to the keyboard and then saves it to the keyboard's
flash under that entry's menu index [P].
**F-LGT-4 Custom mode:** the user paints keys. Each key gets either a static colour or one of
the animated effects. A painting always applies to the selected set of keys. Base Camp
seeds a palette of 10 colours [P]:
`#5BDE62 #98eb34 #98eb35 #B3357F #eb4034 #DC29BC #00ccb5 #0076cc #4700cc #cc0043`.
The order of operations is in [04](04-protocol-everest-max.md) §5.5.
**F-LGT-5** On the keyboard, a "Change Effect" key and brightness +/- keys change lighting
without the app. The app must reflect the change (§[05](05-host-actions.md) §3) [P].
**F-LGT-6** Main brightness on/off, the indicator LEDs on/off (`12 02/03`, `12 07`) and
"sync effect" (live streaming mode used for screen/Chroma sync) are device toggles [P].
**F-LGT-7 Extended brightness:** for each of the 5 profiles, separate brightness levels for
the media dock and the numpad [P] (ExtendInfo bytes 19–28, [04](04-protocol-everest-max.md) §6.4).

## 5. Key bindings (Everest Max)

**F-KEY-1** Each profile stores one binding per key. Unbound keys behave as "Default".
**F-KEY-2** Binding types, what is written to the keyboard, and what happens at press time
[P]:

| Binding | Sub-options | Stored on keyboard as | Executed by |
|---|---|---|---|
| Default | – | remap to `255` (restore) | keyboard |
| Disable | – | remap to `0` | keyboard |
| Keyboard shortcut | any key + optional Ctrl/Shift/Alt/Win | remap, or shortcut remap with modifier mask | keyboard |
| Keyboard function | F13–F24 | remap to target code 230..241 | keyboard |
| Media | Play/Pause, Stop, Previous, Next, Volume up/down, Mute | remap to target code (table in [04](04-protocol-everest-max.md) §7.2) | keyboard |
| Mouse | Left, Right, Middle, Forward, Back, Scroll up/down | remap to target code 257..305 | keyboard |
| Mouse | Scroll left / right | a stored macro (one wheel event) | keyboard |
| Profile | Profile 1–5, Cycle, Previous, Next | remap to target code 160..164, 210..212 | keyboard |
| OS command | Shutdown, Sleep, Hibernate, Calculator | remap to target code 128/129/195 | keyboard |
| OS command | Lock computer | shortcut remap: fwKey 39 (L) + mask 8 (Win) | keyboard |
| OS command | Run explorer | shortcut remap: fwKey 19 (E) + mask 8 (Win) | keyboard |
| OS command | Task manager | stored macro, play once: LCtrl↓ LShift↓ Esc↓ Esc↑ LShift↑ LCtrl↑ (fwKeys 58, 44, 110) | keyboard |
| Run macro | a macro from the library | macro data on the source key | keyboard |
| Adobe / DaVinci / Zoom presets | app-specific shortcut | shortcut remap or stored macro | keyboard |
| Run program | file path | launch content kind 1 | **host** |
| Run browser | URL (http/https only) | launch content kind 2 | **host** |
| OBS | action + argument | launch content kind 3 | **host** |
| Open folder | folder path | launch content kind 4 | **host** |
| Twitch | chat message, play ad, slow chat, followers-only chat, stream title/game, … | launch content kind 5 | **host** |
| YouTube | chat message, … | launch content kind 6 | **host** |
| Spotify | play/pause, next/prev, volume, shuffle, repeat, like, playlist, play URI, … | launch content kind 7 | **host** |
| Hotkey switch | two shortcuts, alternated on each press | launch content kind 8 | **host** |
| Multi-action | ordered list of actions with delays | launch content kind 9 | **host** |

**F-KEY-3** Host-executed bindings work only while the host daemon runs. Their *content* is
stored on the keyboard, so a binding follows the keyboard to another PC that runs the
software [P].
**F-KEY-4** Keys on the numpad display ("touch keys") can carry an image and a caption.
Image upload is out of scope [U].
**F-KEY-5** A key can carry a user-visible alternate name (UI only).

## 6. Macros

**F-MAC-1** Macros form a **global library** shared by all profiles and devices. A key
binding refers to a macro by name [P].
**F-MAC-2** Macro fields: name (unique), record keyboard (bool), record mouse (bool), delay
mode, custom delay (ms), playback mode, event list, order in the list.
**F-MAC-3 Recording** captures global input events while recording is on. Base Camp
records with system-wide hooks; on Linux use evdev or libinput [P/I]. It captures:
- key down/up, as a key id;
- mouse button down/up (1 = left, 2 = right, 3 = middle);
- wheel (up/down).

Each event stores `delay` = milliseconds until the **next** event. The last event's delay is
forced to 0 [P].
**F-MAC-4 Delay modes** [P]:

| Mode | Behaviour |
|---|---|
| Record delay | keep the recorded timings |
| Custom | every event's delay = the macro's custom delay (default from settings); each delay is then individually editable |
| No delay | every delay = 0 |

**F-MAC-5 Playback modes** [P, labels and help text from the UI]:

| Mode | Device type code | Behaviour |
|---|---|---|
| Play once | 1 | run the macro once per press |
| Hold | 4 | repeat while the key is held |
| Repeat | 3 | start on press, repeat until the key is pressed again |

**F-MAC-6** The event editor supports insert, delete, reorder and editing each delay.
**F-MAC-7** Saving a macro rewrites it onto **every** key, in every profile and device,
whose binding is "Run macro" with that name [P].
**F-MAC-8** Macros run on the keyboard (stored in flash). The host is not involved during
playback [P]. Limit: 1000 events including the terminator [P].
**F-MAC-9** Export and import macros (your own file format). Base Camp's format is not specified.

## 7. Media dock (display dial)

**F-DCK-1 Menu pages** can each be enabled or disabled: Clock, Profile, Lighting mode,
Volume, Brightness, PC info, APM counter, Custom mode [P].
**F-DCK-2 Clock:** analog or digital, 12 h or 24 h [P]. The host sets the time on attach
(+10 s), and every 30 minutes after that [P].
**F-DCK-3 Volume page:** while the dock shows the volume page, the host pushes the system
master volume (0..100) every second [P].
**F-DCK-4 PC info:** while the dock shows a PC-info page, the host pushes that metric every
second [P]:

| Page (dock menu index) | Metric | Range |
|---|---|---|
| 97 | CPU load % | 0..100 |
| 98 | GPU load % | 0..100 |
| 99 | Disk usage % | 0..100 |
| 100 | Network speed | 0..100 (unit [U]) |
| 101 | RAM usage % | 0..100 |
| 113 | APM: key releases in the last 60 s | 0..999 |

**F-DCK-5 Screen saver:** enable flag, timeout, and a type: Image, Clock, Stopwatch, Timer,
Volume, Brightness, PC info (CPU, GPU, HDD, Internet, RAM) or APM. Image upload is out of
scope [P/U].
**F-DCK-6 Auto turn-off:** enable flag and timeout [P].
**F-DCK-7 Menu colour:** RGB accent colour of the dock UI [P].
**F-DCK-8** Choose which profiles the dock shows (5 entries) [P].
**F-DCK-9** Detect which side the dock and the numpad are attached to (left/right) [P].
Base Camp uses this only for display.

## 8. Device modes

| Mode | Behaviour | Command |
|---|---|---|
| Game mode | firmware disables selected keys (e.g. Win) while on | `12 06`, `12 09` [P]; which keys [U] |
| YETI mode | an extra lighting/status mode | `12 0A` [P]; semantics [U] |
| Reset keys / effects / all | factory resets of firmware sections | `13 60` / `13 61` / `13 40` [P] |

## 9. Integrations (host side)

| Integration | Mechanism | Used by |
|---|---|---|
| OBS Studio | obs-websocket on `ws://127.0.0.1:<port>` with password | OBS bindings: switch profile/scene/source/transition, transition duration, mic/desktop volume, start/stop streaming/recording/replay buffer, studio mode, media controls |
| Spotify | Spotify Web API (OAuth) | Spotify bindings |
| Twitch | Twitch Helix API + chat (OAuth) | Twitch bindings |
| YouTube | YouTube Data API (OAuth) | YouTube bindings |

**F-INT-1** Each integration needs its own OAuth app and credentials, so don't reuse Base
Camp's. Store tokens in the platform keyring.
**F-INT-2** Treat each integration as optional and pluggable. A first version may ship with
none of them.

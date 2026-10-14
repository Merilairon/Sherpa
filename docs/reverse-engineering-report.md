# Mountain Base Camp 1.9.10: how it talks to the hardware

**Input:** `Base-Camp-Release-Notes_1.9.10-public.zip` → `BaseCamp_V1.9.10.msi` (264 MB).

**Method:** `msiextract`, a custom extractor for the .NET single-file bundle
`BaseCamp.UI.exe`, ILSpy 8.2 (managed code), Ghidra 11.4.2 headless and radare2
(native SDK DLLs), and `msiinfo` (MSI tables). No hardware was attached. Nothing
here has been observed on the wire; every claim comes from the shipped code.

Byte-level details are in [protocol-everest-max.md](protocol-everest-max.md).

## 1. Architecture

```
 Base Camp.exe (Electron.NET shell, x64)
   └─ resources/bin/BaseCamp.UI.exe   ASP.NET Core MVC, single-file bundle, x64
        │  pages, SQLite BaseCamp.db, macro recorder (SharpHook / uiohook.dll)
        │
        │  TCP 127.0.0.1:<ephemeral>   {"Class","MethodName","Parameters"} + "<EOF>"
        ▼                               ◄── HTTP GET localhost:13488/home/EverestConnectedChanged
 BaseCamp.Service.exe   Windows service "BaseCampService", .NET Fx 4.5.2, x86,
   │                    auto-start, LocalSystem
   │  P/Invoke (cdecl)                                ▲ window msgs 0x5401 / 0x5402 / 0x6400
   ├─ SDKDLL.dll           Everest Max  3282:0001  ───┘ (plug/unplug, key events)
   ├─ Everest360_USB.dll   Everest 60   3282:0005/0006 (hidapi, interface 2)
   ├─ MacroPadSDK.dll      MacroPad     3282:0008
   ├─ makalu_67_dll.dll    Makalu 67    3282:0003/0004
   └─ Makalu.dll (managed) Makalu       3282:0002
 MountainDisplayPadWorker.exe → DisplayPadSDK.dll   DisplayPad 3282:0009 (own socket server)
 Basecamp.Worker.exe   starts programs, browser and cmd in the user's session (the service runs in session 0)
```

**What's proven, and where it comes from:**

* **Native code split:** the UI process is x64 and the SDK DLLs are x86, so the UI cannot load them. Every device call goes UI → service RPC → DLL.
* **Finding the service:** the UI locates the service by running `netstat` against the service PID (`ProcessPortHelper.GetNetStatPort`), with port 11000 as a fallback (`BaseCamp.UI.Common/TCPClient.cs`).
* **RPC dispatch:** the service turns `Class` into a type name and calls `MethodName` by reflection, passing the JSON parameters as strings (`BaseCampService.ReadCallback`). Struct parameters are serialized with Newtonsoft and deserialized straight into the P/Invoke structs.
* **Hotplug path:** the SDK DLL registers for device notifications and posts `0x5401` to a hidden window (`OpenUSBDriver` writes `+0x4c4 = 0x5401`). The service's `MessageHandler.WndProc` then GETs `CallBackURL` from `BaseCamp.Service.exe.config`, which is how the UI learns about plug and unplug.
* **Service account:** `ServiceInstall` row: `BaseCampService`, type 16, start 2 (automatic), `StartName` empty, which means LocalSystem.

**Security note (proven from code, not exploited):** the RPC socket has no authentication. Any local process can connect and call any public method on those classes, inside a LocalSystem process. That includes raw flash access (`EraseSectorMem`, `WriteSectorMem`) and firmware update. The Linux design below avoids this.

## 2. Device I/O (Everest Max, traced end to end)

* **Discovery:** `SetupDiGetClassDevs(HidGuid)`, `HidD_GetAttributes`, then a VID/PID match (`3282:0001`). The SDK then picks the top-level collection with usage page `0xFF00` and usage `1` (`FUN_10008880`).
* **Write:** `WriteFile` of `ReportID ‖ 64 bytes` on that collection's handle (`FUN_10030570`).
* **Request/response:** one command at a time, under a critical section. After each write the SDK waits up to 600 ms for an input report whose first four bytes echo the request. `FF AA` means rejected (`FUN_1000be60`, reader `FUN_100077b0`).
* **Unsolicited reports:**
  * `01 …` is a key-state bitmap. The SDK diffs it and calls the managed `KEY_CALLBACK`, which is how "Run Program", OBS, media and similar bindings run on the PC.
  * `02`–`0A` stream the live LED frame.

The other keyboard-class SDKs (Everest 60, MacroPad) use the same opcode family: constants `0x11`, `0x13`, `0x14`, `0x15` and `0x19` appear in their packet builders. Everest 60 goes through hidapi (`hid_open(0x3282, 5|6, …, "mi_02")`) and queues commands to a worker thread.

## 3. Macros: from recording to flash

1. **Record (UI):** SharpHook (libuiohook) global hooks capture `keydown`/`keyup` (Windows VK in `rawcode`), `mousedown`/`mouseup` (`button`) and `wheel` (`rotation`). Each event has a `delay`. The list is saved as JSON in `Macros.InputsJson`, alongside `PlaybackOption`, `DelayOption` and `RecordMouse`/`RecordKeyboard` (`MacroController.SaveMacro`).
2. **Bind:** assigning "Run Macro" to a key stores a `KeyboardBinding` row (`FunctionType="Run Macro"`, `FunctionValue=<macro name>`).
3. **Convert (UI):** `EverestDataHelper.GetMacroInputContent`:
   * `iSrc` = the bound key's `DLLKeyId`.
   * `iType`: 1 for "play-one", 4 for "play-two", 3 otherwise. `iTimes` = 1.
   * Each event maps to a 4-byte `KEYMAP_EVENT`:
     * **Keys:** VK → `DLLKeyId`, with press = action 2 and release = action 1 on page 0.
     * **Mouse:** buttons 1/2/3 on page 1.
     * **Wheel:** 49 up / 48 down, action 3, page 1.
   * A terminator `FF FF 00 00` is appended.
4. **RPC:** `{'Class':'Everest','MethodName':'EverestSetFullMacroData','Parameters':[<json>]}`.
5. **Service:** deserializes `MacroInputContent` and calls `SetFullMacroData(iSrc, iType, iTimes, n, events[])`.
6. **SDK** (`FUN_1000b9d0`): caps the macro at 1000 events, sends the header `14 18 <type> 00 <times>`, then `15 <src> <count> <cont>` chunks of 15 events each. Every packet is acknowledged before the next one goes out.

Re-saving a macro rewrites it onto every key bound to it. For Everest 60 Fn-layer keys, `SetFnFullMacroData` is used instead.

## 4. RGB

* **Model:** each profile has nine `Lighting` rows (Static, Color Wave, Tornado, Breathing, Reactive, Matrix, Custom, YETI, Off). They carry colours as hex strings, `Speed` 0..100, `Brightness` and `Type` (single, dual or random colour). `EffIndex` is the firmware effect id and `EffMenuIndex` is the slot the effect is saved into.
* **Preset effects:** `getChangeEffect` builds `EffData`, which goes through `EverestChangeEffect` to `ChangeEffect`. The SDK then adjusts the struct before sending `14 2C + EffData`:
  * it drops effects it doesn't handle;
  * it remaps speed per effect;
  * it forces direction and width to `FF`;
  * it fixes `byRandColor`.

  Color Wave and Tornado go through `ChangeBlockEffect` instead.
* **Custom / per-key static:** the UI sends `SwitchToCustomizeEffect(brightness)` (`14 2C 0A 00 FF b`), then `ChangeCustomizeStatic` (126 RGB values in 7 × `14 2C 00 01 <i> 4B 00 …`), then `SetCustomizeTable` (126 per-LED effect ids in 3 × `14 A0 <i> 01 …`), then `SaveFlash(6)`. The LED order is `DLLMatrixIndex` from the key table.
* **Persistence:** `SaveFlash(menu)` (`13 55 00 00 m`), which Base Camp sends after lighting changes. My reading, not yet observed: changes before `SaveFlash` are live only, and the firmware stores lighting per profile, with `SwitchProfile(profile, menu)` choosing which profile is active.

## 5. Proven, inferred, hidden

### Proven (read directly from shipped code)

* Process layout, the IPC format, the service account and the hotplug notification path (§1).
* VID `3282` and the PIDs per device and SDK. For Everest Max: vendor collection `FF00:0001`, 64-byte payloads with the report ID taken from the HID caps, echo-based completion, `FF AA` reject, and the 600 / 1300 ms timeouts.
* Exact byte layouts of `APEnable`, `EnableKeyFunc`, `SaveFlash`, `ChangeEffect`, `SwitchToCustomizeEffect`, `ChangeCustomizeStatic`, `SetCustomizeTable` and `SetFullMacroData`.
* The speed remap table and the SDK's rewriting of `EffData` fields.
* The macro event encoding and how the UI fills it, the 1000-event cap and the terminator.
* The key table: 126 keys, each with firmware code, LED index and VK.
* The effect and menu index enums.
* Recording uses global input hooks. Host-side bindings run off the `01` key-state report.

### Inferred (consistent with the code, not observed)

* **`iType` meanings:** now confirmed from the macro editor's labels: 1 Play once, 4 Hold, 3 Repeat. How the firmware times repeats is still unobserved.
* **Macro storage:** macros and lighting go to the firmware's active on-board profile, because `SetFullMacroData` has no profile argument.
* **Per-LED effect id:** `SetCustomizeTable` value 0 means "static" for that LED, because the UI writes `Lighting.EffIndex` there.
* **Multi-collection interfaces:** on Linux the vendor collection may share a hidraw node with other collections. Reading the report ID from the descriptor reproduces what `HidP_GetCaps` gives the SDK.
* **Speed remap input:** for every effect branch, the remap reads `bySpeed`. The disassembly confirms this for Static and Yeti; for the others it follows from the decompiled C.
* **Other devices:** Everest 60 and MacroPad payloads probably match Everest Max for shared commands (same opcodes appear), but I didn't verify them layout by layout.

### Hidden (needs the device, firmware, or more work)

* **Report descriptor:** the real descriptor, and therefore the actual report ID and interface number. Read `/sys/class/hidraw/hidrawN/device/report_descriptor` on a real keyboard.
* **Firmware internals:** flash layout, how many macros fit, per-profile limits, the meaning of the `byData` / `undef` padding, and what `APEnable` actually unlocks. The firmware is not in the MSI; it is downloaded by the updater.
* **`ChangeKey`:** the payload after `14 20`. Its arguments are passed in registers and Ghidra lost them. Recoverable with more disassembly work or a USB capture.
* **Read-side commands:** `0x11 …` gets and the `0x19` macro readback were only partially decoded.
* **Not traced in this pass:** `ChangeBlockEffect` field rewriting, the display/numpad image upload, the firmware update (IAP `060B:7801`), the Makalu mouse protocol (managed code, easy to follow next) and the DisplayPad image protocol.
* **Timing:** whether the firmware tolerates faster command pacing than Base Camp uses.

**To close the gaps:** capture a Base Camp session with USBPcap/Wireshark on Windows (or `usbmon` with Base Camp in a VM using USB passthrough) and diff it against `basecamp-linux --dry-run` output for the same action.

## 6. Proposed small working version (implemented in this repo)

**Goal:** per-key and preset lighting plus on-board macros for the Everest Max on Linux, with no kernel driver and no Windows components.

| Piece | What | Why |
|---|---|---|
| Kernel | none, `hidraw` (in-tree) | the protocol is plain HID writes/reads |
| `packaging/70-mountain-basecamp.rules` | `TAG+="uaccess"` for VID 3282 hidraw | the logged-in user can open the device without root |
| `basecamp_linux` (Python ≥3.10, stdlib only) | `protocol.py` packet builders, `hidraw.py` discovery, descriptor parsing and request/echo, `keys.py`, `macros.py` | one module per concern; builders are pure and tested |
| CLI `basecamp-linux` | `list`, `keys`, `effect`, `perkey`, `macro`, `apply`, `watch`, plus `--dry-run` / `-v` | manual use and scripting |
| `packaging/basecamp-linux.service` | systemd **user** unit running `watch profile.json` | re-applies your profile on hotplug |
| `examples/` | profile and macro JSON | macro files use Base Camp's `InputsJson` shape, so exported macros import as-is |

**Deliberately left out:**

* firmware update, raw flash read/write and image upload (risk of bricking the device);
* any network-reachable or unauthenticated IPC.

**Next steps, in order:**

1. **Validate on hardware:** start with `effect static -c ff0000` without `--save`, then `perkey`, then `macro`. Then `--save`.
2. **Host actions daemon:** keep the vendor node open, send `APEnable(1)`, and decode `01` key-state reports. Map keys to actions: `xdg-open`, `playerctl`, obs-websocket.
3. **Desktop integration:** add `ChangeBlockEffect` (Color Wave, Tornado), the `ChangeKey` remap and profile switching. Add a small GTK or local web UI, talking to the daemon over a Unix socket checked with `SO_PEERCRED`.
4. **More devices:** Everest 60, MacroPad and Makalu, once their payloads are confirmed.

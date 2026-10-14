"""basecamp-linux: drive a Mountain Everest Max from Linux."""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from . import hidraw, keys, macros, protocol as p

EVEREST_MAX = 0x0001
# Lighting.MenuIndex in BaseCamp.Data.dll: the slot SaveFlash persists into.
MENU_INDEX = {
    "static": 0, "colorwave": 1, "tornado": 2, "breathing": 3, "reactive": 4,
    "matrix": 5, "custom": 6, "yeti": 7, "off": 8,
}


def hex_color(s: str) -> p.Color:
    s = s.lstrip("#")
    if len(s) != 6:
        raise argparse.ArgumentTypeError(f"bad colour {s!r}, want RRGGBB")
    return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))


def open_device(args) -> hidraw.Device:
    log = (lambda b: print(b.hex(" "), file=sys.stderr)) if args.verbose or args.dry_run else None
    if args.dry_run:
        return hidraw.Device(hidraw.Node("(dry-run)", hidraw.VENDOR_ID, EVEREST_MAX, 0), dry_run=True, log=log)
    nodes = [n for n in hidraw.find(EVEREST_MAX) if n.report_id is not None]
    if not nodes:
        sys.exit("no Everest Max vendor interface found (is the udev rule installed?)")
    return hidraw.Device(nodes[0], log=log)


# --- operations shared by CLI and profile apply -------------------------------


def apply_effect(dev, name: str, colors, brightness: int, speed: int, save: bool) -> None:
    idx = p.EFFECTS[name]
    rand = p.RAND_DUAL if len(colors) > 1 else p.RAND_SINGLE
    if not colors:
        colors, rand = [(255, 255, 255)], p.RAND_RANDOM
    eff = p.Effect(idx, brightness=brightness, speed=speed, colors=list(colors), rand=rand, apply_all=1)
    dev.request(p.change_effect(eff))
    if save:
        dev.request(p.save_flash(MENU_INDEX[name]), timeout=1.3)


def apply_per_key(dev, default: p.Color, per_key: dict[str, p.Color], brightness: int, save: bool) -> None:
    leds = [default] * p.LED_COUNT
    for name, color in per_key.items():
        leds[keys.by_name(name).led] = color
    dev.request(p.switch_to_custom(brightness))
    dev.send_all(p.custom_static(leds))
    dev.send_all(p.customize_table([p.EFFECTS["static"]] * p.LED_COUNT))
    if save:
        dev.request(p.save_flash(MENU_INDEX["custom"]), timeout=1.3)


def apply_macro(dev, key_name: str, path: str) -> None:
    events, playback = macros.load(path)
    dev.send_all(p.macro(keys.by_name(key_name).dll_key, events, playback))


def apply_profile(dev, path: Path) -> None:
    """Profile file: {"lighting": {...}, "macros": {"F1": "file.json"}}; paths relative to it."""
    doc = json.loads(path.read_text())
    light = doc.get("lighting", {})
    save = bool(light.get("save", False))
    brightness = int(light.get("brightness", 100))
    if "per_key" in light:
        apply_per_key(
            dev,
            hex_color(light.get("default", "000000")),
            {k: hex_color(v) for k, v in light["per_key"].items()},
            brightness,
            save,
        )
    elif "effect" in light:
        apply_effect(
            dev, light["effect"], [hex_color(c) for c in light.get("colors", [])],
            brightness, int(light.get("speed", 50)), save,
        )
    for key_name, macro_file in doc.get("macros", {}).items():
        apply_macro(dev, key_name, str(path.parent / macro_file))


# --- commands ---------------------------------------------------------------


def cmd_list(args) -> None:
    for n in hidraw.find():
        rid = "-" if n.report_id is None else n.report_id
        print(f"{n.path}  {n.vid:04x}:{n.pid:04x}  {n.name:<12} vendor-report-id={rid}")


def cmd_keys(args) -> None:
    print(f"{'name':<14} {'fw':>4} {'led':>4} {'vk':>4}")
    for k in keys.table():
        print(f"{k.name:<14} {k.dll_key:>4} {k.led:>4} {k.vk:>4}")


def cmd_effect(args) -> None:
    with open_device(args) as dev:
        apply_effect(dev, args.name, args.color or [], args.brightness, args.speed, args.save)


def cmd_perkey(args) -> None:
    per_key = {}
    for spec in args.set or []:
        name, _, col = spec.rpartition("=")
        per_key[name] = hex_color(col)
    with open_device(args) as dev:
        apply_per_key(dev, args.default, per_key, args.brightness, args.save)


def cmd_macro(args) -> None:
    with open_device(args) as dev:
        apply_macro(dev, args.key, args.file)


def cmd_apply(args) -> None:
    with open_device(args) as dev:
        apply_profile(dev, Path(args.profile))


def cmd_watch(args) -> None:
    """Re-apply a profile whenever the keyboard (re)appears. Meant for a systemd user unit."""
    seen: set[str] = set()
    while True:
        present = {n.path for n in hidraw.find(EVEREST_MAX) if n.report_id is not None}
        for path in sorted(present - seen):
            node = next(n for n in hidraw.find(EVEREST_MAX) if n.path == path)
            time.sleep(1.0)  # let the firmware finish enumerating
            try:
                with hidraw.Device(node) as dev:
                    apply_profile(dev, Path(args.profile))
                print(f"applied {args.profile} to {path}", flush=True)
            except (OSError, hidraw.ProtocolError) as e:
                print(f"{path}: {e}", file=sys.stderr, flush=True)
                present.discard(path)  # retry on next poll
        seen = present
        time.sleep(args.interval)


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(prog="basecamp-linux", description=__doc__)
    ap.add_argument("-n", "--dry-run", action="store_true", help="print packets, touch no device")
    ap.add_argument("-v", "--verbose", action="store_true", help="print packets as they are sent")
    sub = ap.add_subparsers(dest="cmd", required=True)

    sub.add_parser("list", help="list Mountain hidraw nodes").set_defaults(fn=cmd_list)
    sub.add_parser("keys", help="print the key table").set_defaults(fn=cmd_keys)

    e = sub.add_parser("effect", help="set a built-in lighting effect")
    e.add_argument("name", choices=sorted(n for n, i in p.EFFECTS.items() if i in p.SIMPLE_EFFECTS))
    e.add_argument("-c", "--color", type=hex_color, action="append", help="RRGGBB, up to 2; none = random")
    e.add_argument("-b", "--brightness", type=int, default=100)
    e.add_argument("-s", "--speed", type=int, default=50, help="0..100")
    e.add_argument("--save", action="store_true", help="persist to on-board flash")
    e.set_defaults(fn=cmd_effect)

    k = sub.add_parser("perkey", help="static per-key colours")
    k.add_argument("--default", type=hex_color, default=(0, 0, 0))
    k.add_argument("--set", action="append", metavar="KEY=RRGGBB")
    k.add_argument("-b", "--brightness", type=int, default=100)
    k.add_argument("--save", action="store_true")
    k.set_defaults(fn=cmd_perkey)

    m = sub.add_parser("macro", help="write a macro onto a key (stored on the keyboard)")
    m.add_argument("key")
    m.add_argument("file")
    m.set_defaults(fn=cmd_macro)

    a = sub.add_parser("apply", help="apply a profile JSON")
    a.add_argument("profile")
    a.set_defaults(fn=cmd_apply)

    w = sub.add_parser("watch", help="apply a profile on every hotplug (daemon)")
    w.add_argument("profile")
    w.add_argument("--interval", type=float, default=2.0)
    w.set_defaults(fn=cmd_watch)

    args = ap.parse_args(argv)
    try:
        args.fn(args)
    except (hidraw.ProtocolError, KeyError, ValueError) as e:
        sys.exit(f"error: {e}")


if __name__ == "__main__":
    main()

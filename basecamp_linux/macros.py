"""Macro files -> KEYMAP_EVENT lists.

Accepts Base Camp's own Macros.InputsJson shape, so exported macros import as-is:
  {"type": "keydown"|"keyup", "rawcode": <Windows VK>, "delay": ms}
  {"type": "mousedown"|"mouseup", "button": 1|2|3, "delay": ms}
  {"type": "wheel", "rotation": -1|1, "delay": ms}
and, for hand-written files, "key": "<key name>" instead of "rawcode".
Conversion follows EverestDataHelper.GetMacroInputContent.
"""

from __future__ import annotations

import json
from pathlib import Path

from . import keys, protocol as p


def event_from_json(item: dict) -> p.MacroEvent:
    kind = item["type"]
    delay = int(item.get("delay", 0))
    if kind in ("keydown", "keyup"):
        if "key" in item:
            key = keys.by_name(item["key"])
        else:
            key = keys.by_vk(int(item["rawcode"]))
            if key is None:
                raise KeyError(f"no Everest key for rawcode {item['rawcode']}")
        action = p.ACT_PRESS if kind == "keydown" else p.ACT_RELEASE
        return p.MacroEvent(key.dll_key, action, p.PAGE_KEY, delay)
    if kind in ("mousedown", "mouseup"):
        action = p.ACT_PRESS if kind == "mousedown" else p.ACT_RELEASE
        return p.MacroEvent(int(item["button"]), action, p.PAGE_MOUSE, delay)
    rotation = int(item.get("rotation", 1))
    wheel = p.WHEEL_UP if rotation in (-1, 120) else p.WHEEL_DOWN
    return p.MacroEvent(wheel, p.ACT_WHEEL, p.PAGE_MOUSE, delay)


def load(path: str | Path) -> tuple[list[p.MacroEvent], int]:
    """Return (events, playback type). File is a list of events or {"playback", "events"}."""
    doc = json.loads(Path(path).read_text())
    if isinstance(doc, list):
        doc = {"events": doc}
    playback = p.PLAYBACK[doc.get("playback", "play-one")]
    return [event_from_json(e) for e in doc["events"]], playback

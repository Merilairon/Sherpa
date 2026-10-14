"""Everest Max (3282:0001) command packets.

Every builder returns the 64-byte payload that Mountain's SDKDLL.dll passes to
its HID write routine (FUN_1000be60). The transport prepends the report ID.
Function addresses refer to SDKDLL.dll shipped with Base Camp 1.9.10; see
docs/protocol-everest-max.md for the evidence behind each layout.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable, Sequence

PACKET_LEN = 64
LED_COUNT = 126  # CustomStatic.color / CustomTable.effValue length

Color = tuple[int, int, int]


def _packet(head: Sequence[int], fill: int = 0x00) -> bytearray:
    buf = bytearray([fill]) * PACKET_LEN
    buf[: len(head)] = bytes(head)
    return buf


# --- session ---------------------------------------------------------------


def ap_enable(on: bool) -> bytes:
    """APEnable -> FUN_10008f60: 11 80 00 00 <on>."""
    return bytes(_packet([0x11, 0x80, 0x00, 0x00, int(on)]))


def save_flash(effect_menu_index: int) -> bytes:
    """SaveFlash -> FUN_10009e10: 13 55 00 00 <menu>. SDK waits 1300 ms for the reply."""
    if not 0 <= effect_menu_index < 9:
        raise ValueError("effect menu index must be 0..8")
    return bytes(_packet([0x13, 0x55, 0x00, 0x00, effect_menu_index]))


# --- lighting --------------------------------------------------------------

# Lighting.EffectIndex in BaseCamp.Data.dll
EFFECTS = {
    "static": 0,
    "breathing": 1,
    "reactive": 3,
    "colorwave": 4,
    "yeti": 6,
    "tornado": 7,
    "matrix": 9,
    "custom": 10,
    "reactive-c": 11,
    "off": 12,
}
# ChangeEffect only emits a packet for these; colorwave/tornado use ChangeBlockEffect.
SIMPLE_EFFECTS = {0, 1, 3, 6, 9, 11, 12}

RAND_SINGLE, RAND_RANDOM, RAND_DUAL = 0, 2, 16


def firmware_speed(effect: int, speed: int) -> int:
    """UI speed 0..100 -> firmware step, as done inside SDKDLL (FUN_10005950)."""
    s = speed & 0xFF
    if effect in (1, 3, 5, 11):
        if s < 13:
            return 5
        if s < 38:
            return 4
        if s < 63:
            return 3
        return 1 if s < 88 else 0
    if effect in (4, 7):
        if s < 13:
            return 10
        if s < 38:
            return 9
        if s < 63:
            return 8
        return 7 if s < 88 else 6
    if effect == 9:
        if s < 13:
            return 20
        if s < 38:
            return 15
        if s < 63:
            return 10
        return 5 if s < 88 else 0
    if effect == 6:
        if s < 13:
            return 10
        if s < 38:
            return 7
        if s < 63:
            return 5
        return 3 if s < 88 else 0
    return s


@dataclass
class Effect:
    """EffData (62 bytes) as built by EverestDataHelper.getChangeEffect."""

    index: int
    brightness: int = 100
    speed: int = 50
    colors: list[Color] = field(default_factory=lambda: [(255, 255, 255)])
    background: Color = (0, 0, 0)
    rand: int = RAND_SINGLE
    apply_all: int = 0

    def to_effdata(self) -> bytes:
        d = bytearray(62)
        d[0] = self.index
        d[1] = self.apply_all
        d[2] = 0xFF if self.index in (0, 12) else self.speed
        d[3] = self.brightness
        d[4] = self.rand
        d[5] = 0xFF  # byDirection
        d[6] = 0xFF  # byWidth
        for i, c in enumerate(self.colors[:3]):
            d[7 + 3 * i : 10 + 3 * i] = bytes(c)
        d[16:19] = bytes(self.background)
        return bytes(d)


def change_effect(effect: Effect) -> bytes:
    """ChangeEffect (0x10006c60) -> FUN_1000a310: 14 2C + EffData[62].

    The SDK rewrites speed/rand/direction/width before sending; mirrored here.
    """
    if effect.index not in SIMPLE_EFFECTS:
        raise ValueError(f"effect {effect.index} is not sent through ChangeEffect")
    d = bytearray(effect.to_effdata())
    idx = d[0]
    d[5] = d[6] = 0xFF
    if idx == 0:
        if d[4] != RAND_RANDOM:
            d[4] = 0
        d[2] = 0xFF
    else:
        if idx == 6:
            d[4] = 0
        elif idx == 12:
            d[4] = 0xFF
        d[2] = firmware_speed(idx, d[2])
    return bytes([0x14, 0x2C]) + bytes(d)


def switch_to_custom(brightness: int) -> bytes:
    """SwitchToCustomizeEffect -> FUN_1000a3d0: FF-filled, 14 2C 0A 00 FF <brightness>."""
    buf = _packet([0x14, 0x2C, 0x0A, 0x00], fill=0xFF)
    buf[5] = brightness
    return bytes(buf)


def custom_static(colors: Sequence[Color]) -> list[bytes]:
    """ChangeCustomizeStatic -> FUN_1000a700.

    126 RGB triplets (indexed by DLLMatrixIndex) = 378 bytes, sent as 7 chunks:
    14 2C 00 01 <chunk> 4B 00 + up to 57 data bytes.
    """
    if len(colors) != LED_COUNT:
        raise ValueError(f"need {LED_COUNT} colours, got {len(colors)}")
    blob = b"".join(bytes(c) for c in colors)
    out = []
    for chunk, off in enumerate(range(0, len(blob), 57)):
        buf = _packet([0x14, 0x2C, 0x00, 0x01, chunk, 0x4B, 0x00])
        part = blob[off : off + 57]
        buf[7 : 7 + len(part)] = part
        out.append(bytes(buf))
    return out


def customize_table(effects: Sequence[int]) -> list[bytes]:
    """SetCustomizeTable -> FUN_1000a480: per-LED effect id, 126 bytes in 3 chunks.

    14 A0 <chunk> 01 + up to 60 bytes.
    """
    if len(effects) != LED_COUNT:
        raise ValueError(f"need {LED_COUNT} entries, got {len(effects)}")
    blob = bytes(effects)
    out = []
    for chunk, off in enumerate(range(0, len(blob), 60)):
        buf = _packet([0x14, 0xA0, chunk, 0x01])
        part = blob[off : off + 60]
        buf[4 : 4 + len(part)] = part
        out.append(bytes(buf))
    return out


# --- macros ----------------------------------------------------------------

PAGE_KEY, PAGE_MOUSE = 0, 1
ACT_RELEASE, ACT_PRESS, ACT_WHEEL = 1, 2, 3
MOUSE_LEFT, MOUSE_RIGHT, MOUSE_MIDDLE = 1, 2, 3
WHEEL_DOWN, WHEEL_UP = 48, 49

# KeyboardMacro.PlaybackOption -> iType (EverestDataHelper.GetMacroInputContent)
PLAYBACK = {"play-one": 1, "play-two": 4, "repeat": 3}
MAX_EVENTS = 1000


@dataclass(frozen=True)
class MacroEvent:
    """KEYMAP_EVENT: keyByte, (page << 4 | action), timer (ms, LE16)."""

    key: int
    action: int
    page: int = PAGE_KEY
    delay_ms: int = 0

    def pack(self) -> bytes:
        return bytes(
            [self.key & 0xFF, ((self.page & 0xF) << 4) | (self.action & 0xF)]
        ) + (self.delay_ms & 0xFFFF).to_bytes(2, "little")


END_EVENT = MacroEvent(key=0xFF, action=0xF, page=0xF, delay_ms=0)


def macro(src_key: int, events: Iterable[MacroEvent], playback: int = 1, times: int = 1) -> list[bytes]:
    """SetFullMacroData (0x100074b0) -> FUN_1000b9d0.

    Header 14 18 <type> 00 <times>, then chunks of up to 15 events:
    15 <src_key> <count> <continuation> + count*4 bytes.
    Base Camp always appends END_EVENT; so do we.
    """
    evs = list(events) + [END_EVENT]
    if len(evs) > MAX_EVENTS:
        raise ValueError(f"macro too long ({len(evs)} > {MAX_EVENTS} events)")
    out = [bytes(_packet([0x14, 0x18, playback, 0x00, times]))]
    for i in range(0, len(evs), 15):
        part = evs[i : i + 15]
        buf = _packet([0x15, src_key, len(part), int(i != 0)])
        data = b"".join(e.pack() for e in part)
        buf[4 : 4 + len(data)] = data
        out.append(bytes(buf))
    return out


# --- replies ---------------------------------------------------------------

NAK = (0xFF, 0xAA)
KEY_STATE_REPORT = 0x01


def is_reply_to(request: bytes, reply: bytes) -> bool:
    """The SDK's reader thread completes a request when bytes 0..3 echo back."""
    return len(reply) >= 4 and reply[:4] == request[:4]


def is_nak(reply: bytes) -> bool:
    return len(reply) >= 2 and (reply[0], reply[1]) == NAK

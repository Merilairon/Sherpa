"""Linux hidraw transport for Mountain devices (no third-party dependencies)."""

from __future__ import annotations

import os
import select
import time
from dataclasses import dataclass
from pathlib import Path

from . import protocol

VENDOR_ID = 0x3282
# Product IDs found in the Base Camp 1.9.10 SDKs.
PRODUCTS = {
    0x0001: "Everest Max",
    0x0002: "Makalu",
    0x0003: "Makalu 67",
    0x0004: "Makalu 67",
    0x0005: "Everest 60",
    0x0006: "Everest 60",
    0x0008: "MacroPad",
    0x0009: "DisplayPad",
}
VENDOR_USAGE_PAGE = 0xFF00
VENDOR_USAGE = 0x0001

SYSFS = Path("/sys/class/hidraw")


@dataclass
class Node:
    path: str
    vid: int
    pid: int
    report_id: int | None  # None: descriptor has no vendor collection

    @property
    def name(self) -> str:
        return PRODUCTS.get(self.pid, f"unknown {self.pid:04x}")


def vendor_report_id(descriptor: bytes) -> int | None:
    """Return the report ID used inside the FF00:0001 application collection.

    0 means the collection exists but uses unnumbered reports. The Windows SDK
    picks this collection by usage page/usage (SDKDLL FUN_10008880) and writes
    caps.ReportID followed by the 64-byte payload.
    """
    i, page, usages, depth = 0, 0, [], 0
    report_id = 0  # Report ID is a global item: it persists until redefined
    vendor_depth = None
    while i < len(descriptor):
        prefix = descriptor[i]
        if prefix == 0xFE:  # long item
            i += 3 + descriptor[i + 1]
            continue
        size = (0, 1, 2, 4)[prefix & 0x3]
        value = int.from_bytes(descriptor[i + 1 : i + 1 + size], "little")
        tag = prefix & 0xFC
        i += 1 + size
        if tag == 0x04:  # Usage Page
            page = value
        elif tag == 0x08:  # Usage
            usages.append(value & 0xFFFF)
        elif tag == 0x84:  # Report ID
            report_id = value
        elif tag == 0xA0:  # Collection
            depth += 1
            if vendor_depth is None and value == 0x01 and page == VENDOR_USAGE_PAGE and VENDOR_USAGE in usages:
                vendor_depth = depth
            usages = []
        elif tag in (0x80, 0x90, 0xB0):  # Input / Output / Feature
            if vendor_depth is not None:
                return report_id
            usages = []
        elif tag == 0xC0:  # End Collection
            if depth == vendor_depth:
                return report_id
            depth -= 1
    return None


def _hid_id(uevent: str) -> tuple[int, int] | None:
    for line in uevent.splitlines():
        if line.startswith("HID_ID="):
            _bus, vid, pid = line.split("=", 1)[1].split(":")
            return int(vid, 16), int(pid, 16)
    return None


def find(pid: int | None = None) -> list[Node]:
    nodes = []
    for entry in sorted(SYSFS.glob("hidraw*")):
        try:
            ids = _hid_id((entry / "device" / "uevent").read_text())
            desc = (entry / "device" / "report_descriptor").read_bytes()
        except OSError:
            continue
        if not ids or ids[0] != VENDOR_ID or (pid is not None and ids[1] != pid):
            continue
        nodes.append(Node(f"/dev/{entry.name}", ids[0], ids[1], vendor_report_id(desc)))
    return nodes


class ProtocolError(RuntimeError):
    pass


class Device:
    """One open vendor-collection hidraw node.

    Mirrors the SDK's request/response cycle: write report, then wait for an
    input report whose first four bytes echo the request (FF AA = rejected).
    """

    def __init__(self, node: Node, dry_run: bool = False, log=None):
        if node.report_id is None:
            raise ProtocolError(f"{node.path} has no FF00:0001 collection")
        self.node = node
        self.dry_run = dry_run
        self.log = log
        self.fd = None if dry_run else os.open(node.path, os.O_RDWR)

    def close(self) -> None:
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def __enter__(self) -> "Device":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def _read(self, timeout: float) -> bytes | None:
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return None
        data = os.read(self.fd, 128)
        if self.node.report_id:
            data = data[1:]
        return data

    def request(self, payload: bytes, timeout: float = 0.6) -> bytes | None:
        if len(payload) != protocol.PACKET_LEN:
            raise ValueError("payload must be 64 bytes")
        if self.log:
            self.log(payload)
        if self.dry_run:
            return None
        os.write(self.fd, bytes([self.node.report_id]) + payload)
        deadline = time.monotonic() + timeout
        while (left := deadline - time.monotonic()) > 0:
            reply = self._read(left)
            if reply is None:
                break
            if protocol.is_nak(reply):
                raise ProtocolError(f"device rejected {payload[:4].hex(' ')}")
            if protocol.is_reply_to(payload, reply):
                return reply
            # key-state (0x01) and unrelated reports are skipped
        raise ProtocolError(f"no reply to {payload[:4].hex(' ')} within {timeout}s")

    def send_all(self, payloads, timeout: float = 0.6) -> None:
        for p in payloads:
            self.request(p, timeout)

"""Everest Max key table extracted from Base Camp (EverestDataHelper.GetEverestKeys_English).

Each key carries three ids:
  dll_key   firmware key code (macro source key, ChangeKey, macro events)
  led       index into the 126-entry per-key colour array (DLLMatrixIndex)
  vk        Windows virtual-key code Base Camp records macros with
"""

from __future__ import annotations

import csv
from dataclasses import dataclass
from functools import lru_cache
from importlib import resources


@dataclass(frozen=True)
class Key:
    dll_key: int
    led: int
    vk: int
    name: str


@lru_cache(maxsize=1)
def table() -> tuple[Key, ...]:
    text = resources.files(__package__).joinpath("data/everest_max_keys.csv").read_text("utf-8")
    rows = csv.DictReader(text.splitlines())
    return tuple(
        Key(int(r["DLLKeyId"]), int(r["DLLMatrixIndex"]), int(r["KeyId"]), r["KeyName"]) for r in rows
    )


def _norm(s: str) -> str:
    return "".join(s.split()).lower()


def by_name(name: str) -> Key:
    want = _norm(name)
    for k in table():
        if _norm(k.name) == want:
            return k
    raise KeyError(f"unknown key {name!r}; run `basecamp-linux keys` for the list")


def by_vk(vk: int) -> Key | None:
    return next((k for k in table() if k.vk == vk), None)

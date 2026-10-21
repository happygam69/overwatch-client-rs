"""Load captured process regions and resolve virtual addresses.

The dumps in dev/dumps_exec are raw copies of committed, executable memory at
the addresses Windows reported.  Keeping the recorded base address lets us
disassemble at true virtual addresses, which matters because nearly every
memory operand in x64 is RIP-relative: only with the right address do those
operands resolve to something we can look up.
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass

# The module allocation base moves per session, and within one session every
# module address moves by the same amount.  dumps_range predates dumps_v2; the
# uniform delta between those sessions is 0x17AEC0000, verified against region
# bases, thunk addresses and embedded pointers.  Tools that hold hardcoded
# addresses read `OW_DELTA` and add it, so the whole pipeline translates to a
# newer capture with one number.
DELTA = int(os.environ.get("OW_DELTA", "0"), 16)


def capture_dir(default: str) -> str:
    """Capture directory a tool should read, overridable for a newer capture.

    The captures are tens of GB and live on D: now (E: ran out of space), so a
    bare name resolves against, in order: $OW_DUMPS_ROOT, the local directory,
    `D:/ow-dumps`, in whichever one the directory actually exists.
    """
    name = os.environ.get("OW_DUMPS", default)
    if os.path.isabs(name) or os.path.exists(name):
        return name
    roots = [
        os.environ.get("OW_DUMPS_ROOT", ""),
        os.path.dirname(os.path.abspath(__file__)),
        "D:/ow-dumps",
    ]
    for root in roots:
        if root and os.path.isdir(os.path.join(root, name)):
            return os.path.join(root, name)
    return name


def out_name(default: str) -> str:
    """Output file a tool should write, overridable so runs stay side by side."""
    return os.environ.get("OW_OUT", default)


@dataclass
class Region:
    base: int
    size: int
    protect: int
    kind: int
    path: str
    _data: bytes | None = None

    @property
    def end(self) -> int:
        return self.base + self.size

    def data(self) -> bytes:
        if self._data is None:
            with open(self.path, "rb") as f:
                self._data = f.read()
        return self._data

    def contains(self, addr: int) -> bool:
        return self.base <= addr < self.end

    def slice(self, addr: int, length: int) -> bytes | None:
        if not self.contains(addr):
            return None
        off = addr - self.base
        return self.data()[off : off + length]


def load(directory: str | None = None) -> list[Region]:
    regions: list[Region] = []
    dump_dir = directory if directory else capture_dir("dumps_exec")
    # A merged manifest wins: the dumper writes one manifest per run, and
    # later runs leave files the newest manifest does not list (558 such files
    # in dumps_range).  merge_manifests.py folds them into manifest_full.txt.
    manifest = os.path.join(dump_dir, "manifest_full.txt")
    if not os.path.exists(manifest):
        manifest = os.path.join(dump_dir, "regions.txt")
    if not os.path.exists(manifest):
        manifest = os.path.join(dump_dir, "manifest.txt")
    with open(manifest) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split()
            if len(parts) < 5:
                continue
            base, size, protect, kind = (int(p, 16) for p in parts[:4])
            # Older manifests carry an extra allocbase column before the file.
            path = parts[-1]
            regions.append(Region(base, size, protect, kind, os.path.join(dump_dir, path)))
    regions.sort(key=lambda r: r.base)
    return regions


class Space:
    """Address space built from the captured regions."""

    def __init__(self, regions: list[Region]):
        self.regions = regions
        self._starts = [r.base for r in regions]

    def find(self, addr: int) -> Region | None:
        import bisect

        i = bisect.bisect_right(self._starts, addr) - 1
        if i < 0:
            return None
        r = self.regions[i]
        return r if r.contains(addr) else None

    def read(self, addr: int, length: int) -> bytes | None:
        r = self.find(addr)
        if r is None:
            return None
        off = addr - r.base
        chunk = r.data()[off : off + length]
        return chunk if len(chunk) == length else None

    def cstring(self, addr: int, limit: int = 200) -> str | None:
        r = self.find(addr)
        if r is None:
            return None
        off = addr - r.base
        data = r.data()
        end = data.find(b"\0", off, off + limit)
        if end < 0:
            return None
        raw = data[off:end]
        if not raw or len(raw) < 2:
            return None
        try:
            text = raw.decode("ascii")
        except UnicodeDecodeError:
            return None
        return text if all(32 <= ord(c) < 127 for c in text) else None

    def u64(self, addr: int) -> int | None:
        b = self.read(addr, 8)
        return int.from_bytes(b, "little") if b else None

    def u32(self, addr: int) -> int | None:
        b = self.read(addr, 4)
        return int.from_bytes(b, "little") if b else None


FIELD_HASH_RE = re.compile(r"0x([0-9A-Fa-f][0-9A-Fa-f_]{7})\b")
SRC_DIRS = ["crates/ow-tank/src", "crates/ow-sim/src", "crates/ow-range/src", "crates/ow-stu/src"]

# Field hashes recovered from the shipped STU data itself (the generated class
# layouts name the fields of the states the graphs actually use), plus the ones
# documented in docs/formats/weapons.md.  These are the parameters combat
# cares about.
DATA_FIELD_HASHES = {
    0x02F3F5D4: "numShotsPerSecond",
    0xF2AE0897: "numProjectilesPerShot",
    0x26E68021: "projectileSpeed",
    0xBA490EB9: "projectileLifetime",
    0x351BD93F: "projectileMotions",
    0xBAA390E1: "projectileEntity",
    0x81F1D90B: "projectileVisual",
    0xF9808409: "aimID",
    0x07DD813E: "m_value",
    0xAF872E86: "ModifyHealth.m_amount",
    0x7C3457DC: "hero",
    0x3BD16B9E: "weaponComponent.weapons",
    0xC71EA6BC: "weaponDefinition.graph",
    0x4D5597A8: "weaponDefinition.script",
    0x6EFDBD73: "moverComponent",
    0x031FE7EE: "mover.speed",
    0x15CB6532: "weaponComponent",
    0x61386B75: "weaponDefinition",
    0x950BBA06: "hero.entity",
    0xEED6D952: "WeaponVolley",
    0xA658CA1C: "healthComponent",
    0x7D88A63A: "gender",
    0x7CEA65C7: "max",
    0x7594F80E: "min",
    0x2C54AEAF: "category",
    0x4D2DB658: "identifier",
    0xB48F1D22: "name",
    0x45A7A69C: "default",
    0x9321E2B9: "step",
    0x7236F6E3: "statescriptGraph",
    0x5866C45E: "statescriptSchemaEntry",
    0x0D09D2D9: "token",
    0xE25DDDA1: "heroColor",
    0xAF4EC410: "heroSize",
    0x77FED604: "heroLoadout",
    0xB87D57AA: "heroSettingCategory",
    0x76B1DB1F: "heroSettingFloat",
    0x90A50AC1: "heroSettingIdentifier",
    0x12E434A1: "configVarInt",
    0x7B45B325: "configVarFloat",
    0xB567AADA: "configVarBool",
}


def known_hashes(repo_root: str) -> dict[int, str]:
    """Collect the field hashes the project already names, with their names.

    The Rust sources carry these as `pub const NAME: u32 = 0x...`, so parsing
    them keeps this tool honest about what is actually known.
    """
    out: dict[int, str] = {}
    const_re = re.compile(r"pub const ([A-Za-z_][A-Za-z0-9_]*)\s*:\s*u32\s*=\s*(0x[0-9A-Fa-f_]+)")
    for d in SRC_DIRS:
        full = os.path.join(repo_root, d)
        if not os.path.isdir(full):
            continue
        for root, _dirs, files in os.walk(full):
            for name in files:
                if not name.endswith(".rs"):
                    continue
                with open(os.path.join(root, name), encoding="utf-8", errors="replace") as f:
                    text = f.read()
                for m in const_re.finditer(text):
                    value = int(m.group(2).replace("_", ""), 16)
                    if value < 0x10000:
                        continue
                    out.setdefault(value, m.group(1))
    for value, name in DATA_FIELD_HASHES.items():
        out.setdefault(value, name)
    return out

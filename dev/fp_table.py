"""Decode the client's config-var name table and read each var's default value.

The name table tiles memory as `{u8 key[8]; u32 count|flag<<24; char str[count+1]}`
with the string XOR-obfuscated by the key *until* the client decodes it in place
at startup (the flag byte then reads 0, which is how a capture can hold
plaintext -- the 2026-10-15 captures do).  Records are contiguous, so walking
forward from a verified record recovers the list: a name is a record with no
whitespace, its description the record that follows it.

Every var is registered by a call whose 4th argument (`r8`) points at the var's
**default value**, so following that gives the client's own defaults -- for
`firstPerson.OffsetX` the offset the client uses, not a fitted guess.  Arguments
are recovered by byte pattern (`48/4c 8d 05/0d/15/1d <disp32>`), which needs no
instruction alignment.

Usage:
    python fp_table.py [--filter SUBSTR] [--image PATH] [--blob N]

Read-only; nothing is written.
"""

from __future__ import annotations

import struct
import sys

import numpy as np

IMAGE = "D:/ow-dumps/module_v2full.bin"
BASE = 0x7FF7C4D20000
REGISTRAR = 0x7FF7C6994450

# Verified record starts (count-word addresses) in the v2b image: the offset-X
# name, and the first camera.FirstPerson* name.  Both were read out of the
# registration code (the decode loop's count operand), not guessed.
SEEDS = [0x7FF7C869DF20, 0x7FF7C86957C8]

# `lea r<reg>, [rip+disp32]`
LEA_RIP = {
    b"\x48\x8d\x05": "rax",
    b"\x48\x8d\x0d": "rcx",
    b"\x48\x8d\x15": "rdx",
    b"\x4c\x8d\x05": "r8",
    b"\x4c\x8d\x0d": "r9",
}


class Image:
    def __init__(self, path: str, base: int):
        self.data = np.fromfile(path, dtype=np.uint8)
        self.raw = self.data.tobytes()
        self.base = base

    def at(self, addr: int, n: int) -> bytes:
        return self.raw[addr - self.base : addr - self.base + n]

    def u32(self, addr: int) -> int:
        return struct.unpack("<I", self.at(addr, 4))[0]

    def text(self, r: int) -> tuple[str, int] | None:
        """The string of the record whose count word is at `r`, and its end."""
        n = self.u32(r) & 0xFFFFFF
        if n == 0 or n > 400:
            return None
        raw = self.at(r + 4, n)
        key = self.at(r - 8, 8)
        for s in (raw, bytes(c ^ key[i & 7] for i, c in enumerate(raw))):
            try:
                t = s.decode("ascii")
            except UnicodeDecodeError:
                continue
            if t and all(0x20 <= ord(c) < 0x7F for c in t):
                # The 8-byte key that follows is 8-aligned, so the next
                # count word sits 8 bytes after the aligned end of the nul.
                return t, ((r + 4 + n + 1 + 7) & ~7) + 8
        return None

    def walk(self, seed: int, limit: int = 4000) -> list[tuple[int, str]]:
        out, r = [], seed
        while len(out) < limit:
            got = self.text(r)
            if got is None:
                break
            t, end = got
            out.append((r, t))
            r = end
        return out


def refs(img: Image, targets: set[int]) -> dict[int, list[int]]:
    data = img.data
    n = len(data)
    hits: dict[int, list[int]] = {t: [] for t in targets}
    for shift in range(4):
        v = np.frombuffer(data[shift:], dtype="<u4", count=(n - shift) // 4)
        offs = img.base + shift + 4 + np.arange(len(v), dtype=np.uint64) * 4
        cand = offs + v.astype(np.uint64)
        for t in targets:
            for i in np.nonzero(cand == t)[0]:
                hits[t].append(int(offs[i]) - 4)
    return hits


def leas(img: Image, lo: int, hi: int) -> dict[str, int]:
    """Last RIP-relative `lea` into each register within [lo, hi)."""
    out: dict[str, int] = {}
    for off in range(lo - img.base, hi - img.base):
        for pat, reg in LEA_RIP.items():
            if img.raw[off : off + 3] == pat:
                disp = struct.unpack("<i", img.raw[off + 3 : off + 7])[0]
                out[reg] = off + img.base + 7 + disp
    return out


def registrar_call(img: Image, lo: int, hi: int) -> int | None:
    for off in range(lo - img.base, hi - img.base):
        if img.raw[off] == 0xE8:
            disp = struct.unpack("<i", img.raw[off + 1 : off + 5])[0]
            if off + img.base + 5 + disp == REGISTRAR:
                return off + img.base
    return None


def main() -> int:
    args = sys.argv[1:]
    image = args[args.index("--image") + 1] if "--image" in args else IMAGE
    blob_len = int(args[args.index("--blob") + 1]) if "--blob" in args else 16
    want = args[args.index("--filter") + 1] if "--filter" in args else ""
    img = Image(image, BASE)

    table: list[tuple[int, str, int]] = []
    for seed in SEEDS:
        for r, t in img.walk(seed):
            table.append((r, t, r + 4))
    table.sort()
    print(f"{len(table)} record(s) from {len(SEEDS)} seed(s)")

    # A name has no whitespace; its description is the next record.
    names = [
        (r, t, table[i + 1][1] if i + 1 < len(table) and " " in table[i + 1][1] else "")
        for i, (r, t, _) in enumerate(table)
        if " " not in t
    ]
    print(f"{len(names)} name record(s)")
    if want:
        names = [(r, t, d) for r, t, d in names if want.lower() in t.lower()]
        print(f"{len(names)} matching {want!r}")

    hits = refs(img, {r + 4 for r, _, _ in names})
    for r, t, desc in names:
        sites = hits.get(r + 4, [])
        print(f"\n=== {t}    (record 0x{r:x})")
        if desc:
            print(f"    desc: {desc}")
        if not sites:
            print("    (unreferenced)")
            continue
        for site in sites:
            arg = leas(img, site - 0x40, site + 0x40)
            call = registrar_call(img, site, site + 0x60)
            print(f"    site 0x{site:x}  registrar call {'0x%x' % call if call else '(none)'}")
            if "r8" in arg:
                blob = img.at(arg["r8"], blob_len)
                print(f"      default @0x{arg['r8']:x}"
                      f"  floats {[round(f, 6) for f in struct.unpack(f'<{blob_len // 4}f', blob)]}"
                      f"  u32 {struct.unpack(f'<{blob_len // 4}I', blob)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Where the client's own code compares an STU field hash or class hash.

The STU reflection layer reads a field by comparing its hash against immediates
in a per-class dispatch function (the pattern established for `C`: `0x141334ea0`
and `0x141335e60` compare 22 of the class's declared field hashes). So "does any
client code consume this field?" is a bounded immediate scan, not a census: a
consumer that reads the field through the reflection layer must carry the hash,
and a consumer that reads it from a typed offset shows up as the offset instead.

Usage:
    python dev/static_field_scan.py 0xCB4D298D 0x2DD96D6D 0x537E56E4

Reports, per value, every 4-byte little-endian occurrence with the section it
sits in and the containing `.pdata` function when there is one.
"""

from __future__ import annotations

import struct
import sys

IMAGE = "dev/data/offline_recover/module_offline.bin"
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
BASE = 0x140000000


def sections(exe: bytes) -> list[tuple[str, int, int]]:
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, o + 8)
        out.append((name, va + BASE, va + BASE + max(vsz, rsz)))
    return out


def pdata(exe: bytes) -> list[tuple[int, int]]:
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    table = None
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, o + 8)
        if name == ".pdata":
            table = (ro, max(vsz, rsz))
    raw = exe[table[0]:table[0] + table[1]]
    out = []
    for i in range(len(raw) // 12):
        beg, end = struct.unpack_from("<II", raw, 12 * i)
        if end > beg:
            out.append((beg + BASE, end + BASE))
    out.sort()
    return out


def main() -> int:
    img = open(IMAGE, "rb").read()
    exe = open(EXE, "rb").read()
    secs = sections(exe)
    ranges = pdata(exe)

    def section_of(va: int) -> str:
        for name, lo, hi in secs:
            if lo <= va < hi:
                return name
        return "?"

    def function_of(va: int) -> str:
        lo, hi = 0, len(ranges) - 1
        best = None
        while lo <= hi:
            mid = (lo + hi) // 2
            beg, end = ranges[mid]
            if va < beg:
                hi = mid - 1
            elif va >= end:
                lo = mid + 1
            else:
                best = (beg, end)
                break
        return f"{best[0]:#x}+{va - best[0]:#x}" if best else "-"

    for arg in sys.argv[1:]:
        value = int(arg, 16)
        pat = struct.pack("<I", value & 0xFFFFFFFF)
        hits = []
        i = 0
        while True:
            i = img.find(pat, i)
            if i < 0 or i + 4 > len(img):
                break
            va = i + BASE
            hits.append((va, section_of(va), function_of(va)))
            i += 1
        print(f"{value:#010x}: {len(hits)} occurrence(s)")
        for va, sec, fn in hits[:24]:
            print(f"    {va:#x}  {sec:<8} fn {fn}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

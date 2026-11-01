"""Classify an address in the install's PE: raw-backed, zero-fill (BSS), or unmapped.

    python dev/pe_layout.py --sections
    python dev/pe_layout.py --addr 0x143c3c620 0x1438846c0
    python dev/pe_layout.py --check

Why this exists: `.data` has a **zero-filled tail** — its virtual size is larger
than its raw size, so everything past `raw_end` is written as zero by the loader
and filled by the client at runtime. A Ghidra read there returns zeros, and
those zeros are not data: they are the absence of data. Mistaking them for a
value has already cost one wrong conclusion (the "field table" at
`0x143c3c620`, and earlier the `0x141d71200` fill). This tool makes the
distinction mechanical: an address is reported as `raw` (the install's own
bytes, readable), `zero-fill` (BSS: runtime-only, read the writer instead), or
`unmapped`.

`--check` pins the three addresses the collision investigation depends on, so a
future image whose section table changed fails loudly instead of quietly
returning zeros.

Reads the install in place; writes nothing.
"""

from __future__ import annotations

import argparse
import pathlib
import struct
import sys

INSTALL = pathlib.Path(r"E:\Overwatch\_retail_\Overwatch.exe")
BASE = 0x140000000


def sections(image: bytes) -> list[dict[str, int | str]]:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out: list[dict[str, int | str]] = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out.append(
            {
                "name": name,
                "va": va,
                "vsz": vsz,
                "raw": ro,
                "rsz": rsz,
                "raw_end": va + rsz,
                "virt_end": va + max(vsz, rsz),
            }
        )
    return out


def classify(secs: list[dict[str, int | str]], va: int) -> tuple[str, dict[str, int | str] | None]:
    rva = va - BASE
    for s in secs:
        if s["va"] <= rva < s["virt_end"]:  # type: ignore[operator]
            kind = "raw" if rva < s["raw_end"] else "zero-fill"  # type: ignore[operator]
            return kind, s
    return "unmapped", None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--install", type=pathlib.Path, default=INSTALL)
    ap.add_argument("--sections", action="store_true", help="print the section table")
    ap.add_argument("--addr", nargs="+", default=[], help="classify one or more VAs")
    ap.add_argument("--check", action="store_true", help="assert the pinned layout facts")
    args = ap.parse_args()

    if not args.install.exists():
        print(f"install not found: {args.install}", file=sys.stderr)
        return 2
    image = args.install.read_bytes()
    secs = sections(image)

    if args.sections:
        for s in secs:
            print(
                f"{s['name']:10s} va={s['va']:#010x} vsz={s['vsz']:#09x} "
                f"raw={s['raw']:#010x} rsz={s['rsz']:#09x} "
                f"raw_end={s['raw_end']:#010x} virt_end={s['virt_end']:#010x}"
            )

    for text in args.addr:
        va = int(text, 0)
        kind, s = classify(secs, va)
        where = f"{s['name']}" if s else "-"
        print(f"{va:#018x}  {kind:9s} {where}")

    if args.check:
        pinned = [
            # The collision component's field table: BSS, zero at rest.
            (0x143C3C620, "zero-fill"),
            # The descriptor's type-info buffer: BSS too.
            (0x143C3EFC0, "zero-fill"),
            # The XOR'd class-name table: real install bytes.
            (0x1438846C0, "raw"),
            # A .rdata vtable: real install bytes.
            (0x143367670, "raw"),
        ]
        bad = 0
        for va, want in pinned:
            kind, _ = classify(secs, va)
            mark = "ok" if kind == want else "FAIL"
            bad += kind != want
            print(f"check {va:#018x} expect {want:9s} got {kind:9s} {mark}")
        data = next(s for s in secs if s["name"] == ".data")
        print(
            f".data raw-backed {data['va']:#x}..{data['raw_end']:#x}, "
            f"zero-fill {data['raw_end']:#x}..{data['virt_end']:#x}"
        )
        if bad:
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

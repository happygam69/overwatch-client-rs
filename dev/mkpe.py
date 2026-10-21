"""Wrap raw captured regions as one minimal valid PE for analysis tools.

The module_*.bin synthetic full-module PEs carry junk in their data
directories (invalid export/debug/TLS RVAs), which some PE importers reject
outright. A small set of regions wrapped in a clean PE imports everywhere,
including REA's headless Ghidra, and keeps each region at the exact virtual
address it had in the capture.

Layout: ImageBase = (lowest region base) - 0x1000; each region gets a section
at RVA = region_base - ImageBase. No directories, no relocations, no entry
point. Addresses inside the resulting image equal capture addresses.

Usage:
    python mkpe.py <out.exe> <base-hex> <region.bin> [<base-hex> <region.bin> ...]
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

FILE_ALIGN = 0x200
SECT_ALIGN = 0x1000
HDR_SIZE = 0x400  # DOS stub + PE header + section headers (PE header at 0x80)


def build(pairs: list[tuple[int, bytes]]) -> bytes:
    pairs = sorted(pairs, key=lambda p: p[0])
    image_base = (pairs[0][0] - SECT_ALIGN) & ~0xFFF
    sections = []  # (name, rva, raw_ptr, raw_size, vsize)
    blob = bytearray(b"\x00" * HDR_SIZE)
    for i, (base, region) in enumerate(pairs):
        raw_size = (len(region) + FILE_ALIGN - 1) // FILE_ALIGN * FILE_ALIGN
        raw_ptr = len(blob)
        rva = base - image_base
        assert rva >= SECT_ALIGN and rva % SECT_ALIGN == 0, hex(rva)
        blob += region + b"\x00" * (raw_size - len(region))
        name = f".t{i}" if i else ".text"
        sections.append((name.encode().ljust(8, b"\x00"), rva, raw_ptr, raw_size, len(region)))
    size_image = ((max(rva + rs for _, rva, _, rs, _ in
                       [(n, r, p, rs, vs) for n, r, p, rs, vs in sections])) + SECT_ALIGN - 1) & ~(SECT_ALIGN - 1)

    pe = bytearray()
    pe += b"PE\0\0"
    pe += struct.pack("<HHIIIHH", 0x8664, len(sections), 0, 0, 0, 0xF0, 0x0022)
    opt = bytearray()
    opt += struct.pack("<H", 0x20B)
    opt += struct.pack("<BB", 14, 0)
    opt += struct.pack("<III", 0, 0, 0)
    opt += struct.pack("<I", 0)
    opt += struct.pack("<I", sections[0][1])
    opt += struct.pack("<Q", image_base)
    opt += struct.pack("<II", SECT_ALIGN, FILE_ALIGN)
    opt += struct.pack("<HHHHHH", 6, 0, 0, 0, 6, 0)
    opt += struct.pack("<I", 0)
    opt += struct.pack("<I", size_image)
    opt += struct.pack("<I", HDR_SIZE)
    opt += struct.pack("<I", 0)
    opt += struct.pack("<HH", 3, 0x8160)
    opt += struct.pack("<QQQQ", 0x100000, 0x1000, 0x100000, 0x1000)
    opt += struct.pack("<I", 0)
    opt += struct.pack("<I", 16)
    opt += b"\x00" * (16 * 8)
    assert len(opt) == 0xF0
    pe += opt
    for name, rva, raw_ptr, raw_size, vsize in sections:
        pe += name + struct.pack("<IIII", vsize, rva, raw_size, raw_ptr)
        pe += struct.pack("<IIHHI", 0, 0, 0, 0, 0x60000020)

    buf = bytearray(HDR_SIZE)
    buf[0:2] = b"MZ"
    struct.pack_into("<I", buf, 0x3C, 0x80)
    assert 0x80 + len(pe) <= HDR_SIZE, (0x80 + len(pe))
    buf[0x80 : 0x80 + len(pe)] = pe
    blob[0:HDR_SIZE] = buf
    return bytes(blob)


def main() -> int:
    if len(sys.argv) < 4 or len(sys.argv) % 2 == 1:
        print(__doc__)
        return 2
    out_path = sys.argv[1]
    rest = sys.argv[2:]
    pairs = []
    for i in range(0, len(rest), 2):
        base = int(rest[i], 16)
        region = Path(rest[i + 1]).read_bytes()
        pairs.append((base, region))
    data = build(pairs)
    Path(out_path).write_bytes(data)
    print(f"{out_path}: {len(data)} bytes, image base {pairs and (min(b for b,_ in pairs) - 0x1000) & ~0xFFF:#x}, {len(pairs)} sections at {[hex(b) for b,_ in pairs]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

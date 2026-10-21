"""Identify a module dump: PE sections, entropy, and where known capture code lands.

Usage:
    python pe_probe.py <module.bin> [region.bin ...]

Prints the PE image base, sections with per-section entropy, and for each
region file given, searches for its first 64 bytes inside the module and
reports the file offset and the VA it maps to under the PE's own layout.
"""

from __future__ import annotations

import math
import struct
import sys
from pathlib import Path


def entropy(data: bytes) -> float:
    if not data:
        return 0.0
    counts = [0] * 256
    for b in data:
        counts[b] += 1
    n = len(data)
    return -sum((c / n) * math.log2(c / n) for c in counts if c)


def parse_pe(path: Path):
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise SystemExit(f"{path}: not MZ")
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    if data[e_lfanew : e_lfanew + 4] != b"PE\0\0":
        raise SystemExit(f"{path}: no PE signature at {e_lfanew:#x}")
    coff = e_lfanew + 4
    machine, nsec, _, _, _, opt_size, _ = struct.unpack_from("<HHIIIHH", data, coff)
    opt = coff + 20
    magic = struct.unpack_from("<H", data, opt)[0]
    pe32p = magic == 0x20B
    image_base = struct.unpack_from("<Q", data, opt + 24)[0] if pe32p else struct.unpack_from("<I", data, opt + 28)[0]
    size_image = struct.unpack_from("<I", data, opt + 56)[0]
    print(f"{path.name}: {len(data)} bytes  machine={machine:#x}  sections={nsec}")
    print(f"  image_base={image_base:#x}  size_image={size_image:#x}  entry={struct.unpack_from('<I', data, opt+16)[0]:#x}")
    secs = []
    so = opt + opt_size
    for i in range(nsec):
        name, vsize, va, raw_size, raw_off = struct.unpack_from("<8sIIII", data, so + i * 40)
        nm = name.rstrip(b"\0").decode(errors="replace")
        blob = data[raw_off : raw_off + min(raw_size, 4 << 20)]
        e = entropy(blob)
        secs.append((nm, va, vsize, raw_off, raw_size, e))
        print(f"  {nm:8s} va={va:#010x} vsize={vsize:#010x} raw={raw_off:#x}+{raw_size:#x} entropy={e:.3f}")
    return data, image_base, secs


def va_of(off: int, secs) -> int | None:
    for nm, va, vsize, raw_off, raw_size, _ in secs:
        if raw_off <= off < raw_off + raw_size:
            return va + (off - raw_off)
    return None


def main() -> int:
    mod_path = Path(sys.argv[1])
    data, image_base, secs = parse_pe(mod_path)
    for rp in sys.argv[2:]:
        region = Path(rp).read_bytes()
        needle = region[:64]
        idx = data.find(needle)
        if idx < 0:
            idx2 = data.find(region[:32])
            print(f"  {Path(rp).name}: 64-byte head not found; 32-byte head at {idx2 if idx2 >= 0 else 'nowhere'}")
            continue
        va = va_of(idx, secs)
        print(f"  {Path(rp).name}: found at file {idx:#x} -> rva-containing VA {va if va is None else hex(va)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

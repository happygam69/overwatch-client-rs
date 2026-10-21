"""Dump the complete Domino physics schema from the v2b capture.

The descriptor registry lives in .data at 0x7ff7c90eb700..0x7ff7c90ebb40 (the
region 0x7ff7c90e7000): one 0x20-byte record per class
    {qword name, qword field_array, u32 size, u32 count, qword method}
with the field arrays filled at runtime by per-class initializers (the dmBody
one is FUN_7ff7c7a38cbf: it writes name/offset/type triples with literal
immediates).  Because the capture is post-startup, every array is populated:
this reads the whole physics schema straight out of the live process.

Field record stride 0x18: {qword name ptr, u32 offset @+8, u32 pad @+0xc,
u32 type @+0x10, u32 pad @+0x14}.  A name slot may instead hold a code pointer
(a custom accessor), printed as `fun@...`.

Usage:
    OW_DUMPS=dumps_v2b python dump_domino_schema.py        # summary + full dump
    OW_DUMPS=dumps_v2b python dump_domino_schema.py dmWorld  # one class
"""

from __future__ import annotations

import struct
import sys

import regions as R

REG_LO, REG_HI = 0x7FF7C90EB700, 0x7FF7C90EBB40
STRIDE = 0x18
MOD_LO, MOD_HI = 0x7FF7C4D20000, 0x7FF7C9400000


def read_descriptors(space: R.Space) -> list[dict]:
    out = []
    a = REG_LO
    while a + 0x20 <= REG_HI:
        b = space.read(a, 0x20)
        if b is None:
            a += 0x10
            continue
        q0, q1, size_v, count_v, method = struct.unpack("<QQIIQ", b)
        name = space.cstring(q0) if MOD_LO <= q0 < MOD_HI else None
        if (
            name
            and all(32 <= ord(c) < 127 for c in name)
            and 2 <= len(name) <= 40
            and MOD_LO <= q1 < MOD_HI
            and 0 < count_v < 0x1000
        ):
            out.append(
                {"rec": a, "name": name, "arr": q1, "size": size_v, "count": count_v, "method": method}
            )
        a += 0x10
    return out


def dump_class(space: R.Space, d: dict, out) -> None:
    print(f"\n=== {d['name']}  size={d['size']:#x} count={d['count']} method={d['method']:#x}", file=out)
    for i in range(d["count"]):
        b = space.read(d["arr"] + i * STRIDE, STRIDE)
        if b is None:
            print(f"  [{i:2}] <no bytes>", file=out)
            break
        q0 = struct.unpack_from("<Q", b)[0]
        off, _pad, typ, _pad2 = struct.unpack_from("<IIII", b, 8)
        if MOD_LO <= q0 < MOD_HI:
            s = space.cstring(q0)
            label = f'"{s}"' if s else f"fun@{q0:#x}"
        else:
            label = f"{q0:#x}"
        print(f"  [{i:2}] {label:<36} +{off:#06x}  type={typ:#x}", file=out)


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_v2b")))
    descs = read_descriptors(space)
    wanted = sys.argv[1:] or [d["name"] for d in descs]
    with open(R.out_name("domino_schema.txt"), "w") as out:
        print(f"{len(descs)} class descriptors", file=out)
        for d in descs:
            print(
                f"  {d['name']:<24} size={d['size']:#08x} count={d['count']:<3} arr={d['arr']:#x}",
                file=out,
            )
        for d in descs:
            if d["name"] in wanted:
                dump_class(space, d, out)
    print(f"{len(descs)} descriptors -> {R.out_name('domino_schema.txt')}")
    for d in descs:
        print(f"  {d['name']:<24} size={d['size']:#08x} count={d['count']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

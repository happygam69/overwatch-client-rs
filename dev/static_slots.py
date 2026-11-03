"""What a set of client functions read and call, by displacement.

Used to answer "which slot of a vtable reads the model instance's `+0x118`
collision list?" without a displacement census of the whole image: the candidate
functions are known (the vtable's slots), so this lists, per function, every
memory operand at a watched displacement and every call to a watched producer.

Usage:
    python dev/static_slots.py 0x142133010 0x1421334b0 ...
    python dev/static_slots.py --vt 0x143394fb0        # a vtable's slots

Watched displacements default to the model/mesh fields under study
(`+0x118` the collision list, `+0x08` its holder's mesh pointer, and the mesh
block's own `+0x08`/`+0x10`/`+0x18`/`+0x28` array slots); members of a
`ShapeAddToBody` family are listed by name.
"""

from __future__ import annotations

import struct
import sys

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
BASE = 0x140000000

WATCH_DISP = {
    0x118: "model+0x118 (collision list)",
    0x8: "+0x08",
    0x10: "+0x10",
    0x18: "+0x18",
    0x28: "+0x28",
    0x30: "+0x30",
    0x40: "+0x40",
    0x48: "+0x48",
    0x90: "+0x90",
    0x98: "+0x98",
    0xA0: "+0xa0",
    0xB0: "+0xb0",
    0xC0: "+0xc0",
    0xF0: "+0xf0",
}

PRODUCERS = {
    0x142CB3940: "ShapeAddToBody",
    0x142CB7F10: "ShapeEntryFromDescriptor",
    0x142CD4EF0: "ShapeAddToOwner",
    0x142CB00C0: "ShapePoolAlloc",
    0x142CC3D90: "OwnerCreateBody",
    0x142CF2010: "MeshToColliderFaces",
    0x142CF23C0: "BrushTreeToMesh",
    0x140C2813F: "MeshShapeFill(a)",
    0x142078562: "MeshShapeFill(b)",
    0x1420A02F6: "MeshShapeFill(c)",
    0x14210BAC0: "MeshShapeFill(d)",
    0x142CCC120: "ColliderMeshEmit",
    0x142CCA460: "ColliderFaceAppend",
    0x142CF0360: "BrushHullStage",
    0x141B53B70: "PairUnpack",
}


def pdata_ranges(exe: bytes) -> dict[int, int]:
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
    out: dict[int, int] = {}
    for i in range(len(raw) // 12):
        beg, end = struct.unpack_from("<II", raw, 12 * i)
        if end > beg:
            out[beg + BASE] = end + BASE
    return out


def main() -> int:
    img = open(IMAGE, "rb").read()
    exe = open(EXE, "rb").read()
    ranges = pdata_ranges(exe)
    starts: list[int] = []
    args = sys.argv[1:]
    if args and args[0] == "--vt":
        vt = int(args[1], 16)
        for k in range(24):
            v = struct.unpack_from("<Q", img, vt - BASE + 8 * k)[0]
            if not (0x140001000 <= v < 0x1433C4000):
                break
            starts.append(v)
    else:
        starts = [int(a, 16) for a in args]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    for start in starts:
        end = ranges.get(start, start + 0x400)
        hits: dict[int, int] = {}
        calls: list[tuple[int, str]] = []
        for ins in md.disasm(img[start - BASE:end - BASE], start):
            if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                t = int(ins.op_str, 16)
                if t in PRODUCERS:
                    calls.append((ins.address, PRODUCERS[t]))
            for op in ins.operands:
                if op.type == capstone.x86.X86_OP_MEM:
                    d = op.mem.disp
                    if d in WATCH_DISP:
                        hits[d] = hits.get(d, 0) + 1
        label = f"{start:#x}..{end:#x}"
        disps = " ".join(
            f"{WATCH_DISP[d]}x{n}" for d, n in sorted(hits.items())
        ) or "-"
        calls_s = " ".join(f"{n}@{a:#x}" for a, n in calls) or "-"
        print(f"{label}  reads: {disps}")
        print(f"{'':<24}  calls: {calls_s}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

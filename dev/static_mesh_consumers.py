"""Who builds a Domino shape, and does the builder read a stored mesh?

The model's prebuilt collision block (`STU_537E56E4`, reached through
`STUModel`'s `+0x118` list) can only become a collider through one of the known
ingest functions. So instead of a displacement census of the whole image, this
probe walks *from* those functions backwards: every `E8` caller of

    ShapeAddToBody   0x142cb3940     ShapeEntryFromDescriptor 0x142cb7f10
    ShapeAddToOwner  0x142cd4ef0     ShapePoolAlloc           0x142cb00c0
    OwnerCreateBody  0x142cc3d90     MeshShapeFill(a..d)      0x140c2813f,
    0x142078562, 0x1420a02f6, 0x14210bac0

and for each caller reports the containing `.pdata` function, whether that
function reads the model instance's `+0x118` collision list, and whether it reads
the mesh block's own array slots (`+0x08` nodes, `+0x18` points, `+0x28`
triangles, `+0x38` nodeCount, `+0x40` height).

Usage:
    python dev/static_mesh_consumers.py
"""

from __future__ import annotations

import struct

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
BASE = 0x140000000

INGEST = {
    0x142CB3940: "ShapeAddToBody",
    0x142CB7F10: "ShapeEntryFromDescriptor",
    0x142CD4EF0: "ShapeAddToOwner",
    0x142CB00C0: "ShapePoolAlloc",
    0x142CC3D90: "OwnerCreateBody",
    0x140C2813F: "MeshShapeFill(a)",
    0x142078562: "MeshShapeFill(b)",
    0x1420A02F6: "MeshShapeFill(c)",
    0x14210BAC0: "MeshShapeFill(d)",
}

# Mesh block fields, at the class's declared offsets (a v2 runtime instance is
# laid out at them, which is why GeometryBuild reads C+0x98 the same way).
MESH_FIELDS = {0x08: "nodes", 0x18: "points", 0x28: "triangles", 0x38: "nodeCount", 0x40: "height"}
LIST_FIELD = 0x118


def pdata_ranges(exe: bytes) -> list[tuple[int, int]]:
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


def enclosing(ranges: list[tuple[int, int]], va: int) -> tuple[int, int] | None:
    lo, hi = 0, len(ranges) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        beg, end = ranges[mid]
        if va < beg:
            hi = mid - 1
        elif va >= end:
            lo = mid + 1
        else:
            return beg, end
    return None


def calls_to(img: bytes, target: int) -> list[int]:
    out = []
    i = 0
    while True:
        i = img.find(b"\xe8", i)
        if i < 0 or i + 5 > len(img):
            break
        rel = struct.unpack_from("<i", img, i + 1)[0]
        if i + BASE + 5 + rel == target:
            out.append(i + BASE)
        i += 1
    return out


def main() -> int:
    img = open(IMAGE, "rb").read()
    ranges = pdata_ranges(open(EXE, "rb").read())
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True

    # Ingest function -> its callers' containing functions.
    callers: dict[int, set[tuple[int, int]]] = {}
    for target, name in INGEST.items():
        sites = calls_to(img, target)
        fns = set()
        for site in sites:
            rng = enclosing(ranges, site)
            if rng:
                fns.add(rng)
        callers[target] = fns
        print(f"{name} {target:#x}: {len(sites)} call site(s) in {len(fns)} function(s)")

    # For each caller function: does it read the list or the mesh fields?
    print()
    seen: set[tuple[int, int]] = set()
    for target, name in INGEST.items():
        for beg, end in sorted(callers[target]):
            if (beg, end) in seen:
                continue
            seen.add((beg, end))
            disps: dict[int, int] = {}
            calls: list[str] = []
            for ins in md.disasm(img[beg - BASE:end - BASE], beg):
                if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
                    t = int(ins.op_str, 16)
                    if t in INGEST:
                        calls.append(INGEST[t])
                for op in ins.operands:
                    if op.type == capstone.x86.X86_OP_MEM:
                        d = op.mem.disp
                        if d == LIST_FIELD or d in MESH_FIELDS:
                            disps[d] = disps.get(d, 0) + 1
            if not disps:
                continue
            body = " ".join(
                f"{'LIST' if d == LIST_FIELD else MESH_FIELDS[d]}x{n}"
                for d, n in sorted(disps.items())
            )
            print(f"{beg:#x}..{end:#x} calls {','.join(sorted(set(calls)))}  reads {body}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

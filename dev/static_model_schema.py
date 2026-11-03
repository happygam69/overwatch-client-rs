"""Decode a class's field table from its field-table installer function.

The client registers a class in two functions: one calls
`RegisterComponentTypeDescriptor` (`0x1421cd360`) with the class's hash, size and
hooks, and another registers each field through `0x1421d4f70` / `0x1421d4f30`
(the field-descriptor constructors), which store the field hash (`r8d`) at
`desc+0x30`, the type id (`r9d`) at `desc+0x34` and the **runtime offset** (the
caller's `[rsp+0x20]`) at `desc+0x08`.

So a function's call sites spell out the class's schema: hash, offset, type id,
name. This probe prints that table for a given installer and decodes the names
with the registry's own obfuscation rule (length in the low 24 bits of the dword
before the name, key = the eight bytes 0xC before it, `byte ^ key[i & 7]`).

Usage:
    python dev/static_model_schema.py 0x14150b78c
"""

from __future__ import annotations

import struct
import sys

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
BASE = 0x140000000
FIELD_CTORS = (0x1421D4F70, 0x1421D4F30)


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


def decode_name(img: bytes, at: int) -> str:
    """The registry's obfuscated string: length/key before the bytes."""
    hdr = at - 0xC
    if hdr < 0 or at > len(img):
        return ""
    word = struct.unpack_from("<I", img, at - 4)[0]
    length = word & 0xFFFFFF
    encoded = (word >> 24) & 0xFF
    if length == 0 or length > 96 or at + length > len(img):
        return ""
    key = img[hdr:hdr + 8]
    out = bytearray(img[at:at + length])
    if encoded:
        for i in range(len(out)):
            out[i] ^= key[i & 7]
    # The decoder leaves the first bytes as a length header in some builds; keep
    # whatever is printable.
    return bytes(out).decode("latin-1", "replace")


def main() -> int:
    img = open(IMAGE, "rb").read()
    ranges = pdata_ranges(open(EXE, "rb").read())
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    start = int(sys.argv[1], 16)
    rng = enclosing(ranges, start)
    end = rng[1] if rng else start + 0x1000
    print(f"installer {start:#x}..{end:#x} ({end - start:#x} bytes)")

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    hash_ = typ = off = name = desc = None
    rows = []
    for ins in md.disasm(img[start - BASE:end - BASE], start):
        ops = ins.operands
        if ins.mnemonic == "mov" and len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG:
            reg = ins.reg_name(ops[0].reg) or ""
            if reg.startswith("r8") and ops[1].type == capstone.x86.X86_OP_IMM:
                hash_ = ops[1].imm & 0xFFFFFFFF
            elif reg.startswith("r9") and ops[1].type == capstone.x86.X86_OP_IMM:
                typ = ops[1].imm & 0xFFFFFFFF
        if (
            ins.mnemonic == "mov"
            and len(ops) == 2
            and ops[0].type == capstone.x86.X86_OP_MEM
            and (ins.reg_name(ops[0].mem.base) or "") == "rsp"
            and ops[0].mem.disp == 0x20
            and ops[1].type == capstone.x86.X86_OP_IMM
        ):
            off = ops[1].imm & 0xFFFFFFFF
        if (
            ins.mnemonic == "lea"
            and len(ops) == 2
            and ops[1].type == capstone.x86.X86_OP_MEM
            and (ins.reg_name(ops[1].mem.base) or "") == "rip"
        ):
            t = ins.address + ins.size + ops[1].mem.disp
            reg = ins.reg_name(ops[0].reg) or ""
            if reg == "rdx":
                name = t
            elif reg == "rcx":
                desc = t
        if ins.mnemonic == "call" and ins.op_str.startswith("0x"):
            t = int(ins.op_str, 16)
            if t in FIELD_CTORS:
                rows.append((hash_, off, typ, name, desc, ins.address, t))
    print(f"{len(rows)} field registration(s)")
    print(f"{'hash':<10} {'offset':<8} {'type':<10} {'name':<34} ctor  at")
    for h, o, ty, n, d, at, ctor in rows:
        nm = decode_name(img, n) if n else ""
        print(
            f"{h if h is None else hex(h):<10} "
            f"{'-' if o is None else hex(o):<8} "
            f"{'-' if ty is None else hex(ty):<10} {nm:<34} {ctor:#x} @{at:#x}"
        )
    print("class descriptor(s) seen:", sorted({hex(d) for *_, d, _a, _c in rows if d}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

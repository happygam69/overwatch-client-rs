"""Enumerate the client's registered class descriptors by their call sites.

`RegisterComponentTypeDescriptor` (`0x1421cd360`) is called once per registered
class from a generated registration function. The call carries the descriptor's
BSS address (RCX), the type hash (R8D), the instance size (arg 5 at [rsp+0x20]),
the alignment (arg 6 at [rsp+0x28]) and the constructor (arg 17 at [rsp+0x80]).
This probe recovers those five values for every call site in the image, so a
class can be looked up by size or by hash instead of by a displacement census.

Usage:
    python dev/model_registry.py                 # every class, size desc
    python dev/model_registry.py --min 0x190     # only classes at least N bytes
    python dev/model_registry.py --hash 0x9199bf6b
    python dev/model_registry.py --desc 0x143c3f060

The image is read-only input; only conclusions (addresses and values) belong in
the docs, never the listing.
"""

from __future__ import annotations

import argparse
import struct

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
BASE = 0x140000000
REGISTER = 0x1421CD360


def load() -> bytes:
    with open(IMAGE, "rb") as f:
        return f.read()


def pdata_ranges(exe_path: str) -> list[tuple[int, int]]:
    """(begin, end) image VAs of every .pdata function range."""
    exe = open(exe_path, "rb").read()
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    pdata = None
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, o + 8)
        if name == ".pdata":
            pdata = (ro, max(vsz, rsz))
    raw = exe[pdata[0]:pdata[0] + pdata[1]]
    out = []
    for i in range(len(raw) // 12):
        beg, end = struct.unpack_from("<II", raw, 12 * i)
        out.append((beg + BASE, end + BASE))
    return out


def calls_to(img: bytes, target: int) -> list[int]:
    """Addresses of `E8 rel32` whose target is `target`."""
    hits = []
    i = 0
    while True:
        i = img.find(b"\xe8", i)
        if i < 0 or i + 5 > len(img):
            break
        va = i + BASE
        rel = struct.unpack_from("<i", img, i + 1)[0]
        if (va + 5 + rel) & 0xFFFFFFFFFFFFFFFF == target:
            hits.append(va)
        i += 1
    return hits


def enclosing(ranges: list[tuple[int, int]], va: int) -> tuple[int, int] | None:
    for beg, end in ranges:
        if beg <= va < end:
            return beg, end
    return None


def rip_target(md, insn, img: bytes) -> int | None:
    for op in insn.operands:
        if op.type == capstone.x86.X86_OP_MEM and op.mem.base == capstone.x86.X86_REG_RIP:
            return insn.address + insn.size + op.mem.disp
    return None


def decode_args(img: bytes, start: int, stop: int) -> dict:
    """Walk one registration function and collect the call's arguments."""
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    found: dict = {}
    leas: dict[str, int] = {}
    code = img[start - BASE:stop - BASE]
    for insn in md.disasm(code, start):
        ops = insn.operands
        if len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_MEM \
                and ops[0].mem.base == capstone.x86.X86_REG_RSP \
                and ops[1].type in (capstone.x86.X86_OP_IMM, capstone.x86.X86_OP_REG):
            slot = ops[0].mem.disp
            if 0x20 <= slot <= 0x88:
                if ops[1].type == capstone.x86.X86_OP_IMM:
                    found[slot] = ("imm", ops[1].imm)
                else:
                    name = insn.reg_name(ops[1].reg) or ""
                    if name in leas:
                        found[slot] = ("lea", leas[name])
                    else:
                        found[slot] = ("reg", insn.address)
        if len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG \
                and (insn.reg_name(ops[0].reg) or "").startswith("r8") \
                and ops[1].type == capstone.x86.X86_OP_IMM:
            found["hash"] = ops[1].imm & 0xFFFFFFFF
        if len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG \
                and (insn.reg_name(ops[0].reg) or "") == "rcx":
            if ops[1].type == capstone.x86.X86_OP_MEM:
                t = rip_target(md, insn, img)
                if t is not None:
                    found["desc"] = t
            elif ops[1].type == capstone.x86.X86_OP_IMM:
                found["desc"] = ops[1].imm
        if len(ops) == 2 and ops[0].type == capstone.x86.X86_OP_REG \
                and ops[1].type == capstone.x86.X86_OP_MEM:
            t = rip_target(md, insn, img)
            if t is not None:
                leas[insn.reg_name(ops[0].reg) or ""] = t
    return found


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--min", default=None)
    ap.add_argument("--hash", default=None)
    ap.add_argument("--desc", default=None)
    ap.add_argument("--exe", default=EXE)
    args = ap.parse_args()

    img = load()
    ranges = pdata_ranges(args.exe)
    hits = calls_to(img, REGISTER)
    print(f"{len(hits)} call sites of RegisterComponentTypeDescriptor")

    rows = []
    for call in hits:
        rng = enclosing(ranges, call)
        start = rng[0] if rng else call - 0x200
        got = decode_args(img, start, call + 5)

        def val(slot, kind=None):
            v = got.get(slot)
            if not v:
                return None
            if kind is None or v[0] == kind:
                return v[1]
            return None

        rows.append({
            "call": call,
            "fn": start,
            "desc": got.get("desc"),
            "hash": got.get("hash"),
            "size": val(0x20),
            "align": val(0x28),
            "a7": val(0x30),
            "a9": val(0x40),
            "ctor": val(0x80, "lea"),
            "slots": {hex(s): v[1] for s, v in got.items()
                      if isinstance(s, int) and s >= 0x40 and v[0] == "lea"},
        })

    if args.hash:
        want = int(args.hash, 16)
        rows = [r for r in rows if r["hash"] == want]
    if args.desc:
        want = int(args.desc, 16)
        rows = [r for r in rows if r["desc"] == want]
    if args.min:
        low = int(args.min, 16)
        rows = [r for r in rows if isinstance(r["size"], int) and r["size"] >= low]
        rows.sort(key=lambda r: -r["size"])

    for r in rows:
        size = r["size"]
        print(f"size {size:#08x}  hash {r['hash']:#010x}  desc {r['desc']:#x}  "
              f"ctor {r['ctor'] and hex(r['ctor'])}  align {r['align']}  fn {r['fn']:#x}")
        if r["slots"]:
            print("            slots " + " ".join(f"{k}={hex(v)}" for k, v in r["slots"].items()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

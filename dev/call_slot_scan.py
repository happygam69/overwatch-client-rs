"""Find every `call qword ptr [reg + disp]` site for a set of slot offsets.

    python dev/call_slot_scan.py 0x40 [0x100 ...] [--ctx N]

Why this exists: a virtual method that no `E8` reaches is dispatched through a
vtable slot, and the *only* way to say "nobody calls it" is to count the sites
that could. Two encodings carry a memory-indirect call and they are not
interchangeable:

  * `FF /2` with mod=01 carries a **disp8** (`FF 50 40` == call [rax+0x40]);
  * `FF /2` with mod=10 carries a **disp32** (`FF 90 00 01 00 00`).

A slot below 0x80 is therefore *invisible* to a disp32 scan -- this image has
zero `FF 9x 40 00 00 00` bytes and thousands of `FF 5x 40` bytes, so a disp32-only
search silently reports "no caller" for every early slot. This tool scans both
forms, re-decodes each candidate with capstone so a coincidental byte triple is
rejected, prints the few instructions before the call, and resolves the hit to
its `.pdata` chain root (a fragment start is not a function start).

Read-only: reads `dev/data/offline_recover/module_offline.bin`
(file offset == RVA, image base 0x140000000).
"""

from __future__ import annotations

import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import capstone

import fn_extent as fe

IMG = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO = 0x1000
TEXT_HI = 0x2F2C3D3

# FF /2 = call r/m64. mod=01 -> disp8, mod=10 -> disp32.
REX = (b"", b"\x41", b"\x48", b"\x49")


def _confirm(md: capstone.Cs, blob: bytes, site: int) -> capstone.CsInsn | None:
    """Re-decode from up to 7 bytes back so `site` is the call instruction."""
    for back in range(0, 8):
        start = site - back
        code = blob[start - BASE: site - BASE + 6]
        ins = next(md.disasm(code, start, 1), None)
        if ins is None or ins.address != site:
            continue
        if ins.mnemonic == "call" and ins.op_str.startswith("qword ptr [r"):
            return ins
    return None


def noreturn_root(blob: bytes, ents, addr: int) -> int:
    k, _chain = fe.root_of(blob, ents, addr - BASE)
    if k is None:
        return addr
    return BASE + ents[k][0]


def main() -> int:
    args = sys.argv[1:]
    ctx = 4
    if "--ctx" in args:
        k = args.index("--ctx")
        ctx = int(args[k + 1])
        del args[k: k + 2]
    disps = sorted({int(a, 16) for a in args})
    blob = open(IMG, "rb").read()
    ents = fe.entries(blob)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)

    for disp in disps:
        forms = []
        if disp < 0x80:
            forms.append(("disp8", struct.pack("<b", disp), 2))
        forms.append(("disp32", struct.pack("<i", disp), 5))
        print(f"== slot +{disp:#x}")
        total = 0
        for name, tail, tlen in forms:
            raw = 0
            kept = []
            for rex in REX:
                for reg in range(8):
                    if reg == 4 and not rex:
                        continue  # [rsp+disp] needs a SIB: not a vtable slot
                    modrm = (0x50 if name == "disp8" else 0x90) | reg
                    pat = rex + bytes([0xFF, modrm]) + tail
                    pos = TEXT_LO
                    while True:
                        i = blob.find(pat, pos, TEXT_HI)
                        if i < 0:
                            break
                        raw += 1
                        pos = i + 1
                        site = BASE + i + len(rex)
                        ins = _confirm(md, blob, site)
                        if ins is not None:
                            kept.append((site, ins.mnemonic + " " + ins.op_str))
            print(f"   {name}: {raw} raw byte pattern(s), {len(kept)} confirmed call(s)")
            for site, text in kept:
                root = noreturn_root(blob, ents, site)
                print(f"      {site:#x}  in {root:#x}   {text}")
                back = blob[site - BASE - 12 * ctx: site - BASE]
                # a coarse walk: print the last `ctx` decodable instructions
                ins_list = list(md.disasm(back, site - 12 * ctx))
                for x in ins_list[-ctx:]:
                    print(f"          {x.address:#x}  {x.mnemonic:8} {x.op_str}")
            total += len(kept)
        print(f"   total confirmed sites: {total}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

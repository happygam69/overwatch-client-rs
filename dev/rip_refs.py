"""Find RIP-relative references (lea/mov/cmp, any opcode) to a set of addresses.

    python dev/rip_refs.py 0x142f7d678 0x142f7d320 [--window N]

Why not a full disassembly: the image is 50 MB of code and the obfuscated
regions make linear disassembly unreliable, so this scans every 4-byte position
`p` in `.text` that could be the displacement field of a RIP-relative operand
and solves `target = p + 4 + disp32`. Every hit is then re-decoded with capstone
from `p-3 .. p` so a false positive (a coincidental 4-byte value) is rejected.

Reports each hit as `insn-address  mnemonic operands`, and with `--window`
prints that many bytes of surrounding instructions so the reader sees whether
the address is loaded into a register, stored to an object, or compared.

Read-only against `dev/data/offline_recover/module_offline.bin`.
"""

from __future__ import annotations

import struct
import sys

import numpy as np

IMG = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO = 0x1000
TEXT_HI = 0x2F2C3D3


def main() -> int:
    args = sys.argv[1:]
    window = 0
    if "--window" in args:
        k = args.index("--window")
        window = int(args[k + 1])
        del args[k:k + 2]
    targets = {int(a, 16) for a in args}

    data = np.fromfile(IMG, dtype=np.uint8)
    seg = data[TEXT_LO:TEXT_HI]
    n = len(seg) - 4
    b = np.lib.stride_tricks.as_strided(seg, shape=(n, 4), strides=(1, 1))
    disp = (b[:, 0].astype(np.uint32)
            | (b[:, 1].astype(np.uint32) << 8)
            | (b[:, 2].astype(np.uint32) << 16)
            | (b[:, 3].astype(np.uint32) << 24)).view(np.int32)
    idx = np.arange(n, dtype=np.int64)
    insn_end = BASE + TEXT_LO + idx + 4
    cand = insn_end + disp.astype(np.int64)
    hits = []
    for t in targets:
        for j in np.nonzero(cand == t)[0]:
            hits.append((int(BASE + TEXT_LO + j), t))
    hits.sort()

    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    raw = open(IMG, "rb").read()
    confirmed = 0
    for p, t in hits:
        # the displacement field covers p..p+3, so the instruction ends at p+4
        for back in range(1, 8):
            start = p - back
            code = raw[start - BASE: p + 4 - BASE]
            ins = next(md.disasm(code, start, 1), None)
            if ins is None or ins.address + ins.size != p + 4:
                continue
            rip = any(o.type == capstone.x86.X86_OP_MEM
                      and o.mem.base == capstone.x86.X86_REG_RIP
                      for o in ins.operands)
            if not rip:
                continue
            print(f"{ins.address:#x}  -> {t:#x}   {ins.mnemonic} {ins.op_str}")
            confirmed += 1
            if window:
                lo = ins.address - window
                hi = ins.address + ins.size + window
                for x in md.disasm(raw[lo - BASE:hi - BASE], lo):
                    mark = " >>" if x.address == ins.address else "   "
                    print(f"{mark} {x.address:#x}  {x.mnemonic:8} {x.op_str}")
            break
    print(f"{confirmed} confirmed rip-relative reference(s) to {len(targets)} target(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

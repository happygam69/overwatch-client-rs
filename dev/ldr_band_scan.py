"""Targeted disassembly of the loader's cold `.text` band, from disk.

The `.pdata`-guided scan found 1,806 real call/jmp edges from ordinary
functions into the loader's 342 protected `.text` pages (zero in every
captured session, yet full code on disk), and zero `.eid` references from
ordinary code.  If the engine that decrypts the game module's pages lives
in that band, its references would be visible here: the band pages are
read straight from the on-disk file (plaintext, no capture needed).

This script disassembles every cold-band page from the on-disk file and
reports:
  - references into the `.eid` rva range 0x1f80000..0x2410000,
  - whether band functions carry `.pdata` entries (compiled) or not,
  - caller-function starts per cold page and xref counts, samples listed.

Usage:
    python ldr_band_scan.py [--path PATH] [--warm PATH]

Read-only; prints only.
"""
from __future__ import annotations

import argparse
import struct
from collections import Counter

import capstone
import numpy as np

EID_LO = 0x1F80000
EID_HI = 0x1F80000 + 0x490000
TEXT_LO = 0x10000
TEXT_HI = 0x1D70000
DEFAULT_LDR = r"E:\Overwatch\_retail_\Overwatch_loader.dll"
DEFAULT_WARM = r"D:\ow-dumps\loader_after_diff.bin"


def sections(path: str):
    d = open(path, "rb").read()
    pe = d.find(b"PE\0\0")
    nsec = struct.unpack_from("<H", d, pe + 6)[0]
    optsz = struct.unpack_from("<H", d, pe + 4 + 16)[0]
    secoff = pe + 4 + 16 + 4 + optsz
    secs = []
    for i in range(nsec):
        o = secoff + i * 40
        name = d[o:o + 8].rstrip(b"\0").decode("latin1")
        vsz, va, rsz, roff = struct.unpack_from("<IIII", d, o + 8)
        secs.append((name, va, vsz, roff, rsz))
    return d, secs


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--path", default=DEFAULT_LDR)
    ap.add_argument("--warm", default=DEFAULT_WARM)
    args = ap.parse_args()
    d, secs = sections(args.path)
    warm = open(args.warm, "rb").read()
    b = np.frombuffer(warm[TEXT_LO:TEXT_HI], np.uint8).reshape(-1, 4096)
    cold = sorted(int(i) for i in np.flatnonzero((b == 0).all(axis=1)))
    print("cold pages:", len(cold))

    start_pgs = set()
    for name, va, vsz, roff, rsz in secs:
        if name == ".pdata":
            cnt = min(rsz, vsz) // 12
            for i in range(cnt):
                beg, end, _ = struct.unpack_from("<III", d, roff + i * 12)
                if TEXT_LO <= beg < TEXT_HI:
                    start_pgs.add((beg >> 12) << 12)
    n_eid = 0
    eid_samples = []
    pages_with_pdata = 0
    pages_no_pdata = 0
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    md.skipdata = True
    ninsn = 0
    for pgi in cold:
        rva = TEXT_LO + pgi * 4096
        if rva in start_pgs:
            pages_with_pdata += 1
        else:
            pages_no_pdata += 1
        fo = None
        for name, va, vsz, roff, rsz in secs:
            if va <= rva < va + rsz:
                fo = roff + (rva - va)
                break
        if fo is None:
            continue
        page = d[fo:fo + 4096]
        for insn in md.disasm(page, rva):
            if insn.mnemonic == ".byte":   # skipdata pseudo-instruction
                continue
            ninsn += 1
            for op in insn.operands:
                tgt = None
                if op.type == capstone.x86.X86_OP_MEM and \
                        op.mem.base == capstone.x86.X86_REG_RIP and \
                        op.mem.index == 0:
                    tgt = op.mem.disp + insn.address + insn.size
                elif op.type == capstone.x86.X86_OP_IMM:
                    tgt = op.imm
                else:
                    continue
                if tgt is not None and EID_LO <= tgt < EID_HI:
                    n_eid += 1
                    if len(eid_samples) < 40:
                        eid_samples.append((insn.address, tgt,
                                            insn.mnemonic,
                                            insn.op_str[:64]))
    print(f"pages with a .pdata-start: {pages_with_pdata}, "
          f"without: {pages_no_pdata}")
    print("instructions from band pages:", ninsn)
    print("eid references from band:", n_eid)
    for s in eid_samples[:30]:
        print(f"   0x{s[0]:08x} -> 0x{s[1]:06x}  {s[2]} {s[3]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

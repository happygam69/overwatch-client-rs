""".pdata-guided static xref scan over the plaintext loader code.

The linear skipdata sweep produced false decodes (data bytes read as
instructions), so this version disassembles only at real instruction
starts: every entry in the loader's `.pdata` runtime-function table
(RVAs, ~5,500 entries) is a true 64-bit function start.  Per function it
collects:
  - RIP-relative entities and call/jmp imm targets into the `.eid`
    store (va 0x1f80000..0x2410000),
  - call/jmp edges into the protected `.text` pages (loader pages that
    were never readable in any captured session - the cold set),
  - the instruction image around those xrefs is printed for samples.

Falsification instrument: it should agree with a Ghidra pass; it exists
so the hunt can proceed while the GUI's auto-analysis queue is busy.

Usage:
    python ldr_xref_scan.py [--path PATH] [--warm PATH]

Read-only; prints only.
"""
from __future__ import annotations

import argparse
import struct
from collections import Counter

import capstone
import numpy as np

IMAGE_BASE = 0x180000000
EID_LO = 0x1F80000
EID_HI = 0x1F80000 + 0x490000
TEXT_LO = 0x10000
TEXT_HI = 0x1D70000
PROT_RVA = 0x1F00000         # .pdata section RVA
PROT_RAW = 0x1E8F200         # .pdata raw offset
PROT_RAW_SZ = 0x10C00
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


def function_starts() -> list:
    ld = open(DEFAULT_LDR, "rb").read()
    pdb = ld[PROT_RVA - 0x1F00000 + 0x1E8F200:None]  # placeholder, replaced
    return []


def entry_functions(sections, ld: bytes) -> list:
    for name, va, vsz, roff, rsz in sections:
        if name == ".pdata":
            rawsz_bytes = min(rsz, vsz)
            cnt = rawsz_bytes // 12
            starts = []
            for i in range(cnt):
                beg, end, _uw = struct.unpack_from("<III", ld, roff + i * 12)
                if beg and TEXT_LO <= beg < TEXT_HI:
                    starts.append((beg, end))
            return starts
    return []


def protected_pages(warm_path: str) -> set:
    warm = open(warm_path, "rb").read()
    b = np.frombuffer(warm[TEXT_LO:TEXT_HI], np.uint8).reshape(-1, 4096)
    return set(TEXT_LO + i * 4096 for i in np.flatnonzero((b == 0).all(axis=1)))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--path", default=DEFAULT_LDR)
    ap.add_argument("--warm", default=DEFAULT_WARM)
    args = ap.parse_args()

    d, secs = sections(args.path)
    prot = protected_pages(args.warm)
    print(f"protected (zeroed-in-warm) .text pages: {len(prot)}")
    mfns = entry_functions(secs, d)
    print(f".pdata function entries: {len(mfns)}")

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True

    eid_pages = Counter()
    eid_samples = []
    call_edges = Counter()     # (src 4K, tgt 4K) into protected-band pages
    fn_bytes: dict = {}
    nfn = 0
    ninsn = 0
    for beg, end in mfns:
        fo, _ = None, None
        for name, va, vsz, roff, rsz in secs:
            if va <= beg < va + rsz:
                fo = roff + (beg - va)
                break
        if fo is None:
            continue
        take = min(end - beg if end >= beg else 0x1000, 0x40000)
        blob = d[fo:fo + take]
        if not blob:
            continue
        nfn += 1
        fn_bytes[beg] = len(blob)
        for insn in md.disasm(blob, beg):
            ninsn += 1
            is_call = insn.mnemonic in ("call", "jmp")
            for op in insn.operands:
                tgt = None
                if op.type == capstone.x86.X86_OP_MEM and \
                        op.mem.base == capstone.x86.X86_REG_RIP and \
                        op.mem.index == 0:
                    tgt = op.mem.disp + insn.address + insn.size
                elif op.type == capstone.x86.X86_OP_IMM and is_call:
                    tgt = op.imm
                else:
                    continue
                if not isinstance(tgt, int):
                    continue
                if EID_LO <= tgt < EID_HI:
                    pg = (tgt >> 12) << 12
                    eid_pages[pg] += 1
                    if len(eid_samples) < 40:
                        eid_samples.append((insn.address, tgt, insn.mnemonic,
                                            insn.op_str[:64]))
                elif is_call and (tgt >> 12) << 12 in \
                        set((p >> 12) << 12 for p in prot):
                    call_edges[((insn.address >> 12) << 12,
                                (tgt >> 12) << 12)] += 1
    print(f"functions {nfn}, instructions {ninsn}")
    print("distinct .eid target 4KB pages:", len(eid_pages))
    for pg, c in eid_pages.most_common(15):
        print(f"   eid 0x{pg:06x}  refs {c}")
    print("eid ref samples:")
    for addr, tgt, mn, ops in eid_samples[:30]:
        print(f"   0x{addr:08x} -> 0x{tgt:06x}  {mn} {ops}")
    print("call/jmp edges into protected-band pages:", len(call_edges))
    srcs = Counter(s for s, _t in call_edges)
    tgts = Counter(t for _s, t in call_edges)
    print("hot source pages:", [hex(s) for s, _c in srcs.most_common(12)])
    print("hot target pages:", [hex(t) for t, _c in tgts.most_common(12)])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

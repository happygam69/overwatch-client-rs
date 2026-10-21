"""Find the physics tick object in a captured heap set.

The tick object (0xf10 bytes) is identified by its step-context pointer:
`[A+0x40]` points at a context with the position/velocity iteration counts
(`+8 == 8`, `+0xc == 3`) and a world handle (`u16 +0x58 < 0x3fff`).  The scan
runs in two passes over the heap regions of a capture: collect context
addresses, then look for pointers to them and print the candidate object's
fields (+0x1b4 mode, +0xf08 flags, +0xef8/+0xefc step counters, +0x268/+0x280/
+0x2a0 list fields, +8 owner).

Usage:
    OW_DUMPS=dumps_v2b python find_tick.py [--max-mb N]
"""

from __future__ import annotations

import os
import struct
import sys

import numpy as np

import regions as R

MOD_LO, MOD_HI = 0x7FF7C4D20000, 0x7FF7C9000000


def main() -> int:
    max_mb = 6000
    if "--max-mb" in sys.argv:
        max_mb = int(sys.argv[sys.argv.index("--max-mb") + 1])
    vs = R.Space(R.load(R.capture_dir(os.environ.get("OW_DUMPS", "dumps_v2b"))))
    heaps = [r for r in vs.regions
             if not (MOD_LO <= r.base < MOD_HI) and (1 << 20) <= r.size <= (256 << 20)]
    total = sum(r.size for r in heaps)
    print(f"{len(heaps)} heap regions, {total/1048576:.0f} MB", flush=True)

    # pass 1: context addresses
    ctxs = []
    scanned = 0
    for r in heaps:
        if scanned > max_mb << 20:
            print(f"  (pass 1 stopped at {scanned/1048576:.0f} MB)", flush=True)
            break
        with open(r.path, "rb") as fh:
            b = fh.read()
        n32 = len(b) // 4
        u32 = np.frombuffer(b[:n32*4], dtype=np.uint32)
        if n32 < 0x100:
            scanned += r.size
            continue
        # A = base + 8j ; offsets 8 and 0xc and 0x58
        d8 = u32[2::2]
        d12 = u32[3::2]
        w58 = u32[0x16::2] & 0xffff
        m = min(len(d8), len(d12), len(w58))
        sel = np.nonzero((d8[:m] == 8) & (d12[:m] == 3) & (w58[:m] < 0x3fff))[0]
        for j in sel:
            ctxs.append(r.base + 8 * int(j))
        scanned += r.size
        if len(ctxs) and len(ctxs) % 200 == 0:
            print(f"  ... {len(ctxs)} contexts after {scanned/1048576:.0f} MB", flush=True)
    print(f"pass 1: {len(ctxs)} context candidates over {scanned/1048576:.0f} MB", flush=True)
    if not ctxs:
        return 0
    ctx_arr = np.array(sorted(set(ctxs)), dtype=np.uint64)

    # pass 2: pointers to a context; A = pointer - 0x40
    found = 0
    scanned = 0
    for r in heaps:
        if scanned > max_mb << 20:
            break
        with open(r.path, "rb") as fh:
            b = fh.read()
        n64 = len(b) // 8
        if n64 < 0x200:
            scanned += r.size
            continue
        q = np.frombuffer(b[:n64*8], dtype=np.uint64)
        hit = np.isin(q, ctx_arr)
        for i in np.nonzero(hit)[0]:
            A = r.base + 8 * int(i) - 0x40
            probe = vs.read(A, 0xf10)
            if not probe or len(probe) < 0xf10:
                continue
            import struct as _s
            if _s.unpack_from("<I", probe, 0x1b4)[0] not in (0, 1):
                continue
            found += 1
            print(f"\nTICK CANDIDATE A={A:#x} (region {r.base:#x})", flush=True)
            data = vs.read(A, 0xf10)
            if not data or len(data) < 0xf10:
                print("   (object not fully readable)", flush=True)
                continue
            def qq(off): return struct.unpack_from("<Q", data, off)[0]
            def dd(off): return struct.unpack_from("<I", data, off)[0]
            print(f"   +0x008 owner  {qq(8):#018x}")
            print(f"   +0x040 ctx    {qq(0x40):#018x}")
            print(f"   +0x128 field  {qq(0x128):#018x}")
            print(f"   +0x1b4 mode   {dd(0x1b4)}")
            print(f"   +0x268 back   {qq(0x268):#018x}")
            print(f"   +0x280 flags  {qq(0x280):#018x}")
            print(f"   +0x2a0 next   {qq(0x2a0):#018x}")
            print(f"   +0x2a8 ?      {qq(0x2a8):#018x}")
            print(f"   +0xef8 pair0  {dd(0xef8)}   +0xefc pair1 {dd(0xefc)}   +0xf08 flags8 {data[0xf08]:#x}")
            ctx = qq(0x40)
            cd = vs.read(ctx, 0x60)
            if cd and len(cd) >= 0x60:
                print(f"   ctx+0x00 {struct.unpack_from('<Q',cd,0)[0]:#018x}"
                      f"  +0x08 {struct.unpack_from('<I',cd,8)[0]}"
                      f"  +0x0c {struct.unpack_from('<I',cd,0xc)[0]}"
                      f"  +0x58 handle {struct.unpack_from('<H',cd,0x58)[0]}")
        scanned += r.size
    print(f"\npass 2: {found} candidate(s) over {scanned/1048576:.0f} MB", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

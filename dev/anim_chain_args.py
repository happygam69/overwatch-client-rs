"""Collect the animation-chain function list (main module) for headless.

.pdata at rva 0x4635000 (main exe section table), bodies [beg, end),
warm-body check, spanning -0x10000 for the stripped headless import.
Prints found/missing; writes D:/ow-dumps/anim_args_s.txt.
"""
import bisect
import struct

import numpy as np

BASE = 0x7ff7c4d20000
MINUS = 0x10000
a = np.frombuffer(open(r"D:\ow-dumps\module_after_diff.bin", "rb").read(),
                  np.uint8)

PD_RVA, PD_SZ = 0x4635000, 0x24dd1c
n = PD_SZ // 12
pdata = []
for i in range(n):
    beg, end, un = struct.unpack_from("<III", a, PD_RVA + 12 * i)
    if beg and end > beg:
        pdata.append((beg, end))
pdata.sort()
starts = [p[0] for p in pdata]
seen = {}
for p in pdata:
    seen.setdefault(p[0], p[1])

def body_for(start):
    e = seen.get(start)
    if e and e - start < 0x40000:
        return start, e
    lo = bisect.bisect_left(starts, start)
    if lo < len(starts) and pdata[lo][0] > start:
        return start, pdata[lo][0]
    return None

def q(rva):
    return struct.unpack_from("<Q", a, rva)[0]

targets = []

def add(rva):
    if 0x10000 < rva < 0x2f95913:
        targets.append(rva)

# six class-descriptor vtables (+0x160 apart), slots jump between them
for k in range(6):
    vt = 0x4081848 + k * 0x160
    row = [hex(q(vt + 8 * j)) for j in range(4)]
    print(f"vtable {k} @ {vt:#x}: {row}")
    for j in range(4):
        add(q(vt + 8 * j) - BASE)
# the shared runtime cells
for cell in (0x4081500, 0x40817c0, 0x4081920, 0x4081660):
    for j in range(2):
        add(q(cell + 8 * j) - BASE)
# animation.md-named blend-tree / sampler / pose functions
for name in ("7ff7c6e84610", "7ff7c6e84900", "7ff7c6e8a400", "7ff7c6e8bcb0",
             "7ff7c6e93450", "7ff7c6eb6460", "7ff7c6bf7b30", "7ff7c6e85f40",
             "7ff7c6e85b10", "7ff7c6e85c50", "7ff7c6e85ab0", "7ff7c6e7f1e0",
             "7ff7c6ba3a70", "7ff7c6b67bc0", "7ff7c6e30d00", "7ff7c6e2bd80",
             "7ff7c6e30e00", "7ff7c6e2a570", "7ff7c6e2b440", "7ff7c6bf6f20",
             "7ff7c6bf69a0", "7ff7c6bf7220", "7ff7c6e850e0", "7ff7c6e85720"):
    add(int(name, 16) - BASE)

out = []
for t in sorted(set(targets)):
    body = body_for(t)
    if body and all(a[p * 4096:(p + 1) * 4096].any()
                    for p in range(body[0] >> 12, ((body[1] - 1) >> 12) + 1)):
        out.append(hex(body[0] - MINUS) + " " + hex(body[1] - MINUS))
        print(f"fn {t:#x}-{body[1]:#x} (size {body[1]-body[0]})")
    else:
        print(f"fn {t:#x}: no warm body -> skipped")
open(r"D:\ow-dumps\anim_args_s.txt", "w").write(" ".join(out))
print("pairs:", len(out))

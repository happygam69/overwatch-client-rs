"""Parse DISK loader: relocations (.reloc) -> which .data/.rdata slots are
pre-patched absolute pointers; export directory -> exported names/functions.

Usage: python dev/ldr_reloc.py
"""
import struct
from collections import defaultdict

DISK = r"D:\ow-dumps\loader_disk_rva.bin"
b = open(DISK, "rb").read()
assert b[:2] == b"MZ"
pe = struct.unpack_from("<I", b, 0x3C)[0]
nsec = struct.unpack_from("<H", b, pe + 6)[0]
opt = struct.unpack_from("<H", b, pe + 20)[0]
s0 = pe + 24 + opt
secs = []
for i in range(nsec):
    o = s0 + 40 * i
    name = b[o:o + 8].split(b"\x00")[0].decode("latin-1")
    vsz, va, rsz, ro = struct.unpack_from("<IIII", b, o + 8)
    secs.append((name, va, vsz, ro, rsz))


def rva2off(rva):
    # flat image: offset == rva
    return rva


def sec_of(rva):
    for name, va, vsz, ro, rsz in secs:
        if va <= rva < va + max(vsz, rsz):
            return name
    return "??"


BASE = 0x180000000
RELOC_RVA = 0x1F70000
ro = rva2off(RELOC_RVA)
rva = RELOC_RVA
total = defaultdict(int)
targets = defaultdict(list)
while True:
    page, size = struct.unpack_from("<II", b, ro + (rva - RELOC_RVA))
    if size == 0:
        break
    basepage = page
    n = size // 2
    for i in range(1, n):
        e = struct.unpack_from("<H", b, ro + (rva - RELOC_RVA) + 2 * i)[0]
        typ = e >> 12
        off = e & 0xFFF
        if typ == 0:
            continue
        trva = basepage + off
        sec = sec_of(trva)
        total[sec] += 1
        targets[sec].append(trva)
    rva += size
print("relocation entries per section:", dict(total))
for sec in (".data", ".rdata", "??"):
    lst = sorted(set(targets[sec]))
    print(f"\n{sec}: {len(lst)} distinct relocated slots")
    for t in lst[:80]:
        # read the stored qword (byte-quoted relative to preferred base)
        v = struct.unpack_from("<Q", b, rva2off(t))[0]
        tgt = v - BASE + t if 0 else None
        print(f"  {t:#x} [{sec_of(t)}] stores {v:#x} (=base+{v - BASE:#x})")

# export directory
EXP_RVA = 0x1DCD190
eo = rva2off(EXP_RVA)
chars, ts, mj, mi, nm, base_nfunc, nfunc, nnames, afuncs, anames, ord_base = \
    struct.unpack_from("<IHHIIHHIIII", b, eo)
print(f"\nexports: nfunc {nfunc} nnames {nnames} ord_base {ord_base}")
fo = rva2off(afuncs)
no = rva2off(anames)
for i in range(nnames):
    nrva = struct.unpack_from("<I", b, no + 4 * i)[0]
    x = rva2off(nrva)
    name = b[x:b.find(b"\x00", x)].decode("latin-1")
    ori = struct.unpack_from("<H", b, rva2off(struct.unpack_from("<I", b, eo + 36)[0]) + 2 * i)[0]
    frva = struct.unpack_from("<I", b, rva2off(afuncs) + 4 * ori)[0]
    print(f"  export {i}: {name} -> rva {frva:#x} [{sec_of(frva)}]")

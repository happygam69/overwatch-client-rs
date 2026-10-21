"""Probe the loader offline: warm-image IAT resolution + the 0x48ffd0 page.

- loader_after_diff.bin (warm capture, flat RVA image, base 0)
- loader_disk_rva.bin  (disk loader flattened to RVA space)
Both 0x2410000 bytes; byte at offset X == RVA X.

Prints:
  1. bytes/disasm at rva 0x48ffd0 in both images (disk TLS callback 0;
     expected zero in the warm capture).
  2. the runtime IAT (rva 0x1dcdd58, dd[12], 0x960 bytes = 300 qwords):
     resolved pointers from the warm image, clustered by module, each
     matched against C:\\Windows\\System32 export tables where possible.
"""
import struct

WARM = r"D:\ow-dumps\loader_after_diff.bin"
DISK = r"D:\ow-dumps\loader_disk_rva.bin"

warm = open(WARM, "rb").read()
disk = open(DISK, "rb").read()
assert len(warm) == len(disk) == 0x2410000, (len(warm), len(disk))


def show(name, img, rva, n):
    b = img[rva:rva + n]
    print(f"{name} rva {rva:#x}: {b[:32].hex(' ')}")
    return b


def disasm(img, rva, n, label):
    import capstone as cs
    md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
    code = img[rva:rva + n]
    if not any(code):
        print(f"{label} rva {rva:#x}: all zero ({n} bytes)")
        return
    print(f"--- {label} rva {rva:#x} ---")
    for i, ins in enumerate(md.disasm(code, rva)):
        print(f"  {ins.address:#09x}  {ins.bytes.hex():<20} {ins.mnemonic} {ins.op_str}")
        if i >= 40:
            break


print("== rva 0x48ffd0 (TLS callback 0 on disk) ==")
show("warm", warm, 0x48ffd0, 32)
show("disk", disk, 0x48ffd0, 32)
disasm(disk, 0x48ffd0, 160, "disk")

print("\n== runtime IAT rva 0x1dcdd58 ==")
IAT, N = 0x1DCDD58, 0x960 // 8
slots = struct.unpack_from(f"<{N}Q", warm, IAT)
# cluster by top-16-bits groups (module base in-session)
bases = {}
for i, p in enumerate(slots):
    if p:
        bases.setdefault(p >> 40, []).append(i)
print(f"{sum(1 for p in slots if p)} resolved of {N} slots; clusters:")
for hi, idxs in sorted(bases.items(), key=lambda kv: -len(kv[1])):
    ptrs = [slots[i] for i in idxs]
    print(f"  cluster ptr>>40 {hi:#x}: {len(idxs)} slots, "
          f"first {ptrs[0]:#x} last {ptrs[-1]:#x}")

# resolve against system dlls: for each cluster, guess base so that
# pointers map into export ranges of known dlls
import os
sys32 = r"C:\Windows\System32"
candidates = ["ntdll.dll", "kernel32.dll", "kernelbase.dll", "ucrtbase.dll",
              "bcrypt.dll", "advapi32.dll", "user32.dll", "ole32.dll"]


def exports(path):
    b = open(path, "rb").read()
    pe = struct.unpack_from("<I", b, 0x3C)[0]
    opt = struct.unpack_from("<H", b, pe + 20)[0]
    nsec = struct.unpack_from("<H", b, pe + 6)[0]
    s0 = pe + 24 + opt
    secs = []
    for i in range(nsec):
        o = s0 + 40 * i
        vsz, va, rsz, ro = struct.unpack_from("<IIII", b, o + 8)
        secs.append((va, max(vsz, rsz), ro))
    dd = pe + 24 + 112
    exp_rva, _ = struct.unpack_from("<II", b, dd)
    if not exp_rva:
        return {}
    off = None
    for va, sz, ro in secs:
        if va <= exp_rva < va + sz:
            off = ro + (exp_rva - va)
    names_rva, _ = struct.unpack_from("<II", b, off + 12)
    noff = None
    for va, sz, ro in secs:
        if va <= names_rva < va + sz:
            noff = ro + (names_rva - va)
    n = struct.unpack_from("<I", b, off + 24)[0]
    ord_rva = struct.unpack_from("<I", b, off + 36)[0]
    ooff = None
    for va, sz, ro in secs:
        if va <= ord_rva < va + sz:
            ooff = ro + (ord_rva - va)
    out = {}
    for i in range(n):
        nrva = struct.unpack_from("<I", b, noff + 4 * i)[0]
        x = None
        for va, sz, ro in secs:
            if va <= nrva < va + sz:
                x = ro + (nrva - va)
        nm = b[x:b.find(b"\x00", x)].decode("latin-1")
        fo = struct.unpack_from("<H", b, ooff + 2 * i)[0]
        frva = struct.unpack_from("<I", b, off + 28)[0]
        aoff = None
        for va, sz, ro in secs:
            if va <= frva < va + sz:
                aoff = ro + (frva - va)
        arva = struct.unpack_from("<I", b, aoff + 4 * fo)[0]
        out[nm] = arva
    return out


tables = {}
for dll in candidates:
    p = os.path.join(sys32, dll)
    if os.path.exists(p):
        tables[dll] = exports(p)

for hi, idxs in sorted(bases.items(), key=lambda kv: -len(kv[1])):
    ptrs = [slots[i] for i in idxs]
    lo = min(ptrs)
    best = None
    for dll, tab in tables.items():
        # try base guesses: ptr - export_rva == base for some export
        votes = {}
        for p in ptrs[:40]:
            for nm, rva in tab.items():
                if 0 < p - rva < (1 << 47) and (p - rva) >> 32 == hi:
                    votes[(p - rva, dll)] = votes.get((p - rva, dll), 0) + 1
        if votes:
            (base, bdll), n = max(votes.items(), key=lambda kv: kv[1])
            if best is None or n > best[2]:
                best = (base, bdll, n)
    if best:
        base, bdll, n = best
        tab = tables[bdll]
        named = []
        for i in idxs:
            rva = slots[i] - base
            nm = next((k for k, v in tab.items() if v == rva), None)
            named.append((IAT + 8 * i, slots[i], f"{bdll}!{nm}" if nm else f"{bdll}+{rva:#x}"))
        print(f"\n  module {bdll} base {base:#x} (matched {n}):")
        for a, p, nmn in named:
            print(f"    iat {a:#x} -> {nmn}")

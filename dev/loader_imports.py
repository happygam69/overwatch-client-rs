import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else r"E:\Overwatch\_retail_\Overwatch_loader.dll"
b = open(path, "rb").read()
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
    for name, va, vsz, ro, rsz in secs:
        if va <= rva < va + max(vsz, rsz):
            return ro + (rva - va)
    return None


# Data directories start at opt + 112 for PE32+.
dd = pe + 24 + 112
imp_rva, imp_sz = struct.unpack_from("<II", b, dd + 8)
print(f"import dir rva {imp_rva:#x} size {imp_sz:#x}")
off = rva2off(imp_rva)
names = []
while off is not None:
    ilt, ts, fc, name_rva, iat = struct.unpack_from("<IIIII", b, off)
    if name_rva == 0:
        break
    no = rva2off(name_rva)
    dll = b[no:b.find(b"\x00", no)].decode("latin-1")
    print(f"\n{dll}: ILT {ilt:#x} IAT {iat:#x}")
    t = rva2off(ilt) if ilt else rva2off(iat)
    i = 0
    while t is not None:
        ent = struct.unpack_from("<Q", b, t + 8 * i)[0]
        if ent == 0:
            break
        if ent & (1 << 63):
            print(f"   [{i:3}] ordinal {ent & 0xffff}")
        else:
            fo = rva2off((ent & 0x7FFFFFFF) + 2)
            if fo:
                nm = b[fo:b.find(b"\x00", fo)].decode("latin-1")
                print(f"   [{i:3}] {nm}  (iat {iat + 8 * i:#x})")
                names.append((dll, nm, iat + 8 * i))
        i += 1
        if i > 4000:
            break
    off += 20
print(f"\ntotal imports: {len(names)}")
for dll, nm, iat in names:
    if any(k in nm for k in ("Continue", "Vectored", "Exception", "Protect", "Virtual", "Map", "Section")):
        print(f"ANCHOR {dll}!{nm} IAT {iat:#x}")

"""Parse Overwatch_loader.dll headers: sections, data dirs, TLS callback array.

Usage: python dev/ldr_tls.py [path]
Default path is the retail install file (read-only).
"""
import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else r"E:\Overwatch\_retail_\Overwatch_loader.dll"
b = open(path, "rb").read()
pe = struct.unpack_from("<I", b, 0x3C)[0]
machine, nsec = struct.unpack_from("<HH", b, pe + 4)
opt = struct.unpack_from("<H", b, pe + 20)[0]
s0 = pe + 24 + opt
print(f"machine {machine:#x} nsec {nsec} optsz {opt:#x} nt {pe:#x}")
secs = []
for i in range(nsec):
    o = s0 + 40 * i
    name = b[o:o + 8].split(b"\x00")[0].decode("latin-1")
    vsz, va, rsz, ro = struct.unpack_from("<IIII", b, o + 8)
    chars = struct.unpack_from("<I", b, o + 36)[0]
    secs.append((name, va, vsz, ro, rsz, chars))
    print(f"sec {name:10} va {va:#010x} vsz {vsz:#010x} ro {ro:#010x} rsz {rsz:#010x} chars {chars:#010x}")

# Data directories: PE32+ -> at opt header + 112, count at +108
ndd = struct.unpack_from("<I", b, pe + 24 + 108)[0]
dd0 = pe + 24 + 112
for i in range(min(ndd, 16)):
    rva, sz = struct.unpack_from("<II", b, dd0 + 8 * i)
    if rva:
        print(f"dd[{i}] rva {rva:#x} size {sz:#x}")


def rva2off(rva):
    for name, va, vsz, ro, rsz, chars in secs:
        if va <= rva < va + max(vsz, rsz):
            return ro + (rva - va)
    return None


# TLS = dd index 9
tls_rva, tls_sz = struct.unpack_from("<II", b, dd0 + 9 * 8)
if tls_rva:
    to = rva2off(tls_rva)
    print(f"\nTLS dir rva {tls_rva:#x} file off {to:#x} size {tls_sz:#x}")
    # PE64 layout: start(+0) end(+8) index(+16) callbacks(+24) zero(4) chars(4)
    raw_start, raw_end, idx, cb = struct.unpack_from("<QQQQ", b, to)
    print(f"  start {raw_start:#x} end {raw_end:#x} index {idx:#x} callbacks {cb:#x}")
    cb_rva = cb - 0x180000000
    co = rva2off(cb_rva)
    if co is None:
        print(f"  callback array rva {cb_rva:#x}: no file offset (unmapped?)")
    else:
        print(f"  callback array rva {cb_rva:#x} off {co:#x}")
        i = 0
        while True:
            ent = struct.unpack_from("<Q", b, co + 8 * i)[0]
            if ent == 0 or i > 32:
                break
            print(f"  callback[{i}] {ent:#x}")
            i += 1

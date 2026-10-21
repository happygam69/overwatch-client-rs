"""Parse Overwatch_loader.dll load config (dd[10]) and the .gxfg section.

The load config carries GuardCFCheckFunctionPointer / GuardCFFunctionTable /
GuardXFG tables; the .gxfg section (rva 0x1f30000, vsz 0x2610) holds the
function table entries (rva|flag pairs) for CFG/XFG-instrumented code.

Usage: python dev/ldr_gxfg.py
"""
import struct

path = r"E:\Overwatch\_retail_\Overwatch_loader.dll"
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


# IMAGE_LOAD_CONFIG_DIRECTORY64 fields (PE32+), from load config start:
#  Size(4) TimeDateStamp(4) Version(2) GlobalFlagsClear(2) GlobalFlagsSet(2)
#  CriticalSectionDefaultTimeout(4) DeCommitFreeBlockThreshold(8)
#  DeCommitTotalFreeThreshold(8) LockPrefixTable(8) MaximumAllocationSize(8)
#  VirtualMemoryThreshold(8) ProcessHeapFlags(4) ProcessAffinityMask(4)
#  CSDVersion(2) DependentLoadFlags(2) EditList(8) SecurityCookie(8)
#  SEHandlerTable(8) SEHandlerCount(8) GuardCFCheckFunctionPointer(8)
#  GuardCFDispatchFunctionPointer(8) GuardCFFunctionTable(8)
#  GuardCFFunctionCount(8) GuardFlags(4) ...
off = rva2off(0x1D70AC0)
hdr = b[off:off + 0x140]
size = struct.unpack_from("<I", hdr, 0)[0]
print(f"load config size {size:#x}")
cookie = struct.unpack_from("<Q", hdr, 0x50)[0]
seh_tab, seh_cnt = struct.unpack_from("<QQ", hdr, 0x58)
gcfchk = struct.unpack_from("<Q", hdr, 0x68)[0]
gcfdisp = struct.unpack_from("<Q", hdr, 0x70)[0]
gcftab, gcfcnt = struct.unpack_from("<QQ", hdr, 0x78)
gflags = struct.unpack_from("<I", hdr, 0x88)[0]
code_integ = struct.unpack_from("<I", hdr, 0x8C)[0]
print(f"  security cookie rva {cookie:#x}")
print(f"  seh handler table {seh_tab:#x} count {seh_cnt:#x}")
print(f"  guard cf check fn ptr {gcfchk:#x}")
print(f"  guard cf dispatch fn ptr {gcfdisp:#x}")
print(f"  guard cf function table {gcftab:#x} count {gcfcnt:#x}")
print(f"  guard flags {gflags:#x} codeintegrity flags {code_integ:#x}")

# dump raw words of the rest for extra fields (0x8C..)
rest = struct.unpack_from("<24I", hdr, 0x90)
print("  rest dwords:", " ".join(f"{x:#x}" for x in rest))

# GuardCF function table: entries are 4-byte (rva | flags) when
# GUARD_CF_FLAG_ENCODED... For x64 with XFG, the table may be 8-byte.
if gcftab:
    tab_rva = gcftab - 0x180000000
    to = rva2off(tab_rva)
    print(f"\n.guardcf table rva {tab_rva:#x} off {to:#x} count {gcfcnt}")
    if to is not None:
        ent_sz = 8 if (gflags & 0x70000000) or gcfcnt * 4 > 0x2610 else 4
        print(f"  assuming entry size {ent_sz}")
        for i in range(min(gcfcnt, 4096)):
            if ent_sz == 4:
                v = struct.unpack_from("<I", b, to + 4 * i)[0]
            else:
                v = struct.unpack_from("<Q", b, to + 8 * i)[0]
            if i < 24 or i > gcfcnt - 8:
                print(f"    [{i:4}] {v:#x}")
        # flag histogram
        if ent_sz == 4:
            vals = struct.unpack_from(f"<{gcfcnt}I", b, to)
        else:
            vals = struct.unpack_from(f"<{gcfcnt}Q", b, to)
        hist = {}
        for v in vals:
            f = v >> 28 if ent_sz == 4 else v & 0xF0000000
            hist[f] = hist.get(f, 0) + 1
        print("  flag histogram:", {f"{k:#x}": n for k, n in hist.items()})

# .gxfg raw content digest: first 128 bytes as qwords
gx = rva2off(0x1F30000)
print("\n.gxfg head qwords:")
for i in range(16):
    v = struct.unpack_from("<Q", b, gx + 8 * i)[0]
    print(f"  {0x1F30000 + 8 * i:#x}: {v:#018x}")

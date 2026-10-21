"""Scan warm .text pages for 4/8-byte little-endian immediate constants.

Usage: python dev/imm_scan.py <img> 0xC486B05D [more values...]
       [--width 4] [--base 0x..]

Some STU field hashes appear inline in the attribute getters (the
0x1c4f8e14 pattern).  This finds every code page whose bytes contain a
value, reporting the containing .pdata function when one exists.
"""
import json
import struct
import sys

argv = sys.argv[1:]
img = argv.pop(0)
width = 4
base = None
vals = []
while argv:
    a = argv.pop(0)
    if a == "--width":
        width = int(argv.pop(0))
    elif a == "--base":
        base = int(argv.pop(0), 16)
    else:
        vals.append(int(a, 16))

if base is None:
    side = json.load(open(img + ".json", encoding="utf-8"))
    base = int(side["base"])
blob = open(img, "rb").read()

exe = open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read()


def pdata_entries(exe):
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    ro = vsz = None
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        v, va, r, r_off = struct.unpack_from("<IIII", exe, o + 8)
        if name == ".pdata":
            ro, vsz = r_off, v
    raw = exe[ro:ro + vsz]
    return [struct.unpack_from("<II", raw, 12 * i)
            for i in range(len(raw) // 12)]


pd = pdata_entries(exe)
fmt = "<I" if width == 4 else "<Q"
needle_len = width

for v in vals:
    needle = struct.pack(fmt, v)
    starts = []
    pos = 0
    while True:
        pos = blob.find(needle, pos)
        if pos < 0:
            break
        if 0x10000 <= pos < 0x2f95913:
            starts.append(pos)
        pos += 1
    print(f"{v:#x}: {len(starts)} hits in .text")
    shown = 0
    for st in starts:
        fn = next(((b, e) for b, e in pd if b <= st < e), None)
        loc = f"fn {fn[0]:#x}-{fn[1]:#x}" if fn else "no pdata"
        print(f"  at {st:#x} ({loc})")
        shown += 1
        if shown >= 14:
            print("  ...")
            break

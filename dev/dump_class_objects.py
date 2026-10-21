"""Dump the six animation-class objects' method pointers.

Usage: python dev/dump_class_objects.py <img> [--base 0x..] [--count 6]
       [--start 0x40817c0] [--stride 0x160]

Objects are the static class records: +0x88 self, +0x90 descriptor,
+0x98 global, +0xf8..+0x130 virtual methods (code pointers), +0xf0/0x138
hashes/ids.  Prints each object's code pointers resolved to rvas with a
warm/cold flag, then the unique target list with .pdata attribution.
"""
import json
import struct
import sys


def sections(exe):
    pe = struct.unpack_from("<I", exe, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe, pe + 6)[0]
    opt = struct.unpack_from("<H", exe, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe, o + 8)
        out.append((name, va, vsz, ro, rsz))
    return out


def pdata_entries(exe):
    secs = sections(exe)
    ro = None
    vsz = 0
    for name, va, s2, r_off, rsz in secs:
        if name == ".pdata":
            ro, vsz = r_off, s2
    raw = exe[ro:ro + vsz]
    return [struct.unpack_from("<II", raw, 12 * i)
            for i in range(len(raw) // 12)]


def main():
    argv = sys.argv[1:]
    img = argv.pop(0)
    base = None
    start, stride, count = 0x40817C0, 0x160, 6
    while argv:
        a = argv.pop(0)
        if a == "--base":
            base = int(argv.pop(0), 16)
        elif a == "--start":
            start = int(argv.pop(0), 16)
        elif a == "--stride":
            stride = int(argv.pop(0), 16)
        elif a == "--count":
            count = int(argv.pop(0))
    if base is None:
        side = json.load(open(img + ".json", encoding="utf-8"))
        base = int(side["base"])
    a = open(img, "rb").read()
    end = base + len(a)

    def warm(tr):
        return "w" if any(a[((tr >> 12) << 12):((tr >> 12) << 12) + 16]) \
            else "c"

    targets = set()
    for k in range(count):
        o = start + k * stride
        row = []
        for off in range(0xF8, 0x138, 8):
            v = struct.unpack_from("<Q", a, o + off)[0]
            if base <= v < end:
                tr = v - base
                row.append(f"{tr:#x}{warm(tr)}")
                targets.add(tr)
            else:
                row.append(hex(v))
        print(f"obj {k} @ {o:#x}: " + " ".join(row))

    exe = open(r"E:\Overwatch\_retail_\Overwatch.exe", "rb").read()
    pd = pdata_entries(exe)
    print(f"\nunique targets: {len(targets)}")
    for t in sorted(targets):
        fn = next(((b, e) for b, e in pd if b <= t < e), None)
        if fn:
            print(f"  {t:#x} in fn {fn[0]:#x}-{fn[1]:#x}")
        else:
            print(f"  {t:#x} no pdata range")


if __name__ == "__main__":
    main()

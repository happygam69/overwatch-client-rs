"""Dump qwords of given rvas from a module image.

Usage: python dev/dump_qwords.py <img.bin> [--base 0x<V2B>] <rva> ...
  - session-coherent images (session_merge.py output): gives the base
    from the .json sidecar.
  - any other flat image (e.g. module_after_diff.bin): pass --base.

Each rva prints 6 qwords.  In-module values (within [base, base+size))
print their target rva plus a warm/cold flag for the target page ('w' /
'c'); otherwise the raw value.
"""
import json
import struct
import sys

args = []
base = None
i = 1
img = None
while i < len(sys.argv):
    if sys.argv[i] == "--base":
        base = int(sys.argv[i + 1], 16)
        i += 2
    elif img is None:
        img = sys.argv[i]
        i += 1
    else:
        args.append(sys.argv[i])
        i += 1

if img is None:
    raise SystemExit("usage: dump_qwords.py <img> [--base 0x..] <rva> ...")
if base is None:
    side = json.load(open(img + ".json", encoding="utf-8"))
    base = int(side["base"])

a = open(img, "rb").read()
end = len(a)


def page_warm(rva):
    if not (0 <= rva < end - 8):
        return False
    chunk = a[((rva >> 12) << 12):((rva >> 12) << 12) + 16]
    return any(chunk)


for arg in args:
    rva = int(arg, 16)
    vals = struct.unpack_from("<6Q", a, rva)
    parts = []
    for v in vals:
        if base <= v < end:
            tr = v - base
            parts.append(f"r{tr:#x}" + ((":w" if page_warm(tr) else ":c")))
        else:
            parts.append(f"{v:#x}")
    print(f"{rva:#x}: " + " ".join(parts))

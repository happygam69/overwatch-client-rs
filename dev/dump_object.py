"""Dump an object region as qwords with pointer analysis.

Usage: python dev/dump_object.py <img> <rva> <len> [--base 0x..]
Prints offset, raw qword, and for in-module values the rva + warm flag.
"""
import json
import struct
import sys

args = []
base = None
img = None
i = 1
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

rva = int(args[0], 16)
length = int(args[1], 16) if len(args) > 1 else 0x100

if base is None:
    side = json.load(open(img + ".json", encoding="utf-8"))
    base = int(side["base"])
a = open(img, "rb").read()
end = len(a)


def warm(tr):
    if not (0 <= tr < end - 8):
        return "?"
    return "w" if any(a[((tr >> 12) << 12):((tr >> 12) << 12) + 16]) else "c"


off = 0
while off < length:
    v = struct.unpack_from("<Q", a, rva + off)[0]
    if base <= v < end:
        tr = v - base
        print(f"+{off:03x} rva {rva + off:#x}: {v:#018x} -> rva {tr:#x}{warm(tr)}")
    else:
        print(f"+{off:03x} rva {rva + off:#x}: {v:#018x}")
    off += 8

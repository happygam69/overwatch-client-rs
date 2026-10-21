"""Look at a clip section as bytes, in every cheap reading at once.

No hypothesis here -- four columns so the structure is visible: raw bytes,
the same bytes as u16, as i32 and as f32.  Read-only.
"""

import struct
import sys


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def i32(b, o):
    return struct.unpack_from("<i", b, o)[0]


def f32(b, o):
    return struct.unpack_from("<f", b, o)[0]


def i64(b, o):
    return struct.unpack_from("<q", b, o)[0]


def main() -> int:
    path = sys.argv[1]
    start = int(sys.argv[2], 0)
    length = int(sys.argv[3], 0) if len(sys.argv) > 3 else 0x100
    b = open(path, "rb").read()
    end = min(start + length, len(b))
    print("%s  size=0x%x  frames=%d  fps=%d  dur=%.3f" %
          (path, len(b), u16(b, 0x0c), u16(b, 0x0e), f32(b, 0x14)))
    print("table: " + " ".join("0x%x" % i64(b, 0x30 + 8 * i) for i in range(10)))
    print()
    o = start & ~0xF
    while o < end:
        row = b[o:o + 16]
        if len(row) < 16:
            break
        hx = " ".join("%02x" % x for x in row)
        hx2 = "".join("%02x" % x for x in row)
        h16 = " ".join("%5d" % u16(b, o + 2 * i) for i in range(8))
        h32 = " ".join("%11d" % i32(b, o + 4 * i) for i in range(4))
        hfl = " ".join("%12.5f" % f32(b, o + 4 * i) for i in range(4))
        print("%06x  %s  %s" % (o, hx2, h16))
        print("            i32 %s" % h32)
        print("            f32 %s" % hfl)
        o += 16
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

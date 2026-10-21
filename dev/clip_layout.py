"""Map a clip's reference graph and solve for the element size of each block.

Established from the live diff (clip ...005f, live 0x15d4be00000): the client
does not keep the chunk verbatim.  Every 8-byte slot in section 1 whose file
form is {u32 v, u32 0} is rewritten at load to the qword `buffer + v`, e.g.

    slot @0x150: file 368  ->  mem buffer+368 = 0x15d4be04b70

and sections 0 and 2 come through byte-identical.  So in the file a reference is
{u32 target, u32 pad, u32 count, u32 pad} and the target is an offset into the
same buffer.

With a count in hand, a block's element size is (next_target - target)/count.
If some tracks are sampled once per frame, their count is frames+1 and the
element size that comes out of the arithmetic is the sample stride -- which is
the number this whole line of work needs.  Every hypothesis here is checked
against the arithmetic rather than assumed.

Read-only.
"""

import glob
import os
import struct
import sys
from collections import Counter


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def i64(b, o):
    return struct.unpack_from("<q", b, o)[0]


def f32(b, o):
    return struct.unpack_from("<f", b, o)[0]


def sections(b):
    raw = [i64(b, 0x30 + 8 * i) for i in range(10)]
    starts = sorted({x for x in raw if 0x80 <= x < len(b)})
    out = []
    for i, s in enumerate(starts):
        e = starts[i + 1] if i + 1 < len(starts) else len(b)
        out.append((s, e))
    return out


def refs(b, a, z):
    """All {u32 target, u32 0, u32 count, u32 0} slots in [a, z)."""
    out = []
    o = a & ~3
    while o + 16 <= z:
        t = u32(b, o)
        if (0x80 <= t < len(b) and u32(b, o + 4) == 0 and
                0 < u32(b, o + 8) <= 0x4000 and u32(b, o + 12) == 0):
            out.append((o, t, u32(b, o + 8)))
            o += 16
        else:
            o += 4
    return out


def classify(b, a, n):
    """A cheap reading of what a block looks like."""
    if n < 8:
        return "tiny"
    floats = 0
    for i in range(0, n - 3, 4):
        v = f32(b, a + i)
        if v == 0.0 or (-1000.0 < v < 1000.0 and abs(v) > 1e-6):
            floats += 1
    frac = floats / max(1, n // 4)
    u16s = [u16(b, a + 2 * i) for i in range(min(24, n // 2))]
    small = sum(1 for v in u16s if v < 0x400)
    tag = []
    if frac > 0.9:
        tag.append("f32")
    if small > len(u16s) * 0.9:
        tag.append("u16small")
    return "+".join(tag) or "mixed"


def main() -> int:
    argv = sys.argv[1:]
    files = []
    for a in argv:
        if a.startswith("D:") or "/" in a:
            files.append(a)
        else:
            files.extend(sorted(glob.glob("D:/ow-dumps/asset_0a*%s.bin" % a)))
    if not files:
        files = sorted(glob.glob("D:/ow-dumps/asset_0a*.bin"))[:3]

    elem_hist = Counter()
    count_hist = Counter()
    for path in files:
        b = open(path, "rb").read()
        frames = u16(b, 0x0c)
        secs = sections(b)
        rs = []
        for s, e in secs:
            rs.extend(refs(b, s, e))
        rs.sort()
        print("\n=== %s  size=%d frames=%d fps=%d secs=%s refs=%d"
              % (os.path.basename(path), len(b), frames, u16(b, 0x0e),
                 [("%x" % s, "%x" % e) for s, e in secs], len(rs)))
        # element size from the gap to the next target
        for i, (o, t, c) in enumerate(rs):
            nxt = None
            for (o2, t2, c2) in rs:
                if t2 > t:
                    nxt = t2 if nxt is None else min(nxt, t2)
            gap = (nxt - t) if nxt is not None else (len(b) - t)
            if c and gap > 0 and gap % c == 0:
                elem_hist[gap // c] += 1
            count_hist[c] += 1
        for (o, t, c) in rs[:14]:
            nxt = None
            for (o2, t2, c2) in rs:
                if t2 > t:
                    nxt = t2 if nxt is None else min(nxt, t2)
            gap = (nxt - t) if nxt is not None else (len(b) - t)
            print("   @0x%06x -> 0x%06x count=%-5d gap=%-6d elem=%-6s  %s"
                  % (o, t, c, gap,
                     ("%g" % (gap / c)) if c else "-",
                     classify(b, t, min(gap, len(b) - t))))

    print("\n--- element size == gap/count, histogram over all refs")
    for k, n in elem_hist.most_common(12):
        print("      %6s  %d" % (k, n))
    print("--- ref counts, most common (frames+1 would be the dense case)")
    for k, n in sorted(count_hist.items())[:20]:
        print("      count %-6d %d" % (k, n))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Analysis workbench for the animation clip chunk (asset type 0x006).

The chunk is a raw format with no reader in the reference (`owm-formats/owanimclip.txt`
documents the older OW1 layout), so it is read empirically: a corpus of clips
written out by `ow-tank-anim --list 0x006 --raw`, their headers tabulated, and
candidate structures tested against the whole corpus rather than one sample.

Modes:
    headers   one row per clip: every header field, for grouping
    offsets   test whether the qwords after +0x2c are offsets into the file
    map       a coarse structural map of one clip (32-byte blocks)
    diff      two clips side by side, to see what scales with what

Usage:
    python clip_lab.py headers
    python clip_lab.py map <file>
    python clip_lab.py diff <fileA> <fileB>

Read-only.
"""

from __future__ import annotations

import glob
import struct
import sys

CORPUS = "D:/ow-dumps/asset_0a*.bin"


def corpus() -> list[tuple[str, bytes]]:
    out = []
    for path in sorted(glob.glob(CORPUS)):
        with open(path, "rb") as fh:
            out.append((path.split("\\")[-1].split("/")[-1], fh.read()))
    return out


def u16(b: bytes, o: int) -> int:
    return struct.unpack_from("<H", b, o)[0]


def u32(b: bytes, o: int) -> int:
    return struct.unpack_from("<I", b, o)[0]


def i32(b: bytes, o: int) -> int:
    return struct.unpack_from("<i", b, o)[0]


def f32(b: bytes, o: int) -> float:
    return struct.unpack_from("<f", b, o)[0]


def u64(b: bytes, o: int) -> int:
    return struct.unpack_from("<Q", b, o)[0]


def headers() -> None:
    rows = []
    for name, b in corpus():
        frames = u16(b, 0x0C)
        fps = u16(b, 0x0E)
        dur = f32(b, 0x14)
        rows.append(
            (
                name,
                len(b),
                frames,
                fps,
                dur,
                u32(b, 0x00),
                u32(b, 0x04),
                u32(b, 0x08),
                u32(b, 0x10),
                u32(b, 0x24),
                u32(b, 0x28),
                u64(b, 0x2C),
                u64(b, 0x34),
                u64(b, 0x3C),
                u64(b, 0x44),
            )
        )
    print("file                     size frames fps   dur      +00       +04  +08  +10       id        +28    +2c   +34   +3c   +44")
    for r in rows:
        print(
            "%-20s %6d %5d %4d %6.3f  %08x %5d %4d %4d  %08x %5d  %5d %5d %5d %5d"
            % (r[0][6:18], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10], r[11], r[12], r[13], r[14])
        )
    # Relationships worth testing across the corpus.
    print()
    sized = [r for r in rows if r[2] > 0]
    print("size / frames         : min %.0f max %.0f" % (
        min(r[1] / max(1, r[2]) for r in sized), max(r[1] / max(1, r[2]) for r in sized)))
    exact = sum(1 for r in rows if abs(r[4] - r[2] / max(1, r[3])) < 1e-4)
    print("duration == frames/fps: %d of %d clips" % (exact, len(rows)))
    print("fps values            : %s" % sorted({r[3] for r in rows}))
    print("+28 values            : %s" % sorted({r[10] for r in rows}))
    print("+04/+08/+10 values    : %s / %s / %s" % (
        sorted({r[6] for r in rows})[:6], sorted({r[7] for r in rows})[:6], sorted({r[8] for r in rows})[:6]))


def offsets() -> None:
    """Are the qwords after +0x2c file offsets, and do they land on structure?"""
    print("file                 size  seq of qwords from +0x2c (in-file? values)")
    for name, b in corpus()[:20]:
        vals = [u64(b, o) for o in range(0x30, 0x30 + 8 * 12, 8)]
        shown = []
        inside = 0
        for v in vals:
            s = struct.unpack("<q", struct.pack("<Q", v))[0]
            if s == -1:
                shown.append("-1")
            elif 0 < s < len(b):
                inside += 1
                shown.append("%d%s" % (s, "" if b[s : s + 1] else "?"))
            else:
                shown.append("0x%x" % v)
        print("%-16s %6d  in=%2d  %s" % (name[6:16], len(b), inside, " ".join(shown)))


def block_profile(b: bytes, step: int = 32) -> list[str]:
    """Per-block: what kind of data is here (u16/u32/float/zero/mixed)."""
    out = []
    for o in range(0, len(b), step):
        blk = b[o : o + step]
        nz = sum(1 for x in blk if x)
        if nz == 0:
            out.append(".")
            continue
        if step % 4 == 0:
            fl = 0
            small = 0
            for i in range(0, step, 4):
                v = struct.unpack_from("<f", blk, i)[0]
                if -1.0001 <= v <= 1.0001:
                    fl += 1
                if struct.unpack_from("<I", blk, i)[0] < 4096:
                    small += 1
            n = step // 4
            if fl == n:
                out.append("f")
            elif fl >= n - 1 and fl > 0:
                out.append("+")
            elif small == n:
                out.append("i")
            elif small >= n - 1:
                out.append("-")
            else:
                out.append("x")
        else:
            out.append("x")
    return out


def map_one(path: str) -> None:
    with open(path, "rb") as fh:
        b = fh.read()
    frames, fps = u16(b, 0x0C), u16(b, 0x0E)
    print("%s  %d B  frames %d fps %d dur %.3f" % (path, len(b), frames, fps, f32(b, 0x14)))
    prof = block_profile(b)
    for i in range(0, len(prof), 64):
        row = prof[i : i + 64]
        print("  +%05x  %s" % (i * 32, "".join(row)))
    print("  legend: f/pure-float  +/mostly-float  i/pure-small-int  -/mostly-small-int  x/mixed  ./zero")


def diff(a: str, b_: str) -> None:
    with open(a, "rb") as fh:
        A = fh.read()
    with open(b_, "rb") as fh:
        B = fh.read()
    print("A %s %d B frames %d dur %.3f" % (a, len(A), u16(A, 0x0C), f32(A, 0x14)))
    print("B %s %d B frames %d dur %.3f" % (b_, len(B), u16(B, 0x0C), f32(B, 0x14)))
    pa, pb = block_profile(A), block_profile(B)
    n = max(len(pa), len(pb))
    same = 0
    first_diff = None
    for i in range(n):
        x = pa[i] if i < len(pa) else " "
        y = pb[i] if i < len(pb) else " "
        if x == y:
            same += 1
        elif first_diff is None:
            first_diff = i * 32
    print("blocks identical: %d of %d; first differing block offset: %s" % (same, n, first_diff))


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    match args[0]:
        case "headers":
            headers()
        case "offsets":
            offsets()
        case "map":
            map_one(args[1])
        case "diff":
            diff(args[1], args[2])
        case _:
            print(__doc__)
            return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Diff a live clip buffer against the clip file on disk.

If the client expands or reorders the chunk when it loads it, the difference
between memory and the file *is* the load-time decode -- which is the thing this
whole line of work is trying to recover.  So: read the file-sized region at a
known clip address, identify which corpus file it is, and print where and how
they differ (section by section, with the first differing cells).

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.

Usage:
    python live_clip_diff.py --pid N --at 0xADDR [--file PATH] [--max-mb 8]
"""

from __future__ import annotations

import glob
import os
import struct
import sys

import dump_process as DP

CHUNK = 4 << 20


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


def read_live(handle, at, n):
    out = b""
    off = 0
    while off < n:
        data = DP.read_region(handle, at + off, min(CHUNK, n - off))
        if not data:
            break
        out += data
        off += len(data)
    return out


def corpus():
    out = []
    for path in sorted(glob.glob("D:/ow-dumps/asset_0a*.bin")):
        with open(path, "rb") as fh:
            out.append((path, fh.read()))
    return out


def identify(head, files):
    """The files whose first 24 bytes match, longest full match first."""
    hits = [(p, b) for p, b in files if b[:24] == head[:24]]
    return hits


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    at = int(args[args.index("--at") + 1], 0) if "--at" in args else 0
    only = args[args.index("--file") + 1] if "--file" in args else None
    if not (pid and at):
        print(__doc__)
        return 2

    handle = DP.open_target(pid)
    head = read_live(handle, at, 0x100)
    if len(head) < 0x100:
        print("could not read 0x100 bytes at 0x%x" % at)
        return 1
    files = corpus()
    hits = identify(head, files)
    if only:
        hits = [h for h in hits if os.path.basename(h[0]) == os.path.basename(only)]
    if not hits:
        print("no corpus clip matches the first 24 bytes at 0x%x" % at)
        print("  head: " + head[:24].hex())
        return 1
    print("0x%x matches %d corpus clip(s)" % (at, len(hits)))
    for path, b in hits:
        print("   %s  size=%d frames=%d" % (os.path.basename(path), len(b),
                                            u16(b, 0x0c)))

    # pick the one with the longest full match
    best = None
    for path, b in hits:
        mem = read_live(handle, at, min(len(b), 8 << 20))
        same = 0
        while same < len(mem) and same < len(b) and mem[same] == b[same]:
            same += 1
        if best is None or same > best[2]:
            best = (path, b, same, mem)
    path, b, same, mem = best
    print("\nbest match: %s -- %d of %d bytes equal (%.2f%%)"
          % (os.path.basename(path), same, len(b), 100.0 * same / max(1, len(b))))
    if same == len(b):
        print("IDENTICAL: the client keeps this clip chunk verbatim in memory.")
        return 0

    print("\n--- first differing byte at 0x%x" % same)
    for sec_i, (s, e) in enumerate(sections(b)):
        mem_sec = mem[s:e] if e <= len(mem) else mem[s:]
        f = b[s:e]
        eq = sum(1 for i in range(min(len(mem_sec), len(f))) if mem_sec[i] == f[i])
        print("  sec%d [0x%x..0x%x) %6d bytes: %6d equal (%.1f%%)"
              % (sec_i, s, e, e - s, eq, 100.0 * eq / max(1, min(len(f), len(mem_sec)))))

    print("\n--- first 8 differing 16-byte cells")
    shown = 0
    for s, e in sections(b):
        o = s
        while o + 16 <= min(e, len(mem)) and shown < 8:
            if mem[o:o + 16] != b[o:o + 16]:
                print("  @0x%06x file %s" % (o, b[o:o + 16].hex()))
                print("            mem  %s" % mem[o:o + 16].hex())
                print("            as u32 file %s | mem %s"
                      % ([u32(b, o + 4 * i) for i in range(4)],
                         [u32(mem, o + 4 * i) for i in range(4)]))
                print("            as f32 file %s | mem %s"
                      % (["%.5g" % f32(b, o + 4 * i) for i in range(4)],
                         ["%.5g" % f32(mem, o + 4 * i) for i in range(4)]))
                shown += 1
                o += 16
            else:
                o += 16
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

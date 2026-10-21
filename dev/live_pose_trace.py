"""Capture the client's live pose as a trace, outside the repo.

The pose is an array of 64-byte bone records (measured, see
`docs/formats/animation.md`): three rows of a row-major 3x4 rotation+translation,
each row a unit triple with its translation component in lane 4, then a fourth row
whose lane 4 is exactly 0.  Quaternion hunts miss it entirely, which is why
`dev/live_pose_rank.py` reported nothing moving.

This writes the pose over time to a file *outside* the repository (`D:/ow-dumps`,
the same place every capture lives) so nothing game-derived can be committed.  The
trace is the ground truth for the decoder work: the clip's arrays have to produce
this.

File format (little-endian):
    char  magic[8] = "OWPOSE1\\0"
    u32   bone_count
    u32   hz
    u32   frames
    then frames * bone_count * 16 f32 (the 64-byte records, verbatim)

Usage:
    python live_pose_trace.py --pid N --at 0xADDR [--seconds 3] [--hz 30]
                              [--count N] [--out D:/ow-dumps/pose_trace.bin]

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import struct
import sys
import time

import numpy as np

import dump_process as DP


def read(handle, at, n, regions):
    end = at + n
    for base, size, protect, _kind, _alloc in regions:
        if base <= at < base + size:
            end = min(end, base + size)
            break
    out = b""
    off = at
    while off < end:
        d = DP.read_region(handle, off, min(1 << 20, end - off))
        if not d:
            break
        out += d
        off += len(d)
    return out


def is_bone(e: np.ndarray) -> bool:
    """Unit rows 0-2 with a translation in lane 4, and row 3's lane 4 zero."""
    if len(e) < 16:
        return False
    for r in (0, 4, 8):
        if abs(float(np.linalg.norm(e[r:r + 3])) - 1.0) > 0.05:
            return False
    trans = abs(float(e[3])) + abs(float(e[7])) + abs(float(e[11]))
    return trans > 1e-6 and abs(float(e[15])) < 1e-6


def extent(handle, at, step, regions, limit=512):
    """(count, start) of the bone array: try every phase, keep the longest run.

    `at` may sit anywhere inside a bone record, so the alignment has to be found
    rather than assumed.
    """
    best = None
    for ph in range(16):
        p0 = at + ph * 4
        p = read(handle, p0, step, regions)
        if len(p) != step or not is_bone(np.frombuffer(p, dtype="<f4")):
            continue
        back = 0
        while back < limit:
            q = read(handle, p0 - (back + 1) * step, step, regions)
            if len(q) != step or not is_bone(np.frombuffer(q, dtype="<f4")):
                break
            back += 1
        fwd = 0
        while fwd < limit:
            q = read(handle, p0 + (fwd + 1) * step, step, regions)
            if len(q) != step or not is_bone(np.frombuffer(q, dtype="<f4")):
                break
            fwd += 1
        cand = (back + fwd + 1, p0 - back * step, back, fwd)
        if best is None or cand[0] > best[0]:
            best = cand
    return best if best else (0, at, 0, 0)


def main() -> int:
    args = sys.argv[1:]

    def flag(name, default):
        if name not in args:
            return default
        v = args[args.index(name) + 1]
        return int(v, 0) if isinstance(default, int) else v

    pid = flag("--pid", 0)
    at = int(args[args.index("--at") + 1], 0) if "--at" in args else 0
    seconds = flag("--seconds", 3)
    hz = flag("--hz", 30)
    count = flag("--count", 0)
    out = flag("--out", "D:/ow-dumps/pose_trace.bin")
    if not (pid and at):
        print(__doc__)
        return 2

    step = 0x40
    handle = DP.open_target(pid)
    regions = DP.regions(handle)
    found, start, back, fwd = extent(handle, at, step, regions)
    n = count if count else found
    print("bones: %d backward, %d forward from 0x%x -> using %d from 0x%x (%d bytes)"
          % (back, fwd, at, n, start, n * step))
    if n < 2:
        print("no bone array found there; is the client still animating?")
        return 1

    frames = seconds * hz
    trace = bytearray()
    prev = None
    moved = np.zeros(n, dtype=np.int64)
    t0 = time.time()
    for i in range(frames):
        buf = read(handle, start, n * step, regions)
        if len(buf) != n * step:
            print("short read at frame %d (%d bytes); stopping" % (i, len(buf)))
            break
        a = np.frombuffer(buf, dtype="<f4").reshape(n, 16)
        if prev is not None:
            moved += (np.abs(a - prev).max(axis=1) > 1e-6).astype(np.int64)
        prev = a
        trace += buf
        target = t0 + (i + 1) / hz
        now = time.time()
        if target > now:
            time.sleep(target - now)
    frames = len(trace) // (n * step)

    with open(out, "wb") as fh:
        fh.write(b"OWPOSE1\x00")
        fh.write(struct.pack("<III", n, hz, frames))
        fh.write(bytes(trace))
    print("wrote %s: %d bones, %d frames at %d Hz (%.1f KiB)"
          % (out, n, frames, hz, len(trace) / 1024))
    if frames > 1:
        order = np.argsort(-moved)
        print("most-moved bones: %s" % ", ".join("%d(%d/%d)"
              % (int(b), int(moved[b]), frames - 1) for b in order[:10]))
        print("bones that never moved: %d of %d"
              % (int((moved == 0).sum()), n))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

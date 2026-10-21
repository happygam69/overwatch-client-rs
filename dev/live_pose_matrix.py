"""Read a live transform array: the pose, as it moves.

`dev/live_move_rows.py` found a float run that changes smoothly while the hero
gallery animates (live `0x15ca83a7180`, in the heap band where the clip buffers
live).  Its triples have length ~1.0, so it is a transform array -- most likely
the runtime *pose*, which is why the quaternion hunt never saw it.

This probe reads a window around such an address, works out the stride whose
entries have orthonormal rows, prints the entries as rows plus translation, counts
how many fit before the pattern breaks (that count should be a rig's bone count),
and re-reads to show which entries moved.

Usage:
    python live_pose_matrix.py --pid N --at 0xADDR [--span 0x4000] [--stride 0x40]

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import struct
import sys
import time

import numpy as np

import dump_process as DP


def read(handle, at, n, regions=None):
    """Read n bytes at `at`, clamped to the region that contains it."""
    if regions is None:
        regions = DP.regions(handle)
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


def orthonormal_score(a: np.ndarray) -> float:
    """Fraction of 4-float groups whose first three values are ~unit length."""
    g = a[: (len(a) // 4) * 4].reshape(-1, 4)
    n = np.linalg.norm(g[:, :3], axis=1)
    return float(np.mean(np.abs(n - 1.0) < 0.05))


def main() -> int:
    args = sys.argv[1:]

    def flag(name, default):
        if name not in args:
            return default
        v = args[args.index(name) + 1]
        return int(v, 0) if isinstance(default, int) else v

    pid = flag("--pid", 0)
    at = int(args[args.index("--at") + 1], 0) if "--at" in args else 0
    span = flag("--span", 0x4000)
    stride = flag("--stride", 0)
    dump = flag("--dump", 0)
    if not (pid and at):
        print(__doc__)
        return 2

    handle = DP.open_target(pid)
    regions = DP.regions(handle)
    data = read(handle, at, span, regions)
    if len(data) < 0x100:
        print("cannot read at 0x%x (got %d bytes)" % (at, len(data)))
        return 1
    floats = np.frombuffer(data, dtype="<f4")

    print("--- alignment search: a 64-byte bone is 3 rows carrying a translation in")
    print("    lane 4, then a 4th row whose lane 4 is exactly 0")
    best = (-1.0, 0)
    for ph in range(16):
        m = (len(floats) - ph) // 16 * 16
        if m < 64:
            continue
        g = floats[ph:ph + m].reshape(-1, 16)
        trans = np.abs(g[:, [3, 7, 11]]).sum(axis=1)
        last = np.abs(g[:, 15])
        score = float(np.mean((trans > 1e-6) & (last < 1e-6)))
        if score > best[0]:
            best = (score, ph)
    stride = 0x40
    phase = best[1]
    step = stride // 4
    start = at + phase * 4
    floats = floats[phase:]
    print("--- using 0x40-byte bones from 0x%x (%.0f%% of entries match the pattern)"
          % (start, 100.0 * best[0]))

    if dump:
        raw = read(handle, start - 0x40, dump, regions)
        f = np.frombuffer(raw, dtype="<f4")
        print("--- raw 16-byte groups from 0x%x (triple norm | 4th lane)"
              % (start - 0x40))
        for i in range(0, min(len(f), dump // 4), 4):
            tri = f[i:i + 3]
            n = float(np.linalg.norm(tri))
            mark = "*" if abs(n - 1.0) < 0.02 else " "
            print("   +0x%03x %s %+.4f %+.4f %+.4f | %+.4f   |xyz|=%.4f"
                  % (i * 4 - 0x40, mark, f[i], f[i + 1], f[i + 2], f[i + 3], n))
        return 0
    g = floats[: (len(floats) // step) * step].reshape(-1, step)

    # where does the array start?  step backwards while the rows stay unit
    def matches(entry: np.ndarray) -> bool:
        """The bone pattern: rows 0-2 carry a translation, row 3's lane 4 is 0."""
        if len(entry) < 16:
            return False
        trans = abs(float(entry[3])) + abs(float(entry[7])) + abs(float(entry[11]))
        return trans > 1e-6 and abs(float(entry[15])) < 1e-6

    back = 0
    while back < 400:
        probe = read(handle, start - (back + 1) * stride, stride, regions)
        if len(probe) != stride:
            break
        if not matches(np.frombuffer(probe, dtype="<f4")):
            break
        back += 1
    print("--- the array starts %d bones before 0x%x (0x%x); walking forward:"
          % (back, start, start - back * stride))
    fwd = 0
    while fwd < 4000:
        probe = read(handle, start + (fwd + 1) * stride, stride, regions)
        if len(probe) != stride:
            break
        if not matches(np.frombuffer(probe, dtype="<f4")):
            break
        fwd += 1
    print("    %d bones forward, so about %d in the array (a rig with %s bones?)"
          % (fwd, back + fwd + 1, "71/229"))

    live0 = read(handle, start, min(step * 40, 0x1000), regions)
    time.sleep(0.35)
    live1 = read(handle, start, min(step * 40, 0x1000), regions)
    f0 = np.frombuffer(live0, dtype="<f4")
    f1 = np.frombuffer(live1, dtype="<f4")

    print("--- the entries from the run start, with their motion")
    for i in range(min(10, len(g))):
        e = g[i]
        rows = [e[r:r + 3] for r in (0, 4, 8) if r + 3 < step]
        t = [e[r + 3] for r in (0, 4, 8) if r + 3 < step]
        tr = " ".join("%+.4f" % v for v in t)
        print("  [%2d] rows=%s  col4=(%s)" %
              (i, " | ".join(" ".join("%+.4f" % v for v in r) for r in rows), tr))
    if len(f0) == len(f1):
        n = min(len(f0), step * 38) // 4
        d = np.abs(f0[:n * 4] - f1[:n * 4]).reshape(-1, 4) if n else None
        if d is not None and len(d):
            moved = np.any(d > 1e-6, axis=1)
            print("--- over 350 ms: %d of %d groups changed (%.0f%%)"
                  % (int(moved.sum()), len(moved), 100.0 * moved.mean()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

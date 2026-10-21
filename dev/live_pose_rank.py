"""Find a *real* runtime pose: quaternion runs that move and carry varied values.

The first version of this ranked candidates by how many rows changed over 200 ms,
which picked a buffer of identical `(0.5, 0.5, 0.5, 0.5)` quaternions - 2 of 81
rows moved, and 0.5 is a value that appears in every clip, so the follow-up
correlation was meaningless. A real pose is (a) mostly moving and (b) *varied*:
an idle still has different rotations per bone. So the score is the moved fraction
times the spread of the components, which a constant buffer scores zero on.

Usage:
    python live_pose_rank.py --pid N [--min-run N] [--top N] [--spread]

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import struct
import sys
import time

import numpy as np

import dump_process as DP

SKIP = (DP.PAGE_NOACCESS | DP.PAGE_GUARD)


def unit_runs(buf: bytes, min_run: int) -> list[tuple[int, int]]:
    n = len(buf) // 4
    if n < 4 * min_run:
        return []
    a = np.frombuffer(buf, dtype="<f4", count=n)
    if len(a) < 4:
        return []
    w, x, y, z = a[:-3], a[1:-2], a[2:-1], a[3:]
    with np.errstate(over="ignore", invalid="ignore"):
        m = w * w + x * x + y * y + z * z
    ok = np.isfinite(m) & (m > 0.98) & (m < 1.02)
    idx = np.flatnonzero(ok)
    if idx.size == 0:
        return []
    breaks = np.flatnonzero(np.diff(idx) > 1)
    starts = np.concatenate(([0], breaks + 1))
    ends = np.concatenate((breaks, [idx.size - 1]))
    return [(int(idx[s]) * 4, int(e - s + 1)) for s, e in zip(starts, ends) if e - s + 1 >= min_run]


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    min_run = int(args[args.index("--min-run") + 1]) if "--min-run" in args else 40
    top = int(args[args.index("--top") + 1]) if "--top" in args else 6
    if not pid:
        print(__doc__)
        return 2
    h = DP.open_target(pid)
    print(f"pid {pid}, hunting runs of >= {min_run} unit quaternions")

    chunk = 4 << 20
    found: list[tuple[int, int]] = []
    scanned = 0
    for base, size, protect, kind, _alloc in DP.regions(h):
        if protect & SKIP or size > (512 << 20):
            continue
        off = 0
        while off < size:
            data = DP.read_region(h, base + off, min(chunk, size - off))
            if not data:
                off += chunk
                continue
            scanned += len(data)
            for at, count in unit_runs(data, min_run):
                found.append((base + off + at, count))
            off += chunk
    print(f"  {scanned/1048576:.0f} MB scanned, {len(found)} run(s)")

    scored = []
    for addr, count in found:
        span = min(16 * count, 0x8000)
        a = DP.read_region(h, addr, span)
        if not a or len(a) < 16 * min_run:
            continue
        q = np.frombuffer(a, dtype="<f4").reshape(-1, 4)
        # spread: the standard deviation of each component over the rows that are
        # actually unit quaternions. A buffer of identical quaternions scores 0,
        # which is the point — the earlier metric overflowed on non-unit rows.
        finite = np.isfinite(q).all(axis=1)
        norms = np.linalg.norm(np.where(finite[:, None], q, 0.0), axis=1)
        unit = finite & (np.abs(norms - 1.0) < 0.02)
        qq = q[unit]
        spread = float(np.std(qq, axis=0).mean()) if len(qq) else 0.0
        # rows that are not (near-)identity: a pose that is all identity is not a pose
        varied = int((np.abs(qq[:, :3]).sum(axis=1) > 0.05).sum()) if len(qq) else 0
        time.sleep(0.3)
        b = DP.read_region(h, addr, span)
        if not b or len(b) != len(a):
            continue
        rows = len(a) // 16
        moved = np.frombuffer(a, dtype=np.uint8).reshape(rows, 16) != np.frombuffer(b, dtype=np.uint8).reshape(rows, 16)
        moved_frac = float(moved.any(axis=1).mean())
        scored.append((moved_frac * spread, addr, count, moved_frac, spread, varied))
    scored.sort(reverse=True)
    print('\n  score  moved  spread  varied  address              quaternions')
    for score, addr, count, mf, sp, varied in scored[:top]:
        print('  %6.3f  %5.2f  %6.3f  %6d  0x%-18x %d' % (score, mf, sp, varied, addr, count))
    if scored:
        addr, count = scored[0][1], scored[0][2]
        data = DP.read_region(h, addr, min(16 * count, 0x400))
        print('\n  best candidate 0x%x, first quaternions:' % addr)
        for i in range(min(len(data) // 16, 6)):
            print('     (%+.4f, %+.4f, %+.4f, %+.4f)' % struct.unpack_from('<4f', data, 16 * i))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

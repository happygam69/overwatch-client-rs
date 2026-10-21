"""Find RIP-relative references from captured code to target addresses.

x64 references static data with `lea/mov r, [rip+disp32]`: target = insn_end + disp.
The displacement bytes depend on the instruction's address, so instead of a byte
pattern we try every 4-byte window as int32 and check where it would point.

Usage:
    python find_refs.py <target_hex> [target_hex ...]
"""

from __future__ import annotations

import sys

import numpy as np

import regions as R


def find_refs(space: R.Space, targets: set[int]) -> dict[int, list[int]]:
    hits: dict[int, list[int]] = {t: [] for t in targets}
    for reg in space.regions:
        data = reg.data()
        n = len(data)
        if n < 8:
            continue
        base = reg.base
        # executable-only check is done by the caller's region selection; scan all
        arr = np.frombuffer(data, dtype=np.uint8)
        for shift in range(4):
            v = np.frombuffer(data[shift:], dtype="<u4", count=(n - shift) // 4)
            offs = base + shift + 4 + (np.arange(len(v), dtype=np.uint64) * 4)
            cand = offs + v.astype(np.uint64)
            for t in targets:
                idx = np.nonzero(cand == t)[0]
                for i in idx:
                    hits[t].append(int(offs[i] - 4))
    return hits


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_exec")))
    targets = {int(a, 16) for a in sys.argv[1:]}
    hits = find_refs(space, targets)
    for t in sorted(targets):
        print(f"target {t:#x}: {len(hits[t])} ref site(s)")
        for a in hits[t][:40]:
            print(f"  from {a:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Column-consistency scanner over the FP-family clip value area.

Finds the record phase and strides of the region after the keyed-frame list:
for each candidate starts offset, slices 12-byte records and reports, per
column, (const-within-file, const-across-two-files, value shape).

Usage:
    python clip_fp_lanes.py <A> <B> [start]

Read-only.
"""

from __future__ import annotations

import struct
import sys

ROWS: int = 12


def load(path: str) -> bytes:
    with open(path, "rb") as fh:
        return fh.read()


def main() -> int:
    A = load(sys.argv[1])
    B = load(sys.argv[2])
    frames = struct.unpack_from("<H", A, 0x0C)[0]
    base = int(sys.argv[3], 0) if len(sys.argv) > 3 else 0x190
    n = len(A)
    print(f"frames {frames}, A {n} B, B {len(B)} B, region base {base:#x}")
    area = base
    for phase in range(0, ROWS):
        start = area + phase
        count = (n - start) // ROWS
        if count < 8:
            continue
        tail = (n - start) % ROWS
        a_recs = [A[start + i * ROWS : start + (i + 1) * ROWS] for i in range(count)]
        cols_same_a = [len({r[k] for r in a_recs}) == 1 for k in range(ROWS)]
        cross = [all(A[start + k + i * ROWS] == B[start + k + i * ROWS] for i in range(count)) for k in range(ROWS)]
        same = "".join("C" if s else "." for s in cols_same_a)
        identical = "".join("X" if s else "." for s in cross)
        print(f"phase +0x{phase:02x}  records {count}  tail {tail}: within-file const {same}, A==B {identical}")
        if phase == (0x19A - area) % ROWS and area <= 0x19A:
            print("  first 6 records at that phase:")
            for r in a_recs[:6]:
                print("    " + r.hex())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

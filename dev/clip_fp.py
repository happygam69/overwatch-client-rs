"""Byte-level orientation probes for the first-person clip family 0x00020400.

The family has few installed samples (7), so everything compares samples to one
another rather than statistics over a corpus:

    diff2 <A> <B>       exact byte diff: runs of differing bytes, with counts
    regions <file>      the section table + a coarse per-16-byte class map

Read-only.
"""

from __future__ import annotations

import struct
import sys


def u16(b: bytes, o: int) -> int:
    return struct.unpack_from("<H", b, o)[0]


def load(path: str) -> bytes:
    with open(path, "rb") as fh:
        return fh.read()


def diff2(a_path: str, b_path: str) -> None:
    A, B = load(a_path), load(b_path)
    n = min(len(A), len(B))
    print(f"A {a_path} {len(A)} B frames {u16(A, 0x0C)}")
    print(f"B {b_path} {len(B)} B frames {u16(B, 0x0C)}")
    runs: list[tuple[int, int, int]] = []
    i = 0
    while i < n:
        if A[i] != B[i]:
            j = i
            while j < n and A[j] != B[j]:
                j += 1
            runs.append((i, j, j - i))
            i = j
        else:
            i += 1
    print(f"identical bytes: {n - sum(r[2] for r in runs)} of {n}; run count {len(runs)}")
    total = 0
    for (start, end, length) in runs[:40]:
        total += length
        print(f"  differs 0x{start:04x}..0x{end:04x} ({length} B)")
        for off in range(start, min(end, start + 16), 8):
            a_chunk = A[off : off + 8].hex()
            b_chunk = B[off : off + 8].hex()
            print(f"      A+{off:04x}: {a_chunk}   B: {b_chunk}")
    if len(runs) > 40:
        print(f"  ... {len(runs) - 40} more runs ({sum(r[2] for r in runs[40:])} B)")


def regions(path: str) -> None:
    b = load(path)
    frames = u16(b, 0x0C)
    print(f"{path} {len(b)} B frames {frames}")
    print("  section table (ten qwords at 0x30):")
    for i in range(10):
        v = struct.unpack_from("<Q", b, 0x30 + i * 8)[0]
        mark = ""
        if v not in (0, 0xFFFFFFFFFFFFFFFF):
            mark = f"  -> u32 {v & 0xFFFFFFFF:#x} / {v >> 32:#x}"
        print(f"    [{i}] = {v:#x}{mark}")
    def cls(word: bytes) -> str:
        zeros = word.count(0)
        small = sum(1 for x in word if x < 0x20)
        if zeros == len(word):
            return "."
        if small == len(word):
            return "i"
        if zeros == 0 and all(0x20 <= x <= 0xEF for x in word):
            return "x"
        return "-"
    print("  +0x80.. classes of 16 bytes (byte-hex):")
    for base in range(0x80, len(b), 0x40):
        row = ""
        for k in range(4):
            off = base + k * 16
            row += cls(b[off : off + 16])
        print(f"  +{base:05x}  {row}")


def main() -> int:
    args = sys.argv[1:]
    if len(args) == 3 and args[0] == "diff2":
        diff2(args[1], args[2])
    elif len(args) == 2 and args[0] == "regions":
        regions(args[1])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

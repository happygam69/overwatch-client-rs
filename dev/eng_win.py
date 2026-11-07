"""Print raw disassembly windows (multiple ranges) from the engine dump."""
from __future__ import annotations
import sys

DUMP = "dev/engine_dump.asm"


def load():
    rows = []
    for line in open(DUMP, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        addr, rest = line.split("  ", 1)
        rows.append((int(addr, 16), rest.strip()))
    return rows


def main() -> int:
    rows = load()
    pairs = []
    for spec in sys.argv[1:]:
        lo, _, hi = spec.partition("-")
        pairs.append((int(lo, 16), int(hi, 16)))
    for lo, hi in pairs:
        print(f"### {lo:#x}..{hi:#x}")
        for a, t in rows:
            if lo <= a <= hi:
                print(f"{a:012x}  {t}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Sweep the capture for plaintext identifier pools.

The Domino physics pool proved the capture carries whole reflection name
tables in the clear (dmBody/dmWorld/... next to a `D:\\Madness` source path).
A pool signature: many ASCII identifiers (4+ chars, camelCase-ish) sitting
within small gaps of one another. This sweep clusters maximal ASCII runs,
scores clusters by identifier count, and flags which ones have a source-path
string nearby — the strongest hint of a reflection table vs a log-literal pile.

Usage:
    python scan_name_pools.py             # full sweep -> name_pools.txt
"""

from __future__ import annotations

import re
import sys

import regions as R

IDENT = re.compile(rb"[A-Za-z][A-Za-z0-9_]{3,60}")
PATH = re.compile(rb"[A-Z]:\\[A-Za-z0-9_.\\-]{8,}")


def scan_region(space: R.Space, reg: R.Region, out: list[str]) -> None:
    d = reg.data()
    if len(d) < 0x400:
        return
    idents = list(IDENT.finditer(d))
    if not idents:
        return
    MAX_GAP = 0x20
    clusters: list[list[int]] = []
    start = prev_end = None
    for m in idents:
        if prev_end is not None and m.start() - prev_end <= MAX_GAP:
            clusters[-1].append(m.start())
            prev_end = m.end()
            continue
        clusters.append([m.start()])
        prev_end = m.end()
    for cluster in clusters:
        if len(cluster) < 16:
            continue
        lo = cluster[0]
        hi = d.find(b"\x00", cluster[-1], cluster[-1] + 0x400)
        if hi < 0:
            hi = cluster[-1] + 96
        # sample names, require letter-heavy mix (not a table of digits)
        sample = [m.group() for m in idents if lo <= m.start() <= cluster[-1]][:8]
        letters = sum(1 for s in sample for c in s if 64 < c < 123)
        span = bytes(d[lo:hi])
        paths = [m.group() for m in PATH.finditer(span)][:2]
        # score
        out.append(
            {
                "base": reg.base + lo,
                "addr_end": reg.base + hi,
                "idents": len(cluster),
                "letters": letters,
                "first": [s.decode("ascii") for s in sample],
                "paths": [p.decode("ascii", errors="replace") for p in paths],
                "region": reg.base,
            }
        )


def main() -> int:
    space = R.Space(R.load("dumps_range"))
    out: list[dict] = []
    for reg in space.regions:
        scan_region(space, reg, out)

    def sort_key(pool: dict):
        return (-pool["idents"], -pool["letters"])

    out.sort(key=sort_key)
    writes = []
    for pool in out[:60]:
        head = " ".join(pool["first"][:6])[:100]
        writes.append(
            f"{pool['idents']:5d} ident(s) @{pool['base']:#x}..{pool['addr_end']:#x}"
            f"   first: {head}   paths: {pool['paths']}"
        )
    with open("name_pools.txt", "w", encoding="utf-8") as fh:
        fh.write("# identifier pools: count  range  first names  nearby paths\n")
        for line in writes:
            fh.write(line + "\n")
    for line in writes[:25]:
        print(line)
    print(f"\nwritten: name_pools.txt ({len(writes)} rows of {len(out)} candidates)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

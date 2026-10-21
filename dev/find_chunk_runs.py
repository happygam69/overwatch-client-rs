"""Find runs of map chunk GUIDs in a memory capture.

A map's chunks have to be listed somewhere for the game to load them, and that
list is a run of GUIDs sharing the map chunk type. A run is far more specific
than a single GUID -- one GUID appears in the manifest, but eight consecutive
ones appear only where something is enumerating a map.

    python find_chunk_runs.py [dumpdir] [--min 6]
"""

from __future__ import annotations

import sys

import regions as R

# Bytes 4..8 of a map chunk GUID, little-endian.
MARKER = bytes([0x01, 0x00, 0xD0, 0x0D])
PREFIX = 0x0DD0_0001_0000_0000
MASK = 0xFFFF_FFFF_0000_0000


def main() -> int:
    dump = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "dumps_mapped"
    minimum = 6
    if "--min" in sys.argv:
        minimum = int(sys.argv[sys.argv.index("--min") + 1])

    space = R.Space(R.load(dump))
    print(f"{dump}: {len(space.regions)} regions, runs of at least {minimum}")

    runs = []
    for region in space.regions:
        data = region.data()
        # Walk 8-byte slots and note which hold a map chunk GUID.
        hits = []
        at = 0
        while True:
            at = data.find(MARKER, at)
            if at < 0:
                break
            start = at - 4
            if start >= 0:
                value = int.from_bytes(data[start : start + 8], "little")
                if value & MASK == PREFIX:
                    hits.append(start)
            at += 1

        # Group slots that are exactly 8 bytes apart.
        run_start = None
        previous = None
        for position in hits:
            if previous is not None and position - previous == 8:
                previous = position
                continue
            if run_start is not None and previous is not None:
                length = (previous - run_start) // 8 + 1
                if length >= minimum:
                    runs.append((region.base + run_start, length, region.path))
            run_start = position
            previous = position
        if run_start is not None and previous is not None:
            length = (previous - run_start) // 8 + 1
            if length >= minimum:
                runs.append((region.base + run_start, length, region.path))

    runs.sort(key=lambda r: -r[1])
    print(f"{len(runs)} run(s)")
    for address, length, path in runs[:25]:
        print(f"  {address:#014x}  {length:>5} GUIDs  {path.split(chr(92))[-1]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

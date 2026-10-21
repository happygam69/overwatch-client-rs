"""Scan a running game's whole address space for the loaded map.

Two things could be there, and they answer different questions:

* a **decoded map chunk** -- the raw file bytes after BLTE, starting with a
  header whose third word is always 16. If it is present, the geometry is in
  memory and can be read without knowing any GUID.
* a **run of map chunk GUIDs** -- a list something is enumerating. If it is
  present, the map's chunk set is named and can be looked up in the manifest.

The earlier captures only covered mapped regions, which is where neither was
found. This walks everything committed and readable, private heap included.

    python live_mapscan.py <pid> [--min-run 6]
"""

from __future__ import annotations

import sys

import dump_process as DP
import find_mapchunks
import find_chunk_runs

MARKER = find_mapchunks.MARKER
GUID_MARKER = find_chunk_runs.MARKER
PREFIX = find_chunk_runs.PREFIX
MASK = find_chunk_runs.MASK

CHUNK = 0x4000000


def main() -> int:
    pid = int(sys.argv[1])
    minimum = 6
    if "--min-run" in sys.argv:
        minimum = int(sys.argv[sys.argv.index("--min-run") + 1])

    handle = DP.open_target(pid)
    chunks: list[tuple[int, int, int]] = []
    runs: list[tuple[int, int]] = []
    scanned = 0

    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        offset = 0
        while offset < size:
            data = DP.read_region(handle, base + offset, min(CHUNK, size - offset))
            origin = base + offset
            offset += CHUNK
            if not data:
                continue
            scanned += len(data)

            at = 0
            while True:
                at = data.find(MARKER, at)
                if at < 0:
                    break
                start = at - 8
                if start >= 0:
                    hit = find_mapchunks.looks_like_map_chunk(data, start)
                    if hit:
                        chunks.append((origin + start, hit[0], hit[1]))
                at += 1

            # A run of map chunk GUIDs, eight bytes apart.
            hits = []
            at = 0
            while True:
                at = data.find(GUID_MARKER, at)
                if at < 0:
                    break
                start = at - 4
                if start >= 0:
                    value = int.from_bytes(data[start : start + 8], "little")
                    if value & MASK == PREFIX:
                        hits.append(start)
                at += 1
            run_start = previous = None
            for position in hits:
                if previous is not None and position - previous == 8:
                    previous = position
                    continue
                if run_start is not None and previous is not None:
                    length = (previous - run_start) // 8 + 1
                    if length >= minimum:
                        runs.append((origin + run_start, length))
                run_start = previous = position
            if run_start is not None and previous is not None:
                length = (previous - run_start) // 8 + 1
                if length >= minimum:
                    runs.append((origin + run_start, length))

    print(f"scanned {scanned / 1048576:.0f} MB")
    print(f"decoded map chunks: {len(chunks)}")
    for address, count, span in chunks[:20]:
        print(f"  {address:#014x}  {count:>6} placeables  {span:>9} bytes")
    print(f"runs of {minimum}+ map chunk GUIDs: {len(runs)}")
    runs.sort(key=lambda r: -r[1])
    for address, length in runs[:20]:
        print(f"  {address:#014x}  {length:>5} GUIDs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Catch a map's chunks as the game loads them.

Map chunks do not stay in memory: the game decodes one, parses it, and frees
the buffer. A single scan therefore catches whichever chunk happened to be
live at that instant -- which is exactly what happened, one chunk out of a map.

So this polls. Every pass checks the start of every allocation, which is cheap
because a chunk gets its own allocation, and records each distinct chunk it
sees by hashing the first kilobyte. Over a few minutes of a loaded map that
collects the set.

    python watch_chunks.py <pid> [--seconds 300] [--out chunks_dir]
"""

from __future__ import annotations

import hashlib
import os
import sys
import time

import dump_process as DP
import find_mapchunks as FM


def main() -> int:
    pid = int(sys.argv[1])
    seconds = 300
    if "--seconds" in sys.argv:
        seconds = int(sys.argv[sys.argv.index("--seconds") + 1])
    out_dir = None
    if "--out" in sys.argv:
        out_dir = sys.argv[sys.argv.index("--out") + 1]
        os.makedirs(out_dir, exist_ok=True)

    handle = DP.open_target(pid)
    seen: dict[str, tuple[int, int, int]] = {}
    deadline = time.time() + seconds
    passes = 0

    while time.time() < deadline:
        passes += 1
        for base, size, protect, _kind, _alloc in DP.regions(handle):
            if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
                continue
            if size < 0x1000 or size > 0x800000:
                continue
            head = DP.read_region(handle, base, min(size, 0x2000))
            if not head:
                continue
            hit = FM.looks_like_map_chunk(head, 0)
            if not hit:
                continue
            count, span = hit
            if span > size:
                # The declared placeables run past this allocation, so it is a
                # coincidence rather than a chunk.
                continue
            digest = hashlib.sha256(head[:1024]).hexdigest()[:16]
            if digest in seen:
                continue
            seen[digest] = (base, count, span)
            print(f"  pass {passes}: {base:#014x}  {count:>5} placeables  {span:>9} bytes")
            if out_dir:
                data = DP.read_region(handle, base, span)
                if data:
                    with open(os.path.join(out_dir, f"{digest}.bin"), "wb") as fh:
                        fh.write(data)

    print(f"{passes} pass(es); {len(seen)} distinct map chunk(s)")
    for digest, (base, count, span) in sorted(seen.items(), key=lambda kv: -kv[1][2]):
        print(f"  {digest}  {base:#014x}  {count:>5} placeables  {span:>9} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

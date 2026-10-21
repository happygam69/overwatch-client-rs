"""Capture a running game and look for the map it has loaded.

Two earlier captures came back empty on every map question, and the reason was
not the method: both were taken while the game sat at the menu, and a menu
session has no decoded map chunks in it. This does the capture and the search in
one step, so the state of the game is checked rather than assumed.

    python capture_range.py [--search <text>]

Prints what it found and writes the dump to dev/dumps_range.
"""

from __future__ import annotations

import os
import subprocess
import sys

import dump_process as DP
import regions as R

import find_mapchunks

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "dumps_range")


def main() -> int:
    pids = subprocess.run(
        [sys.executable, "dump_process.py", "0", "find", "overwatch"],
        capture_output=True,
        text=True,
        cwd=os.path.dirname(os.path.abspath(__file__)),
    ).stdout.strip()
    if not pids:
        print("Overwatch is not running")
        return 1
    pid = int(pids.split()[0])
    print(f"pid {pid}")

    if "--search" in sys.argv:
        needle = sys.argv[sys.argv.index("--search") + 1]
        print(f"--- searching live memory for {needle!r}")
        subprocess.run(
            [sys.executable, "dump_process.py", str(pid), "search", needle],
            cwd=os.path.dirname(os.path.abspath(__file__)),
        )
        return 0

    print(f"--- capturing mapped regions to {OUT}")
    code = subprocess.run(
        [sys.executable, "dump_process.py", str(pid), OUT, "mapped"],
        cwd=os.path.dirname(os.path.abspath(__file__)),
    ).returncode
    if code != 0:
        return code

    print("--- looking for decoded map chunks")
    space = R.Space(R.load(OUT))
    found = []
    for region in space.regions:
        data = region.data()
        at = 0
        while True:
            at = data.find(find_mapchunks.MARKER, at)
            if at < 0:
                break
            start = at - 8
            if start >= 0:
                hit = find_mapchunks.looks_like_map_chunk(data, start)
                if hit:
                    found.append((region.base + start, hit[0], hit[1]))
            at += 1

    if not found:
        print(
            "no decoded map chunks. The game is probably not in a map --\n"
            "enter the practice range and run this again."
        )
        return 2

    print(f"{len(found)} decoded map chunk(s) in memory:")
    for address, count, span in found[:40]:
        print(f"  {address:#014x}  {count:>6} placeables  {span:>9} bytes")
    print(f"{sum(f[1] for f in found)} placeables total")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

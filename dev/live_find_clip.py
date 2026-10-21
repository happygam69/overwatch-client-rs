"""Find the client's *loaded* animation clips in memory, by their own bytes.

The gate I had been using (`anim.*` name references) turned out to be invalid: those
57 names read as developer variables, so their registration only runs when the debug
UI does. This probe does not depend on any code name at all — it looks for the clip
files themselves, whose bytes are known (the extracted corpus in `D:/ow-dumps`),
and reports where the client holds them.

A clip found in memory is the *input* to the decoder, so its address is the next
handle: the code that reads it is the decoder, and `find_referrers.py` (or a
RIP-relative scan of the executable regions) finds that code.

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.

Usage:
    python live_find_clip.py --pid N [--max-mb N] [--top N]
"""

from __future__ import annotations

import glob
import os
import sys

import dump_process as DP

SKIP = (DP.PAGE_NOACCESS | DP.PAGE_GUARD)
PATTERN = 24          # bytes taken from each clip's header


def clips() -> list[tuple[str, bytes]]:
    out = []
    for path in sorted(glob.glob("D:/ow-dumps/asset_0a*.bin")):
        with open(path, "rb") as fh:
            b = fh.read(PATTERN)
        if len(b) == PATTERN:
            out.append(("0x0a" + os.path.basename(path)[len("asset_0a"):-4], b))
    return out


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else 0
    max_mb = int(args[args.index("--max-mb") + 1]) if "--max-mb" in args else 4096
    if not pid:
        print(__doc__)
        return 2
    corpus = clips()
    print(f"{len(corpus)} clip header pattern(s) from the corpus")

    handle = DP.open_target(pid)
    chunk = 4 << 20
    budget = max_mb << 20
    scanned = 0
    found: dict[str, list[int]] = {}
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & SKIP or size > (512 << 20):
            continue
        off = 0
        while off < size and scanned < budget:
            data = DP.read_region(handle, base + off, min(chunk, size - off))
            if not data:
                off += chunk
                continue
            scanned += len(data)
            for guid, pat in corpus:
                at = data.find(pat)
                while at >= 0:
                    found.setdefault(guid, []).append(base + off + at)
                    at = data.find(pat, at + 1)
            off += chunk
        if scanned >= budget:
            break
    print(f"scanned {scanned/1048576:.0f} MB")
    if not found:
        print("no loaded clip found in the scanned range")
        return 1
    for guid, addrs in sorted(found.items(), key=lambda kv: -len(kv[1])):
        print(f"  clip {guid}  {len(addrs)} copy(ies): "
              + " ".join("0x%x" % a for a in addrs[:4]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

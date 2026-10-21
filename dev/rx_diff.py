"""Differential code-coverage: which module pages execute when a key is held.

The module maps code pages lazily: a page is PAGE_NOACCESS until it has
executed, then becomes R/RX and stays readable. So diffing the module's RX
map before and after holding a key names the exact pages that key's handling
ran in. Those pages are readable live, byte-identical to the standing
captures, and their RIP-relative operands point straight at the globals and
structures the movement code uses -- the code-first way into the player state.

Usage:
    python rx_diff.py [--pid N] [--key d] [--key space] [--out D:/ow-dumps/trace/hot]

READ-ONLY; sends keys like inject_movement.ps1.
"""

from __future__ import annotations

import os
import sys
import time

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key

MODULE_BASE = 0x7FF70E520000
MODULE_END = MODULE_BASE + 0x6000000


def rx_map(handle):
    out = []
    for base, size, protect, _kind, _alloc in DP.regions(handle):
        if not (adjust := (protect & 0xF0)):
            continue
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        if base < MODULE_END and base + size > MODULE_BASE:
            out.append((base, size, protect))
    return out


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    keys = []
    i = 0
    while "--key" in args[i:]:
        j = args.index("--key", i)
        keys.append(args[j + 1])
        i = j + 1
    if not keys:
        keys = ["d", "space", "ctrl"]
    outdir = args[args.index("--out") + 1] if "--out" in args else r"D:\ow-dumps\trace\hot"
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    os.makedirs(outdir, exist_ok=True)

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    prev = rx_map(handle)
    print(f"baseline: {len(prev)} RX region(s), "
          f"{sum(s for _, s, _ in prev)/1e6:.1f} MB")

    # baseline drift: 2s idle
    time.sleep(2.0)
    cur = rx_map(handle)
    pset = {(b, s) for b, s, _ in prev}
    drift = [(b, s, p) for b, s, p in cur if (b, s) not in pset]
    print(f"idle drift: {len(drift)} new region(s), {sum(s for _, s, _ in drift)/1e3:.0f} KB")
    prev = cur

    for key in keys:
        send_key(key, True)
        time.sleep(1.6 if key != "space" else 0.5)
        send_key(key, False)
        time.sleep(0.3)
        cur = rx_map(handle)
        pset = {(b, s) for b, s, _ in prev}
        new = [(b, s, p) for b, s, p in cur if (b, s) not in pset]
        print(f"\n=== key '{key}': {len(new)} new RX region(s), "
              f"{sum(s for _, s, _ in new)/1e3:.0f} KB")
        for b, s, p in new:
            print(f"  {b:#014x} +{s:#x} prot {p:#x}  (range addr {b - 0xC46C0000:#014x})")
        # save the new pages
        tag = key.replace("+", "_")
        for b, s, _p in new:
            blob = DP.read_region(handle, b, s)
            if blob:
                with open(os.path.join(outdir, f"{tag}_{b:#014x}_{s:#x}.bin"), "wb") as f:
                    f.write(blob)
        prev = cur
    print(f"\npages saved under {outdir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

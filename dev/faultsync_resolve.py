"""Resolve the RVA watchlist to session-absolute addresses (offline, no live touch).

Reads dev/faultsync_watchlist.txt (loader RVAs) plus a session loader base
and prints abs == base + rva lines. The pid-6000 absolute file baked in base
0x7ffc52d50000; a new session will very likely map the loader elsewhere, so
the operator run must resolve with the fresh base from the new run's
meta.json instead of reusing stale addresses.

Usage:
    python dev/faultsync_resolve.py <loader_base_hex> [watchlist]
    python dev/faultsync_resolve.py 0x7ffc52d50000 > abs_check.txt

No process touched. Exit 0 on success.
"""
from __future__ import annotations

import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: faultsync_resolve.py <loader_base_hex> [watchlist]")
        return 2
    base = int(sys.argv[1], 16)
    path = sys.argv[2] if len(sys.argv) > 2 else "dev/faultsync_watchlist.txt"
    section = ""
    for raw in open(path).read().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("["):
            section = line
            print(line)
            continue
        if line.startswith(("arm", "vp_fans", "vp_expected")):
            print(f"{line}  # base-agnostic, no resolve")
            continue
        if line.startswith(("pad_base", "pad_end", "vp_slot")):
            k, _, v = line.partition("=")
            print(f"{k.strip()}: {v.strip()} -> 0x{base + int(v.strip(), 16):x}")
            continue
        print(f"{line} -> 0x{base + int(line, 16):x}  [{section}]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

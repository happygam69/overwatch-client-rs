"""Offline fault-sync flip attribution check (no live touch).

Reads a live_page_diff run's events.csv and decides whether its flips can
even support a decrypt-routine claim. A run is ATTRIBUTABLE only if flips
carry non-zero phase markers (menu -> hero select -> in-range, Enter-marked);
all phase-0 flips are unattributable by construction.

Usage:
    python dev/faultsync_correlate.py D:/ow-dumps/live_page_diff/run_XXXX/events.csv

Exit 0 + ATTRIBUTABLE / UNATTRIBUTABLE line. No process touched.
"""
from __future__ import annotations

import csv
import collections
import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: faultsync_correlate.py <events.csv>")
        return 2
    path = sys.argv[1]
    rows = list(csv.DictReader(open(path, newline="")))
    kinds = collections.Counter(r.get("kind", "") for r in rows)
    flips = [r for r in rows if r.get("kind") == "flip"]
    by_phase = collections.Counter(r.get("phase", "?") for r in flips)
    by_mod = collections.Counter(r.get("module", "?") for r in flips)
    narx = sum(
        1 for r in flips
        if r.get("old_prot") == "00000001" and r.get("new_prot") == "00000020"
    )
    loader_flips = sum(1 for r in flips if "loader" in (r.get("module") or "").lower())
    # secondary signal: loader content-change rows (prot-unchanged, often at/near
    # .eid or cold .text pages) that coincide in time with exe flips. Informational
    # only -- never promotes a run past UNATTRIBUTABLE without phase markers.
    loader_changed = [r for r in rows
                      if r.get("kind") == "changed"
                      and "loader" in (r.get("module") or "").lower()]
    try:
        flip_ts = sorted(float(r.get("t", 0) or 0) for r in flips)
    except ValueError:
        flip_ts = []
    coincident = 0
    for r in loader_changed:
        try:
            tc = float(r.get("t", 0) or 0)
        except ValueError:
            continue
        lo, hi = tc - 1.0, tc + 1.0
        import bisect as _bisect
        i = _bisect.bisect_left(flip_ts, lo)
        if i < len(flip_ts) and flip_ts[i] <= hi:
            coincident += 1
    import collections as _collections2
    _chg_mod_rvas = _collections2.Counter(
        (r.get("addr") or "")[:10] for r in loader_changed
    )
    phases = sorted(by_phase, key=lambda p: (p != "0", p))
    print(f"events: {len(rows)} kinds={dict(kinds)}")
    print(f"flips: {len(flips)} narx={narx} loader_flips={loader_flips}")
    print(f"flips_by_phase: {dict(by_phase)}")
    print(f"flips_by_module: {dict(by_mod)}")
    print(f"loader_changed: {len(loader_changed)} flip_coincident_1s={coincident}")
    if not flips:
        print("verdict: UNATTRIBUTABLE (no flips)")
        return 0
    if set(by_phase) == {"0"}:
        print("verdict: UNATTRIBUTABLE (all flips phase 0, no markers)")
        return 0
    print("verdict: ATTRIBUTABLE-CANDIDATE (phase-marked flips present)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

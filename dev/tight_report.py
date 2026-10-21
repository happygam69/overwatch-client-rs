"""Measure the decrypt-window structure from a tight-tier capture run.

Reads a run dir's `events.csv` (the `tight`/`tightc` transition rows of
dev/live_page_diff.py v3.4) and reports, per page:
  - how many transitions were observed and in which direction,
  - the longest observed plaintext stretch (accept-or-plain -> next cipher
    state, using the `ent` value in the note field),
and one overall verdict:
  - pages that ever showed plaintext vs stayed cipher throughout,
  - the distribution of plaintext-window lengths, to answer whether the
    decrypt window is catchable by a re-read loop at all.

The per-change cadence of a chase is ~1-3 ms (scheduler round trip), so
windows longer than that are catchable; shorter ones are a falsification
of the tight tier and funnel the work to the offline-decrypt path.

Usage:
    python tight_report.py <run_dir> [--module NAME]

Read-only; prints only.
"""
from __future__ import annotations

import argparse
import csv
import os
import re
from collections import Counter, defaultdict

ENT_RE = re.compile(r"ent(-?\d+\.?\d*)")
NZ_RE = re.compile(r"nz(\d+)")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--module", default="Overwatch_loader.dll")
    args = ap.parse_args()
    ev = os.path.join(args.run_dir, "events.csv")
    if not os.path.isfile(ev):
        print(f"no events.csv in {args.run_dir}")
        return 1
    rows = []
    with open(ev, newline="", encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            if r["module"] != args.module:
                continue
            if r["kind"] not in ("tight", "tightc"):
                continue
            ent_m = ENT_RE.search(r["note"])
            nz_m = NZ_RE.search(r["note"])
            rows.append({
                "t": float(r["t"]),
                "addr": r["addr"],
                "ent": float(ent_m.group(1)) if ent_m else None,
                "nz": int(nz_m.group(1)) if nz_m else None,
                "att": r["note"].split()[0] if r["note"] else "",
            })
    if not rows:
        print("no tight rows for the module; was the tier on?")
        return 1

    by_page = defaultdict(list)
    for r in rows:
        by_page[r["addr"]].append(r)
    for addr in by_page:
        by_page[addr].sort(key=lambda x: x["t"])

    states = {}
    windows = []
    plain_pages = set()
    cipher_throughout = 0
    for addr, tr in by_page.items():
        last_plain = None
        best = 0.0
        all_cipher = True
        for r in tr:
            st = ("plain" if (r["ent"] is not None and r["ent"] <= 7.2)
                  else "cipher")
            if st == "plain":
                plain_pages.add(addr)
                all_cipher = False
                last_plain = r["t"]
            elif st == "cipher" and last_plain is not None:
                best = max(best, r["t"] - last_plain)
                last_plain = None
        states[addr] = st
        if best > 0:
            windows.append((addr, best))
        cipher_throughout += 1 if all_cipher else 0

    windows.sort(key=lambda w: -w[1])
    print(f"pages with tight rows: {len(by_page)}  "
          f"ever-plain: {len(plain_pages)}  "
          f"cipher-throughout: {cipher_throughout}")
    print("longest observed plaintext windows (ms):")
    for addr, w in windows[:15]:
        print(f"   {addr}  {w*1000:.1f} ms")
    buckets = Counter(min(int(w * 1000), 999) // 10 * 10
                      for _a, w in windows)
    print("window ms buckets:", dict(sorted(buckets.items())))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

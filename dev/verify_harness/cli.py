"""Run verification units. Exit code is the gate.

    python dev/verify_harness/cli.py fn/AnimTimeWrap
    python dev/verify_harness/cli.py casc        # only the CASC units
    python dev/verify_harness/cli.py all
    python dev/verify_harness/cli.py selftest
    python dev/verify_harness/cli.py list
"""

from __future__ import annotations

import glob
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from runner import load_unit, run_and_record  # noqa: E402

UNITS_DIR = os.path.join(os.path.dirname(__file__), "units")


def unit_paths():
    return sorted(glob.glob(os.path.join(UNITS_DIR, "*.json")))


def raw_descriptor(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def is_casc(raw) -> bool:
    """A unit is part of the CASC suite when its descriptor says so, or when
    its shipped candidate is built from `ow-casc`. Both are descriptor
    fields; the id is never parsed to decide a suite."""
    return raw.get("suite") == "casc" or raw.get("candidateCrate") == "ow-casc"


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    cmd = argv[0]

    if cmd == "selftest":
        import selftest

        return selftest.main(argv[1:])

    if cmd == "list":
        for p in unit_paths():
            try:
                u = load_unit(p)
                print(f"  {u.id}  ({os.path.basename(p)})  {u.title[:70]}")
            except ValueError as e:
                print(f"  INVALID {os.path.basename(p)}: {e}")
        return 0

    if cmd == "all":
        paths = []
        for p in unit_paths():
            raw = raw_descriptor(p)
            if raw.get("scaffold"):
                print(f"  SKIP (scaffold)  {raw.get('id', os.path.basename(p))}")
                continue
            paths.append(p)
    elif cmd == "casc":
        paths = []
        for p in unit_paths():
            raw = raw_descriptor(p)
            if raw.get("scaffold") or not is_casc(raw):
                continue
            paths.append(p)
        if not paths:
            print("no CASC units declared (suite: casc / candidateCrate: ow-casc)")
            return 2
    else:
        # Accept an id or a path.
        p = cmd if os.path.exists(cmd) else os.path.join(UNITS_DIR, cmd + ".json")
        if not os.path.exists(p):
            # also try id with slashes replaced
            alt = os.path.join(UNITS_DIR, cmd.replace("/", "-") + ".json")
            if os.path.exists(alt):
                p = alt
            else:
                print(f"no such unit: {cmd}")
                return 2
        paths = [p]

    results = []
    for p in paths:
        unit = load_unit(p)
        e = run_and_record(unit)
        results.append((unit.id, e))
        print(f"  => {unit.id}: {e.status}")
        # `all` keeps every note; a suite run prints them only for a failure,
        # where the note is the diagnosis rather than background.
        if cmd != "casc" or e.status != "verified":
            for n in e.notes:
                print(f"     note: {n}")

    print()
    ok = True
    for uid, e in results:
        if e.status != "verified":
            ok = False
        if cmd == "casc":
            mark = "PASS" if e.status == "verified" else "FAIL"
            cov = "-" if e.coverage is None else f"{e.coverage:.1f}%"
            mut = "-" if e.mutants_total == 0 else f"{e.mutants_killed}/{e.mutants_total}"
            print(f"  {mark}  {uid}  {e.status}  coverage {cov}  mutants {mut}")
        else:
            mark = "PASS" if e.status == "verified" else "FAIL"
            print(f"  {mark}  {uid}  {e.status}")
    if cmd == "casc":
        n_ok = sum(1 for _, e in results if e.status == "verified")
        print(f"  casc: {n_ok}/{len(results)} verified")
    print("gate:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

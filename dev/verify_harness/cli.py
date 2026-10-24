"""Run verification units. Exit code is the gate.

    python dev/verify_harness/cli.py fn/AnimTimeWrap
    python dev/verify_harness/cli.py all
    python dev/verify_harness/cli.py selftest
    python dev/verify_harness/cli.py list
"""

from __future__ import annotations

import glob
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from runner import load_unit, run_and_record  # noqa: E402

UNITS_DIR = os.path.join(os.path.dirname(__file__), "units")


def unit_paths():
    return sorted(glob.glob(os.path.join(UNITS_DIR, "*.json")))


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
            import json as _json

            with open(p, "r", encoding="utf-8") as f:
                raw = _json.load(f)
            if raw.get("scaffold"):
                print(f"  SKIP (scaffold)  {raw.get('id', os.path.basename(p))}")
                continue
            paths.append(p)
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

    statuses = []
    for p in paths:
        unit = load_unit(p)
        e = run_and_record(unit)
        statuses.append((unit.id, e.status))
        print(f"  => {unit.id}: {e.status}")
        for n in e.notes:
            print(f"     note: {n}")

    print()
    ok = True
    for uid, st in statuses:
        mark = "PASS" if st == "verified" else "FAIL"
        if st != "verified":
            ok = False
        print(f"  {mark}  {uid}  {st}")
    print("gate:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

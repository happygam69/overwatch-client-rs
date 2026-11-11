"""End-to-end test: run the full five-stage ladder on synthetic bytes.

Builds a tiny image containing `u64 FUN_x(u64 a, u64 b) { return a + b; }`,
a matching C decompile, and a unit descriptor, then runs the runner twice
via run_and_record (so the ledger is exercised too, redirected to a temp
file - the real ledger.json is never touched):

  - correct decompile  -> expected verdict: verified
  - wrong decompile    -> expected verdict: divergent (counterexample printed)

This exercises the real machinery end to end: load_unit validation, candidate
compilation, the determinism probe, the differential with the oracle result
cache, the coverage tracker, parallel mutant builds, and the ledger record.

Run:  python3 tests/test_run_unit_e2e.py
"""

from __future__ import annotations

import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
sys.path.insert(0, HARNESS)

import emu_oracle  # noqa: E402
import ledger  # noqa: E402
import runner as runner_mod  # noqa: E402
from runner import load_unit  # noqa: E402

BASE = 0x7FF7C4D20000

# mov rax, rcx; add rax, rdx; ret
FUNC = bytes.fromhex("4889c84801d0c3")

CORRECT_C = (
    "unsigned long long FUN_x(unsigned long long a, unsigned long long b) {\n"
    "    return a + b;\n"
    "}\n"
)

WRONG_C = (
    "unsigned long long FUN_x(unsigned long long a, unsigned long long b) {\n"
    "    return a - b;\n"
    "}\n"
)

UNIT = {
    "id": "fn/SyntheticAdd",
    "title": "e2e synthetic add",
    "address": {"v2b": hex(BASE + 0x100)},
    "entry": "FUN_x",
    "args": [
        {"name": "a", "kind": "u64"},
        {"name": "b", "kind": "u64"},
    ],
    "returns": "u64",
    "options": {
        "iterations": 24,
        "coverageThreshold": 85,
        "mutationCount": 6,
        "mutationKillRate": 0.8,
        "seed": 4241,
    },
}


def main() -> int:
    work = tempfile.mkdtemp(prefix="e2e_")
    failures = []
    try:
        img_path = os.path.join(work, "img.bin")
        import hashlib

        blob = bytearray(b"\x00" * 0x1000)
        blob[0x100 : 0x100 + len(FUNC)] = FUNC
        with open(img_path, "wb") as f:
            f.write(bytes(blob))
        # The synthetic image is not the pinned game image; register its
        # digest so the oracle accepts it without weakening the pin itself.
        emu_oracle.allow_image_sha256(hashlib.sha256(bytes(blob)).hexdigest())

        # Redirect the ledger to a temp file for this test (ledger.py
        # resolves the path at call time, so patching LEDGER_PATH is enough
        # for every load_ledger()/record() call inside the runner).
        ledger.LEDGER_PATH = os.path.join(work, "ledger.json")
        tmp_ledger = ledger.LEDGER_PATH

        for label, src, expect in (
            ("correct-decompile", CORRECT_C, "verified"),
            ("wrong-decompile", WRONG_C, "divergent"),
        ):
            c_file = os.path.join(work, f"{label}.c")
            with open(c_file, "w") as f:
                f.write(src)
            unit_json = os.path.join(work, f"unit_{label}.json")
            unit_data = dict(UNIT)
            unit_data["id"] = f"fn/SyntheticAdd-{label}"
            with open(unit_json, "w") as f:
                json.dump(unit_data, f)
            unit = load_unit(unit_json)
            unit.c_file = c_file
            unit.image = img_path

            print(f"\n--- run_and_record: {label} (expect {expect}) ---")
            e = runner_mod.run_and_record(unit, verbose=True)
            print(f"=> verdict: {e.status}")
            for n in e.notes:
                print(f"   note: {n}")
            if e.status != expect:
                failures.append(f"{label}: got {e.status}, expected {expect}")
            if expect == "divergent" and not e.divergences:
                failures.append(f"{label}: divergent without counterexamples")

        # A declaration that names a line the candidate does not have is
        # stale: the code moved under it. The unit must not verify against a
        # weaker mutant set - it says so and stops.
        stale_label = "stale-declaration"
        c_file = os.path.join(work, f"{stale_label}.c")
        with open(c_file, "w") as f:
            f.write(CORRECT_C)
        unit_json = os.path.join(work, f"unit_{stale_label}.json")
        unit_data = dict(UNIT)
        unit_data["id"] = f"fn/SyntheticAdd-{stale_label}"
        unit_data["knownEquivalentMutants"] = [
            {
                "kind": "boundary +1",
                "line": 999,
                "rationale": "a line the candidate does not have",
            }
        ]
        with open(unit_json, "w") as f:
            json.dump(unit_data, f)
        unit = load_unit(unit_json)
        unit.c_file = c_file
        unit.image = img_path
        print(f"\n--- run_and_record: {stale_label} (expect inconclusive) ---")
        e = runner_mod.run_and_record(unit, verbose=True)
        print(f"=> verdict: {e.status}")
        for n in e.notes:
            print(f"   note: {n}")
        if e.status != "inconclusive":
            failures.append(f"{stale_label}: got {e.status}, expected inconclusive")
        if not any("no longer exist" in n for n in e.notes):
            failures.append(f"{stale_label}: no stale-declaration note: {e.notes}")

        # The ledger must contain both runs, and the wrong-decompile entry
        # must not be usable as a verdict.
        entries = ledger.load_ledger(tmp_ledger)
        if len(entries) != 3:
            failures.append(f"ledger has {len(entries)} entries, expected 3")
        elif entries["fn/SyntheticAdd-wrong-decompile"].status != "divergent":
            failures.append(
                "ledger kept status "
                f"{entries['fn/SyntheticAdd-wrong-decompile'].status!r}, "
                "expected divergent"
            )
    finally:
        import shutil

        shutil.rmtree(work, ignore_errors=True)

    print()
    if failures:
        print(f"gate: FAIL {failures}")
        return 1
    print("gate: PASS - full ladder behaves correctly end to end")
    return 0


if __name__ == "__main__":
    sys.exit(main())

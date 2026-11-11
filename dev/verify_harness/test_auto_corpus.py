"""Auto-corpus growth tests: the autoCorpus option, on synthetic bytes.

Builds a tiny image containing `u64 FUN_x(u64 a) { return a == MAGIC ? 1 : 0; }`
where MAGIC is the value the seeded generator draws exactly once, at draw
index 352 (verified below against gen.py's own sequence: unique in [0, 1200),
absent from the 300-draw base corpus). The function has one conditional
branch; its not-taken edge needs exactly case 352, so:

  - base corpus (300 draws, seed 4241): branch coverage 50% < gate
  - growth draws 50 more per round: rounds at 300 and 350 cases, the second
    covers the branch -> verified, with the grown tail inside the mutation
    corpus (a constant-edit mutant of MAGIC is killed only by case 352)
  - the same unit with autoCorpus {"cap": 320} stops below the magic draw
    -> inconclusive at the coverage gate, with the realized corpus recorded

Also proves the ledger binding: a re-run of the verified unit reproduces the
same corpus digest (determinism), a replay of the fresh entry is served from
the cache, and toggling autoCorpus changes the options digest (stale verdict).

Run:  python3 test_auto_corpus.py
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = HERE
sys.path.insert(0, HARNESS)

import emu_oracle  # noqa: E402
import ledger  # noqa: E402
import runner as runner_mod  # noqa: E402
from runner import load_unit  # noqa: E402

BASE = 0x7FF7C4D20000

# The magic draw: gen.py's generate_args([u64], seed=4241, i) returns this
# value at i == 352 and at no other index in [0, 1200). Re-derived, never
# hand-picked; see the sanity check in main().
MAGIC = 0x18175951
MAGIC_DRAW_INDEX = 352
SEED = 4241
ITERATIONS = 300

# mov rax, MAGIC ; cmp rax, rcx ; jne +6 ; mov eax, 1 ; ret ; xor eax, eax ; ret
FUNC = (
    b"\x48\xb8" + MAGIC.to_bytes(8, "little")
    + b"\x48\x39\xc8"        # cmp rax, rcx
    + b"\x75\x06"            # jne -> xor path
    + b"\xb8\x01\x00\x00\x00"  # mov eax, 1
    + b"\xc3"                # ret
    + b"\x31\xc0"            # xor eax, eax
    + b"\xc3"                # ret
)

C_SOURCE = (
    "unsigned long long FUN_x(unsigned long long a) {\n"
    f"    if (a == 0x{MAGIC:x}ULL) return 1;\n"
    "    return 0;\n"
    "}\n"
)


def _unit_json(unit_id: str, auto) -> dict:
    return {
        "id": unit_id,
        "title": "auto-corpus growth synthetic unit",
        "address": {"v2b": hex(BASE + 0x100)},
        "entry": "FUN_x",
        "args": [{"name": "a", "kind": "u64"}],
        "returns": "u64",
        "options": {
            "iterations": ITERATIONS,
            "autoCorpus": auto,
            "coverageThreshold": 85,
            "mutationCount": 6,
            "mutationKillRate": 0.8,
            "seed": SEED,
        },
    }


def _make_unit(work: str, img_path: str, unit_id: str, auto):
    c_file = os.path.join(work, f"{unit_id.replace('/', '_')}.c")
    with open(c_file, "w") as f:
        f.write(C_SOURCE)
    unit_json = os.path.join(work, f"{unit_id.replace('/', '_')}.json")
    with open(unit_json, "w") as f:
        json.dump(_unit_json(unit_id, auto), f)
    unit = load_unit(unit_json)
    unit.c_file = c_file
    unit.image = img_path
    return unit


def _sanity_check_magic() -> str:
    """The MAGIC invariant the whole test stands on: the seeded generator
    draws it exactly once in [0, 1200), and not within the base corpus."""
    from gen import UnitArg, generate_args

    specs = [UnitArg("a", "u64")]
    seen = [generate_args(specs, SEED, i)[0].bits for i in range(1200)]
    if seen.count(MAGIC) != 1 or seen[MAGIC_DRAW_INDEX] != MAGIC:
        return (
            f"gen.py's draw sequence changed: 0x{MAGIC:x} appears "
            f"{seen.count(MAGIC)} time(s) in [0, 1200)"
        )
    if MAGIC in seen[:ITERATIONS]:
        return "MAGIC appears inside the base corpus; the test premise broke"
    return ""


def main() -> int:
    work = tempfile.mkdtemp(prefix="autocorpus_")
    failures = []
    try:
        img_path = os.path.join(work, "img.bin")
        blob = bytearray(b"\x00" * 0x1000)
        blob[0x100 : 0x100 + len(FUNC)] = FUNC
        with open(img_path, "wb") as f:
            f.write(bytes(blob))
        emu_oracle.allow_image_sha256(hashlib.sha256(bytes(blob)).hexdigest())
        ledger.LEDGER_PATH = os.path.join(work, "ledger.json")

        err = _sanity_check_magic()
        if err:
            failures.append(err)
            print(f"  FAIL magic sanity: {err}")

        # 1. Growth reaches the gate: verified, corpus grew past the base,
        #    and the ledger entry records the realized corpus.
        print("\n--- growth (expect verified, corpus grew) ---")
        unit = _make_unit(work, img_path, "fn/AutoGrow", True)
        e1 = runner_mod.run_and_record(unit, verbose=True)
        print(f"=> verdict: {e1.status} coverage {e1.coverage} corpus {e1.corpus_cases}")
        if e1.status != "verified":
            failures.append(f"growth: got {e1.status}, expected verified")
        if e1.corpus_cases < MAGIC_DRAW_INDEX + 1:
            failures.append(
                f"growth: corpus has {e1.corpus_cases} cases, "
                f"expected >= {MAGIC_DRAW_INDEX + 1} (the magic draw)"
            )
        if not e1.corpus_sha256:
            failures.append("growth: entry has no corpus_sha256")
        if e1.mutants_total == 0:
            failures.append("growth: mutation stage produced no mutants")

        # 2. Determinism: a forced re-run compares the identical corpus.
        print("\n--- re-run (expect the same corpus digest) ---")
        e2 = runner_mod.run_and_record(unit, verbose=False)
        if e2.status != "verified":
            failures.append(f"re-run: got {e2.status}, expected verified")
        elif e2.corpus_sha256 != e1.corpus_sha256 or e2.corpus_cases != e1.corpus_cases:
            failures.append(
                "re-run: corpus changed between identical runs "
                f"({e1.corpus_cases}/{e1.corpus_sha256[:12]} vs "
                f"{e2.corpus_cases}/{e2.corpus_sha256[:12]})"
            )

        # 3. Fresh-verdict replay: the ledger serves the verified entry.
        print("\n--- cached replay (expect 'fresh cached verdict') ---")
        e3 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
        if e3.status != "verified":
            failures.append(f"replay: got {e3.status}, expected verified")
        if not any("fresh cached verdict" in n for n in e3.notes):
            failures.append("replay: entry was not served from the cache")

        # 4. The ledger round-trips the new fields (checked before the toggle
        #    below reuses the unit id and overwrites the entry).
        print("\n--- ledger round-trip ---")
        entries = ledger.load_ledger(ledger.LEDGER_PATH)
        eg = entries.get("fn/AutoGrow")
        if eg is None:
            failures.append("ledger: fn/AutoGrow entry missing")
        elif eg.corpus_cases != e1.corpus_cases or eg.corpus_sha256 != e1.corpus_sha256:
            failures.append("ledger: corpus fields did not round-trip")
        raw = json.load(open(ledger.LEDGER_PATH))
        raw_g = next((r for r in raw["entries"] if r["unit_id"] == "fn/AutoGrow"), None)
        if raw_g is None or "corpus_sha256" not in raw_g or "corpus_cases" not in raw_g:
            failures.append("ledger: corpus fields absent from the JSON")

        # 5. Toggling autoCorpus changes the options digest: stale, re-run.
        print("\n--- autoCorpus off (expect a re-run, not a replay) ---")
        unit_off = _make_unit(work, img_path, "fn/AutoGrow", False)
        # Same unit id, different options: the recorded entry must not replay.
        e4 = runner_mod.run_and_record(unit_off, use_cache=True, verbose=False)
        if any("fresh cached verdict" in n for n in e4.notes):
            failures.append("autoCorpus toggle: stale entry was replayed")
        if e4.corpus_cases != ITERATIONS:
            failures.append(
                f"autoCorpus off: corpus has {e4.corpus_cases} cases, "
                f"expected the base {ITERATIONS}"
            )

        # 6. Cap exhaustion: growth stops below the magic draw and the
        #    coverage gate fails the unit, with the corpus recorded.
        print("\n--- cap 320 (expect inconclusive at the coverage gate) ---")
        unit_cap = _make_unit(work, img_path, "fn/AutoGrowCap", {"cap": 320})
        e5 = runner_mod.run_and_record(unit_cap, verbose=True)
        print(f"=> verdict: {e5.status} coverage {e5.coverage} corpus {e5.corpus_cases}")
        if e5.status != "inconclusive":
            failures.append(f"cap: got {e5.status}, expected inconclusive")
        if not any("coverage" in n and "gate" in n for n in e5.notes):
            failures.append(f"cap: expected a coverage-gate note, got {e5.notes}")
        if e5.corpus_cases != 320:
            failures.append(f"cap: corpus has {e5.corpus_cases} cases, expected 320")
    finally:
        import shutil

        shutil.rmtree(work, ignore_errors=True)

    print()
    if failures:
        print(f"gate: FAIL {failures}")
        return 1
    print("gate: PASS - autoCorpus growth behaves correctly")
    return 0


if __name__ == "__main__":
    sys.exit(main())

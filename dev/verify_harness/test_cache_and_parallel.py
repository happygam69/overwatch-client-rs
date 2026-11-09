"""Fresh-verdict cache, unit-level parallelism, and the ledger lock.

Extends test_run_unit_e2e.py's synthetic-image approach (no game dump, no
Windows needed; gcc required) to the three behaviours that turn `cli.py all`
into an iteration gate instead of a full re-proof:

  1. `run_and_record(..., use_cache=True)` replays a fresh `verified` ledger
     entry: no stage re-runs, the ledger keeps the original timestamp, and
     the replayed entry says it was cached. Everything else re-runs: a stale
     digest (candidate, options, surfaces), a non-`verified` verdict, or
     `use_cache=False`.
  2. Several units verify concurrently and every verdict survives the
     ledger's read-modify-write (the lock in run_and_record).
  3. The CLI drives a batch of units through the thread pool, replays them
     under `--cache`, and re-proves under `--force`.

Run:  python3 test_cache_and_parallel.py
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import sys
import tempfile
import threading
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.dirname(HERE)
sys.path.insert(0, HARNESS)

import emu_oracle  # noqa: E402
import ledger  # noqa: E402
import runner as runner_mod  # noqa: E402
import cli  # noqa: E402
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

UNIT_TEMPLATE = {
    "title": "cache/parallel synthetic",
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
        "mutationCount": 4,
        "mutationKillRate": 0.8,
        "seed": 4241,
    },
}

FAILURES = []


def check(name, cond, detail=""):
    if cond:
        print(f"  PASS {name}")
    else:
        print(f"  FAIL {name}  {detail}")
        FAILURES.append(name)


class Lab:
    """One temp workspace with a shared synthetic image and per-unit files."""

    def __init__(self):
        self.work = tempfile.mkdtemp(prefix="cachetest_")
        blob = bytearray(b"\x00" * 0x1000)
        blob[0x100 : 0x100 + len(FUNC)] = FUNC
        self.img_path = os.path.join(self.work, "img.bin")
        with open(self.img_path, "wb") as f:
            f.write(bytes(blob))
        emu_oracle.allow_image_sha256(hashlib.sha256(bytes(blob)).hexdigest())
        ledger.LEDGER_PATH = os.path.join(self.work, "ledger.json")

    def unit(self, uid, src=CORRECT_C):
        c_file = os.path.join(self.work, uid.replace("/", "_") + ".c")
        with open(c_file, "w", encoding="utf-8") as f:
            f.write(src)
        unit_json = os.path.join(self.work, uid.replace("/", "_") + ".json")
        data = dict(UNIT_TEMPLATE)
        data["id"] = uid
        # The CLI re-loads the descriptor from disk, so the synthetic image
        # (and the candidate) must be named IN the JSON, not patched onto the
        # in-memory unit afterwards.
        data["image"] = self.img_path
        data["c_file"] = c_file
        with open(unit_json, "w", encoding="utf-8") as f:
            json.dump(data, f)
        unit = load_unit(unit_json)
        return unit, unit_json

    def close(self):
        import shutil

        shutil.rmtree(self.work, ignore_errors=True)


class CountingRunner:
    """Count run_unit calls without changing what it does."""

    def __init__(self):
        self.calls = 0
        self._orig = runner_mod.run_unit

    def __enter__(self):
        orig = self._orig
        mod = runner_mod

        def counted(unit, **kw):
            self.calls += 1
            return orig(unit, **kw)

        mod.run_unit = counted
        return self

    def __exit__(self, *exc):
        runner_mod.run_unit = self._orig


def test_cache_replay():
    print("cache: a fresh verified entry is replayed, not re-run")
    lab = Lab()
    try:
        unit, _ = lab.unit("fn/SyntheticCache")
        with CountingRunner() as ctr:
            e1 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check("cache/first-run-verifies", e1.status == "verified", e1.status)
            check("cache/first-run-executes", ctr.calls == 1, f"calls={ctr.calls}")

            ts1 = ledger.load_ledger()[unit.id].timestamp
            e2 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check(
                "cache/replay-returns-verified",
                e2.status == "verified" and "fresh cached verdict" in e2.notes[0],
                f"status={e2.status} notes={e2.notes[:1]}",
            )
            check("cache/replay-does-not-execute", ctr.calls == 1, f"calls={ctr.calls}")
            ts2 = ledger.load_ledger()[unit.id].timestamp
            check(
                "cache/replay-keeps-original-timestamp",
                ts1 == ts2,
                f"{ts1} -> {ts2}",
            )

            # Edit the candidate source: the digest changes, the entry is
            # stale, the ladder re-runs.
            with open(unit.c_file, "a", encoding="utf-8") as f:
                f.write("\n/* candidate edited; the verdict must expire */\n")
            e3 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check("cache/edited-candidate-re-runs", ctr.calls == 2, f"calls={ctr.calls}")
            check("cache/edited-candidate-re-verifies", e3.status == "verified")

            # A different option is a different verdict input.
            unit.options["iterations"] = 26
            e4 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check("cache/edited-options-re-run", ctr.calls == 3, f"calls={ctr.calls}")

            # use_cache=False is the explicit re-prove path.
            e5 = runner_mod.run_and_record(unit, verbose=False)
            check("cache/no-cache-re-runs", ctr.calls == 4, f"calls={ctr.calls}")
            check("cache/no-cache-verifies", e5.status == "verified")
    finally:
        lab.close()


def test_cache_only_verified():
    print("cache: a divergent verdict is never replayed")
    lab = Lab()
    try:
        unit, _ = lab.unit("fn/SyntheticDivergent", src=WRONG_C)
        with CountingRunner() as ctr:
            e1 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check("cache/divergent-recorded", e1.status == "divergent", e1.status)
            e2 = runner_mod.run_and_record(unit, use_cache=True, verbose=False)
            check(
                "cache/divergent-re-runs",
                ctr.calls == 2 and e2.status == "divergent",
                f"calls={ctr.calls} status={e2.status}",
            )
    finally:
        lab.close()


def test_concurrent_units_keep_every_verdict():
    print("parallel: concurrent units cannot drop each other's verdicts")
    lab = Lab()
    try:
        units = [lab.unit(f"fn/SyntheticParallel{i}")[0] for i in range(8)]
        with ThreadPoolExecutor(max_workers=4) as pool:
            entries = list(
                pool.map(lambda u: runner_mod.run_and_record(u, verbose=False), units)
            )
        check(
            "parallel/all-verified",
            all(e.status == "verified" for e in entries),
            [e.status for e in entries],
        )
        saved = ledger.load_ledger()
        missing = [u.id for u in units if u.id not in saved]
        check("parallel/ledger-has-every-verdict", not missing, missing)
    finally:
        lab.close()


def test_cli_batch_cache_and_force():
    print("cli: a batch runs in parallel, --cache replays, --force re-proves")
    lab = Lab()
    try:
        paths = [lab.unit(f"fn/SyntheticBatch{i}")[1] for i in range(4)]

        out = io.StringIO()
        import contextlib

        with contextlib.redirect_stdout(out):
            rc1 = cli.main([*paths, "--unit-jobs", "3"])
        text1 = out.getvalue()
        check("cli/batch-gate-passes", rc1 == 0, f"rc={rc1}")
        check(
            "cli/batch-reports-every-unit",
            text1.count("=> fn/SyntheticBatch") == 4,
            text1,
        )
        check("cli/batch-nothing-cached", "(cached)" not in text1)
        check(
            "cli/batch-parallel-prefix",
            "[fn/SyntheticBatch" in text1,
            "expected per-unit log prefixes in parallel mode",
        )

        ts_first = {k: v.timestamp for k, v in ledger.load_ledger().items()}

        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc2 = cli.main([*paths, "--cache", "--unit-jobs", "3"])
        text2 = out.getvalue()
        check("cli/cache-gate-passes", rc2 == 0, f"rc={rc2}")
        check(
            "cli/cache-replays-every-unit",
            text2.count("(cached)") == 4,
            text2,
        )
        ts_second = {k: v.timestamp for k, v in ledger.load_ledger().items()}
        check(
            "cli/cache-does-not-rewrite-ledger",
            ts_first == ts_second,
            "a replay must not touch the ledger",
        )

        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc3 = cli.main([*paths, "--cache", "--force", "--unit-jobs", "3"])
        text3 = out.getvalue()
        check("cli/force-gate-passes", rc3 == 0, f"rc={rc3}")
        check(
            "cli/force-re-proves",
            "(cached)" not in text3 and text3.count("=> fn/SyntheticBatch") == 4,
            text3,
        )
        ts_third = {k: v.timestamp for k, v in ledger.load_ledger().items()}
        check(
            "cli/force-rewrites-ledger",
            all(ts_third[k] > ts_second[k] for k in ts_second),
            "a forced re-run must record fresh verdicts",
        )
    finally:
        lab.close()


def test_mutant_slot_knob():
    print("slots: the mutant build slot count is a knob, not a constant")
    import candidate_rust

    old = candidate_rust.MUTANT_BUILD_SLOTS
    old_env = os.environ.pop("VERIFY_MUTANT_SLOTS", None)
    try:
        candidate_rust.set_mutant_build_slots(7)
        check("slots/setter", candidate_rust._mutant_slot_count() == 7)
        os.environ["VERIFY_MUTANT_SLOTS"] = "9"
        check("slots/env-overrides", candidate_rust._mutant_slot_count() == 9)
        q9 = candidate_rust._mutant_slot_queue("slots-test-crate-a")
        check("slots/queue-uses-env-count", q9.qsize() == 9, q9.qsize())
        del os.environ["VERIFY_MUTANT_SLOTS"]
        q7 = candidate_rust._mutant_slot_queue("slots-test-crate-b")
        check("slots/queue-uses-module-count", q7.qsize() == 7, q7.qsize())
        try:
            candidate_rust.set_mutant_build_slots(0)
            check("slots/rejects-zero", False, "no ValueError raised")
        except ValueError:
            check("slots/rejects-zero", True)
    finally:
        candidate_rust.MUTANT_BUILD_SLOTS = old
        if old_env is not None:
            os.environ["VERIFY_MUTANT_SLOTS"] = old_env


def main() -> int:
    # The cli prints per-unit notes through the runner's print lock; keep the
    # test's own output readable by running the noisy parts captured.
    print("== verify_harness cache / parallel / slots tests ==")
    test_cache_replay()
    test_cache_only_verified()
    test_concurrent_units_keep_every_verdict()
    test_cli_batch_cache_and_force()
    test_mutant_slot_knob()
    print()
    if FAILURES:
        print(f"gate: FAIL {FAILURES}")
        return 1
    print("gate: PASS - cache, parallel units and slots behave as claimed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

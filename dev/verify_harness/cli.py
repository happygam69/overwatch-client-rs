"""Run verification units. Exit code is the gate.

    python dev/verify_harness/cli.py fn/AnimTimeWrap
    python dev/verify_harness/cli.py fn/BundleLookup fn/ChunkGuid   # a batch
    python dev/verify_harness/cli.py casc        # only the CASC units
    python dev/verify_harness/cli.py all
    python dev/verify_harness/cli.py selftest
    python dev/verify_harness/cli.py list

Performance knobs. None of them can skip a stage inside a run, weaken a
gate, or change a verdict - they only change *when* work runs:

    --unit-jobs N        units verified at once for all/casc/a batch
                         (default min(4, cpus); 1 = the old serial behaviour)
    --jobs N             mutant probes at once per unit (default min(4, active))
    --slots N            mutant cargo build slots per crate (default 4; env
                         VERIFY_MUTANT_SLOTS). Purely a build-throughput knob.
    --mutation-timeout S per-call budget for a mutant probe (default 10 s)

The verdict cache. A ledger entry is bound to the sha256 of the oracle
window, the candidate artifact, the options and the surfaces, under the
harness version - that is the ledger's own freshness contract. `all` and
`casc` therefore replay a `verified` entry whose digests are all unchanged
instead of re-running its five stages (the entry is reported as cached);
anything else, and every unit under `--force`, runs the full ladder. A
single named unit always re-runs: naming it is the request to re-prove it
(`--cache` opts a single unit back into the cache).
"""

from __future__ import annotations

import glob
import json
import os
import sys

sys.path.insert(0, os.path.dirname(__file__))

from runner import (  # noqa: E402
    DEFAULT_MUTANT_JOBS,
    DEFAULT_MUTATION_CALL_TIMEOUT,
    _print_line,
    load_unit,
    run_and_record,
)

UNITS_DIR = os.path.join(os.path.dirname(__file__), "units")

# Units at once for all/casc. 4 keeps a 12-core machine busy (the heavy half
# is cargo builds, which the per-crate slot pool caps independently of this).
DEFAULT_UNIT_JOBS = min(4, os.cpu_count() or 1)


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


def _pop_value(argv, flag):
    """Remove `flag VALUE` / `flag=VALUE` from argv; return (value, argv)."""
    out = []
    value = None
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == flag:
            if i + 1 >= len(argv):
                raise SystemExit(f"{flag} needs a value")
            value = argv[i + 1]
            i += 2
            continue
        if a.startswith(flag + "="):
            value = a.split("=", 1)[1]
            i += 1
            continue
        out.append(a)
        i += 1
    return value, out


def _pop_flag(argv, *flags):
    """Remove boolean flags from argv; return (seen, argv)."""
    out = [a for a in argv if a not in flags]
    return (len(out) != len(argv)), out


def _resolve_unit_path(name):
    """One command-line word -> unit descriptor path, or None."""
    p = name if os.path.exists(name) else os.path.join(UNITS_DIR, name + ".json")
    if not os.path.exists(p):
        # also try id with slashes replaced
        alt = os.path.join(UNITS_DIR, name.replace("/", "-") + ".json")
        if os.path.exists(alt):
            p = alt
        else:
            return None
    return p


def _is_cached(e) -> bool:
    return any("fresh cached verdict" in n for n in e.notes)


def main(argv):
    jobs_raw, argv = _pop_value(argv, "--jobs")
    timeout_raw, argv = _pop_value(argv, "--mutation-timeout")
    unit_jobs_raw, argv = _pop_value(argv, "--unit-jobs")
    slots_raw, argv = _pop_value(argv, "--slots")
    force, argv = _pop_flag(argv, "--force", "--no-cache")
    want_cache, argv = _pop_flag(argv, "--cache")
    try:
        jobs = int(jobs_raw) if jobs_raw is not None else DEFAULT_MUTANT_JOBS
        mutation_timeout = (
            float(timeout_raw)
            if timeout_raw is not None
            else DEFAULT_MUTATION_CALL_TIMEOUT
        )
        unit_jobs = (
            int(unit_jobs_raw) if unit_jobs_raw is not None else DEFAULT_UNIT_JOBS
        )
        slots = int(slots_raw) if slots_raw is not None else None
    except ValueError as e:
        print(f"bad performance flag value: {e}")
        return 2
    if jobs < 1:
        print("--jobs must be at least 1")
        return 2
    if unit_jobs < 1:
        print("--unit-jobs must be at least 1")
        return 2
    if slots is not None and slots < 1:
        print("--slots must be at least 1")
        return 2
    if not (mutation_timeout > 0):
        print("--mutation-timeout must be positive")
        return 2
    if slots is not None:
        import candidate_rust

        candidate_rust.set_mutant_build_slots(slots)
    run_kw = {"mutant_workers": jobs, "mutation_call_timeout": mutation_timeout}

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
        # A batch: every remaining word is a unit id or path, and a typo in
        # the second word must not be silently ignored.
        paths = []
        for name in argv:
            p = _resolve_unit_path(name)
            if p is None:
                print(f"no such unit: {name}")
                return 2
            paths.append(p)

    # Cache policy: all/casc replay fresh verified entries by default (the
    # iteration gate - unchanged units need not be re-proven every run); a
    # named unit re-runs by default (naming it is the re-prove request).
    if force:
        use_cache = False
    elif want_cache:
        use_cache = True
    else:
        use_cache = cmd in ("all", "casc")

    parallel = len(paths) > 1 and unit_jobs > 1

    def run_one(p):
        unit = load_unit(p)
        kw = dict(run_kw)
        if parallel:
            # Every line the unit's ladder prints carries its id, so a
            # terminal showing several units at once stays readable.
            kw["log_prefix"] = f"[{unit.id}] "
        e = run_and_record(unit, use_cache=use_cache, **kw)
        return unit, e

    results = [None] * len(paths)
    if parallel:
        from concurrent.futures import ThreadPoolExecutor, as_completed

        pool = ThreadPoolExecutor(max_workers=min(unit_jobs, len(paths)))
        try:
            futures = {pool.submit(run_one, p): i for i, p in enumerate(paths)}
            for fut in as_completed(futures):
                i = futures[fut]
                unit, e = fut.result()
                results[i] = (unit.id, e)
                emit_result(cmd, unit, e)
        except KeyboardInterrupt:
            # Cancel the queued units, but let the in-flight ones run to
            # completion: a half-finished unit records nothing, so every
            # verdict in the ledger stays a whole-run verdict.
            pool.shutdown(wait=False, cancel_futures=True)
            print(
                "\ninterrupted: queued units cancelled; waiting for the units "
                "in flight to finish and record"
            )
            return 130
        finally:
            pool.shutdown(wait=True)
    else:
        for i, p in enumerate(paths):
            unit, e = run_one(p)
            results[i] = (unit.id, e)
            emit_result(cmd, unit, e)

    print()
    ok = True
    n_cached = 0
    for uid, e in results:
        if e.status != "verified":
            ok = False
        if _is_cached(e):
            n_cached += 1
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
    if cmd == "all" and n_cached:
        print(f"  all: {n_cached}/{len(results)} verdict(s) replayed from the ledger")
    print("gate:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def emit_result(cmd, unit, e):
    """The `=>` line (and notes) for one finished unit, printed atomically."""
    cached = "  (cached)" if _is_cached(e) else ""
    _print_line(f"  => {unit.id}: {e.status}{cached}")
    # `all` keeps every note; a suite run prints them only for a failure,
    # where the note is the diagnosis rather than background.
    if cmd != "casc" or e.status != "verified":
        for n in e.notes:
            _print_line(f"     note: {n}")


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

"""The fast loop: prove only what a change can affect (~30 s, not an hour).

    python dev/verify_changed.py                 # what this working tree touched
    python dev/verify_changed.py --dry-run       # the plan, run nothing
    python dev/verify_changed.py --refs HEAD~1   # a commit range instead of the tree
    python dev/verify_changed.py --proofs        # every tier-B proof (slow ones need --slow)

Affected means, by construction:

| change | re-proves |
|---|---|
| `crates/<crate>/src/<f>.rs` | the tier-A units whose entry function lives in `<f>`, and that crate's tier-B proofs |
| `crates/<crate>/tests/<x>_real.rs` | that crate's proofs (a test file moves with its siblings) |
| `dev/verify_harness/{runner,emu_oracle,cli,ledger,coverage,mutator,batch,candidate_*}.py` | **everything** - these decide verdicts (with `--units-only` to narrow it) |
| `dev/verify_harness/units/<id>.json` | that one unit |
| `dev/verify_harness/<x>_fixture.py` | the units whose `argFactory` is that fixture |
| `docs/**`, `*.md` | nothing |

Tier-A units run through `dev/verify_harness/cli.py`, which replays a `verified`
entry whose digests still match, so an unrelated unit costs milliseconds. Tier-B
proofs run as `cargo test` and record their status in
`dev/verify_harness/proofs.json` - that file is what `dev/queue.py` counts.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# `candidate_rust` lives with the harness and answers "which file implements
# this entry" - the same map the runner binds a verdict to.
sys.path.insert(0, str(Path(__file__).resolve().parent / "verify_harness"))
UNITS = ROOT / "dev" / "verify_harness" / "units"
PROOFS = ROOT / "dev" / "verify_harness" / "proofs.json"

# Proofs known to take minutes; skipped unless --slow or named.
SLOW = {"index_find_real"}


def changed_paths(refs: str | None) -> list[str]:
    out: set[str] = set()
    if refs:
        # A single ref means "that ref against the working tree" only if it is a
        # path; for commits it means "... against HEAD", otherwise `--refs HEAD`
        # would diff HEAD against the tree and never be empty.
        rng = refs if ".." in refs else f"{refs}..HEAD"
        r = subprocess.run(["git", "diff", "--name-only", rng], cwd=ROOT,
                           capture_output=True, text=True)
        out.update(line.strip() for line in r.stdout.splitlines() if line.strip())
    else:
        r = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT,
                           capture_output=True, text=True)
        for line in r.stdout.splitlines():
            if len(line) > 3:
                out.add(line[3:].strip().strip('"').split(" -> ")[-1])
    return sorted(out)


def unit_crate(unit: dict) -> str | None:
    """The crate a unit's entry function lives in, by grepping for its definition."""
    entry = unit.get("entry")
    if not entry or unit.get("candidateLoader") == "c":
        return None
    # The probe dispatches on the unit's entry name and implements it as
    # `<entry>_entry`, so the symbol is neither `fn <entry>` nor `<entry>`:
    # all three shapes have to be tried (measured: the first alone found 31 of
    # the 121 units).
    for pattern in (f"\\b{entry}_entry\\b", f'"{entry}" *=>', f"fn {entry}\\b"):
        r = subprocess.run(["rg", "-l", pattern, "crates"],
                           cwd=ROOT, capture_output=True, text=True)
        for line in r.stdout.splitlines():
            parts = Path(line).parts
            if len(parts) > 1 and parts[0] == "crates":
                return parts[1]
    return None


def load_units() -> dict:
    out = {}
    for p in sorted(UNITS.glob("*.json")):
        d = json.loads(p.read_text(encoding="utf-8"))
        out[d["id"]] = {"path": p, "data": d, "crate": unit_crate(d)}
    return out


def load_proofs() -> dict:
    recorded = {}
    if PROOFS.exists():
        recorded = json.loads(PROOFS.read_text(encoding="utf-8")).get("proofs", {})
    out = {}
    for p in sorted(ROOT.glob("crates/*/tests/*_real.rs")):
        crate = p.parts[-3]
        key = f"{crate}/{p.stem}"
        out[key] = {"crate": crate, "test": p.stem, "file": p,
                    "status": (recorded.get(key) or {}).get("status")}
    return out


# The emulator, the comparator and the mutation stage decide verdicts; a change
# to one of them invalidates every verdict. A *fixture* only decides its own
# unit's inputs, a unit JSON only its own verdict, and a dev tool decides none.
VERDICT_FILES = (
    "runner.py", "emu_oracle.py", "cli.py", "ledger.py", "coverage.py",
    "mutator.py", "batch.py", "candidate_rust.py", "candidate_c.py",
)


def plan(changed: list[str], units: dict, proofs: dict, units_only: bool):
    """Which units and proofs a set of changed paths can invalidate."""
    paths = [c.replace("\\", "/") for c in changed]
    harness = [c for c in paths
               if c.startswith("dev/verify_harness/") and Path(c).name in VERDICT_FILES]
    touched_units = {Path(c).stem for c in paths
                     if c.startswith("dev/verify_harness/units/")}
    touched_fixtures = {Path(c).stem for c in paths
                        if c.startswith("dev/verify_harness/") and c.endswith(".py")}
    crates = {p.split("/")[1] for p in paths
              if p.startswith("crates/") and len(p.split("/")) > 2}
    touched_tests = {Path(p).stem for p in paths if p.startswith("crates/") and "_real.rs" in p}

    if harness:
        sel_units = set(units)
        sel_proofs = set() if units_only else set(proofs)
    else:
        # A unit's verdict is bound to *its function's own source file*, not to
        # the whole crate (`candidate_rust.candidate_binding`): a change to one
        # module must not drag the crate's sixty other units in.
        sel_units = set()
        for uid, u in units.items():
            entry = u["data"].get("entry")
            crate = u["crate"]
            if not crate or crate not in crates:
                continue
            from candidate_rust import entry_sources

            sources = {x.replace("\\", "/") for x in entry_sources(entry or "", crate)}
            if sources & set(paths) or not sources:
                sel_units.add(uid)
        # A unit JSON or a fixture names its unit: only that unit.
        for uid, u in units.items():
            stem = uid.replace("/", "-")
            data = u["data"]
            if stem in touched_units or data.get("argFactory") in touched_fixtures:
                sel_units.add(uid)
        sel_proofs = {k for k, pr in proofs.items()
                      if pr["crate"] in crates or pr["test"] in touched_tests}
    return sorted(sel_units), sorted(sel_proofs), bool(harness)


def run_units(ids: list[str]) -> tuple[int, int]:
    if not ids:
        return 0, 0
    t = time.time()
    r = subprocess.run([sys.executable, str(ROOT / "dev" / "verify_harness" / "cli.py"), *ids],
                       cwd=ROOT, capture_output=True, text=True)
    ok = r.stdout.count("PASS")
    print(f"  units: {ok}/{len(ids)} PASS in {time.time() - t:.0f}s")
    if r.returncode != 0:
        tail = [l for l in r.stdout.splitlines() if "FAIL" in l or "DIVERG" in l][:8]
        print("  " + "\n  ".join(tail) if tail else "  (see the run above)")
    return ok, len(ids)


def record_proofs(results: dict) -> None:
    data = {"version": 1, "proofs": results}
    PROOFS.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n", encoding="utf-8")


def run_proofs(keys: list[str], proofs: dict, allow_slow: bool) -> tuple[int, int]:
    recorded = {}
    if PROOFS.exists():
        recorded = json.loads(PROOFS.read_text(encoding="utf-8")).get("proofs", {})
    ok = 0
    for key in keys:
        pr = proofs[key]
        if pr["test"] in SLOW and not allow_slow:
            print(f"  proof {key}: skipped (slow; --slow to run)")
            continue
        t = time.time()
        r = subprocess.run(
            ["cargo", "test", "--release", "-p", pr["crate"], "--test", pr["test"], "--", "--nocapture"],
            cwd=ROOT, capture_output=True, text=True,
        )
        # A `*_real.rs` test that finds no install returns early and exits 0,
        # so exit code alone would record a green proof on a machine that never
        # ran one. The convention in every such test is the same line.
        blob = (r.stdout + r.stderr).lower()
        if r.returncode == 0 and ("nothing to measure" in blob or "no install" in blob):
            status = "skipped-no-install"
        else:
            status = "passed" if r.returncode == 0 else "failed"
        ok += status == "passed"
        recorded[key] = {"status": status, "seconds": round(time.time() - t, 1)}
        print(f"  proof {key}: {status} in {time.time() - t:.0f}s")
        record_proofs(recorded)
    return ok, len(keys)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--refs", default=None, help="commit range, e.g. HEAD~1")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--proofs", action="store_true", help="every tier-B proof")
    ap.add_argument("--slow", action="store_true")
    ap.add_argument("--units-only", action="store_true")
    a = ap.parse_args()

    units, proofs = load_units(), load_proofs()
    changed = changed_paths(a.refs)
    if a.proofs:
        sel_units, sel_proofs, harness = [], sorted(proofs), False
    else:
        sel_units, sel_proofs, harness = plan(changed, units, proofs, a.units_only)

    print(f"changed: {len(changed)} path(s)")
    for c in changed[:8]:
        print(f"  {c}")
    if len(changed) > 8:
        print(f"  ... {len(changed) - 8} more")
    if harness:
        print("  harness changed: every verdict depends on it")
    print(f"affected: {len(sel_units)} unit(s), {len(sel_proofs)} tier-B proof(s)")
    if a.dry_run:
        for uid in sel_units:
            print(f"  unit  {uid}")
        for key in sel_proofs:
            print(f"  proof {key}")
        return 0

    t = time.time()
    ok_u, n_u = run_units(sel_units)
    ok_p, n_p = run_proofs(sel_proofs, proofs, a.slow)
    print(f"\n{ok_u}/{n_u} units, {ok_p}/{n_p} proofs green in {time.time() - t:.0f}s")
    return 0 if ok_u == n_u and ok_p == n_p else 1


if __name__ == "__main__":
    sys.exit(main())

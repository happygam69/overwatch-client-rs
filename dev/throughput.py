"""How much verified behaviour one unit buys, and what it costs per function.

The question this answers is the one that decides the port plan: is a unit per
function the cheapest way to get functions *proven*, or is a driver trace
(`steps`) worth more per unit of work? It measures both sides rather than
arguing:

* **reach (structural, free, an UPPER BOUND):** for every unit in
  `dev/verify_harness/units/`, the transitive callee closure of its entry
  inside the gameplay scope (`corpus.db`: `edges` + `triage`). One unit whose
  closure is 30 functions could reach 30. Measured, a controlled call walks one
  path through that graph: `dev/reach.py` shows the richest units executing 1-8
  functions, and `fn/ApplyMoverSettings` executing 2 of a 491 closure. Do not
  quote this number as coverage; use it to compare entries.
* **cost (measured, `--time N`):** a fresh verification of N units, wall clock
  divided by units and by proven functions.

    python dev/throughput.py                  # structure and coverage
    python dev/throughput.py --depth 8        # deeper closures
    python dev/throughput.py --time 6         # re-prove 6 units and time them
    python dev/throughput.py --system mover   # one system

Read-only: it opens the index, the units and the ledger, and (with `--time`)
runs the harness, which records verdicts the same way a named run does.
"""
from __future__ import annotations

import json
import sqlite3
import subprocess
import sys
import time
from pathlib import Path
from typing import Dict, Iterable, List, Set, Tuple

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "dev" / "data" / "corpus" / "corpus.db"
UNITS = ROOT / "dev" / "verify_harness" / "units"
LEDGER = ROOT / "dev" / "verify_harness" / "ledger.json"
BASE = 0x140000000
V2B_BASE = 0x7FF7C4D20000


def exe_of(v2b: int) -> int:
    return v2b - V2B_BASE + BASE


class Index:
    def __init__(self) -> None:
        if not CORPUS.exists():
            sys.exit(f"no corpus index at {CORPUS}; run `python dev/corpus.py index`")
        self.c = sqlite3.connect(f"file:{CORPUS}?mode=ro", uri=True)
        # Function register: begin -> end, so a raw target maps to its chain
        # root the same way `dev/fn.py scope` maps it.
        self.begins = [r[0] for r in self.c.execute("SELECT begin FROM fn ORDER BY begin")]
        # Which functions are in the gameplay scope, and how each is ported.
        self.state: Dict[int, str] = dict(self.c.execute("SELECT begin, state FROM triage"))
        self.system: Dict[int, str] = {
            b: (s or "").split(",")[0]
            for b, s in self.c.execute("SELECT begin, systems FROM triage")
        }
        self.size: Dict[int, int] = dict(self.c.execute("SELECT begin, size FROM triage"))

    def containing(self, rva: int) -> int | None:
        """The chain root that covers an RVA (bisect on the register)."""
        import bisect

        i = bisect.bisect_right(self.begins, rva) - 1
        if i < 0:
            return None
        begin = self.begins[i]
        row = self.c.execute("SELECT end FROM fn WHERE begin=?", (begin,)).fetchone()
        if row and row[0] is not None and begin <= rva < row[0]:
            return begin
        return None

    def closure(self, root: int, depth: int) -> Dict[int, int]:
        """Callee closure of a function within `depth` levels, roots only."""
        seen = {root: 0}
        frontier = [root]
        d = 0
        while frontier and d < depth:
            d += 1
            nxt: List[int] = []
            for src in frontier:
                for (_site, target) in self.c.execute(
                    "SELECT site,target FROM edges WHERE src=?", (src,)
                ).fetchall():
                    fn = self.containing(target)
                    if fn is None or fn in seen:
                        continue
                    seen[fn] = d
                    nxt.append(fn)
            frontier = nxt
        return seen

    def bytes_of(self, roots: Iterable[int]) -> int:
        return sum(self.size.get(r, 0) for r in roots)


def load_units() -> List[dict]:
    out = []
    for p in sorted(UNITS.glob("*.json")):
        d = json.loads(p.read_text(encoding="utf-8"))
        addr = d.get("address") or {}
        v2b = addr.get("v2b")
        if not v2b:
            continue
        if d.get("scaffold"):
            continue
        out.append(
            {
                "id": d["id"],
                "path": p,
                "root": exe_of(int(str(v2b), 16)) - BASE,
                "steps": bool(d.get("steps")),
                "iterations": (d.get("options") or {}).get("iterations"),
                "crate": d.get("candidateCrate") or d.get("suite") or "",
            }
        )
    return out


def load_status() -> Dict[str, str]:
    doc = json.loads(LEDGER.read_text(encoding="utf-8"))
    return {e["unit_id"]: e["status"] for e in doc["entries"]}


def report(depth: int, system: str | None) -> int:
    idx = Index()
    units = load_units()
    status = load_status()
    for u in units:
        span = idx.closure(u["root"], depth)
        u["closure"] = span
        u["proven"] = {r for r in span if r in idx.state}
        u["verified"] = status.get(u["id"]) == "verified"

    scope = set(idx.state)
    verified = [u for u in units if u["verified"]]
    covered: Set[int] = set()
    for u in verified:
        covered |= u["proven"] | ({u["root"]} & scope)

    print(f"gameplay scope      {len(scope)} function(s)")
    print(f"units declared      {len(units)}")
    print(f"units verified      {len(verified)}")
    print(
        f"reachable (upper)   {len(covered)}  "
        f"({100.0 * len(covered) / max(1, len(scope)):.1f}% of scope, static closure)"
    )
    print()

    def rows(sel: List[dict], name: str) -> None:
        n = len(sel)
        if not n:
            return
        proven = sum(len(u["proven"]) for u in sel)
        uniq: Set[int] = set()
        for u in sel:
            uniq |= u["proven"]
        print(
            f"{name:12} units {n:4}  reach {proven:5}  unique {len(uniq):5}  "
            f"mean {proven / n:5.1f}  max {max(len(u['proven']) for u in sel):4}"
        )

    rows(verified, "verified")
    rows([u for u in verified if u["steps"]], "  steps")
    rows([u for u in verified if not u["steps"]], "  single-call")

    multi = [u for u in verified if len(u["proven"]) >= 2]
    print(
        f"\nunits whose closure is >= 2 functions: {len(multi)} of {len(verified)} "
        f"({100.0 * len(multi) / max(1, len(verified)):.0f}%)"
    )
    top = sorted(verified, key=lambda u: -len(u["proven"]))[:10]
    print("\nlargest closures among verified units:")
    for u in top:
        print(
            f"  {len(u['proven']):4} fns  {idx.bytes_of(u['proven']):7} B  "
            f"{'steps' if u['steps'] else 'one  '}  {u['id']}"
        )

    if system:
        want = {b for b, s in idx.system.items() if s == system}
        if not want:
            print(f"\nno triage row carries system {system!r}")
            return 0
        cov = covered & want
        left = want - cov
        print(f"\nsystem {system}: {len(want)} function(s), {len(cov)} proven, {len(left)} open")
        by_state: Dict[str, int] = {}
        for r in left:
            by_state[idx.state[r]] = by_state.get(idx.state[r], 0) + 1
        for k in sorted(by_state, key=lambda s: -by_state[s]):
            print(f"  open {k:8} {by_state[k]}")
    else:
        print("\nopen functions by port state:")
        by_state: Dict[str, int] = {}
        for r in scope - covered:
            by_state[idx.state[r]] = by_state.get(idx.state[r], 0) + 1
        for k in sorted(by_state, key=lambda s: -by_state[s]):
            print(f"  {k:8} {by_state[k]}")
    return 0


def timed(n: int, units: List[dict]) -> int:
    """Re-prove `n` units and report wall clock per unit and per function."""
    picks = [u["id"] for u in units if not u["steps"]][:n]
    steps = [u["id"] for u in units if u["steps"]]
    picks += steps
    print(f"re-proving {len(picks)} unit(s): {', '.join(picks)}")
    t0 = time.perf_counter()
    proc = subprocess.run(
        [sys.executable, str(ROOT / "dev" / "verify_harness" / "cli.py"), *picks],
        cwd=str(ROOT),
    )
    dt = time.perf_counter() - t0
    idx = Index()
    status = load_status()
    proven = 0
    verified = 0
    for u in units:
        if u["id"] not in picks:
            continue
        if status.get(u["id"]) == "verified":
            verified += 1
            proven += len({r for r in idx.closure(u["root"], 6) if r in idx.state})
    print()
    print(f"wall clock          {dt:.1f} s for {len(picks)} unit(s)  ({dt / len(picks):.1f} s/unit)")
    if proven:
        print(
            f"proven              {proven} function(s)  ->  {dt / proven:.1f} s per proven "
            f"function, {proven / (dt / 3600.0):.0f} functions/hour"
        )
    print(f"cli exit            {proc.returncode}")
    return proc.returncode


def main(argv: List[str]) -> int:
    depth = 6
    system = None
    do_time = 0
    i = 1
    while i < len(argv):
        a = argv[i]
        if a == "--depth":
            depth = int(argv[i + 1])
            i += 2
        elif a == "--system":
            system = argv[i + 1]
            i += 2
        elif a == "--time":
            do_time = int(argv[i + 1])
            i += 2
        else:
            print(__doc__)
            return 2
    rc = report(depth, system)
    if do_time:
        rc |= timed(do_time, load_units())
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv))

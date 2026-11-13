"""Which client functions a unit's differential actually executes.

`dev/throughput.py` measures the *static* callee closure of a unit's entry -
an upper bound, because one call does not necessarily walk the whole graph.
This measures the other side: every instruction the emulator executes during
the unit's own corpus, folded up to chain-root functions, and reported against
the gameplay scope.

    python dev/reach.py fn/ApplyMoverSettings
    python dev/reach.py fn/ApplyMoverSettings fn/StateTransform --cases 40
    python dev/reach.py fn/ApplyMoverSettings --quiet --sum

`UC_HOOK_CODE` runs on every instruction, so emulation is several times slower
than a plain differential. This is a measurement tool, not part of the gate.

Read-only: the image, the index, the units. It records no verdicts.
"""
from __future__ import annotations

import importlib
import sqlite3
import sys
from pathlib import Path
from typing import Dict, List, Set

sys.path.insert(0, str(Path(__file__).resolve().parent / "verify_harness"))

import runner  # noqa: E402
from emu_oracle import EmuOracle  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "dev" / "data" / "corpus" / "corpus.db"


class FnMap:
    """Executed image offset -> chain-root function, via the corpus register."""

    def __init__(self) -> None:
        self.c = sqlite3.connect(f"file:{CORPUS}?mode=ro", uri=True)
        rows = list(self.c.execute("SELECT begin,end FROM fn ORDER BY begin"))
        self.begins = [r[0] for r in rows]
        self.ends = [r[1] if r[1] is not None else r[0] for r in rows]
        self.state: Dict[int, str] = dict(self.c.execute("SELECT begin,state FROM triage"))
        self.size: Dict[int, int] = dict(self.c.execute("SELECT begin,size FROM triage"))

    def closure(self, root: int, depth: int = 6) -> Set[int]:
        seen = {root}
        frontier = [root]
        d = 0
        while frontier and d < depth:
            d += 1
            nxt = []
            for src in frontier:
                for (target,) in self.c.execute(
                    "SELECT target FROM edges WHERE src=?", (src,)
                ).fetchall():
                    fn = self.root(target)
                    if fn is not None and fn not in seen:
                        seen.add(fn)
                        nxt.append(fn)
            frontier = nxt
        return {r for r in seen if r in self.state}

    def callees(self, root: int) -> Set[int]:
        out = set()
        for (target,) in self.c.execute(
            "SELECT target FROM edges WHERE src=?", (root,)
        ).fetchall():
            fn = self.root(target)
            if fn is not None and fn in self.state:
                out.add(fn)
        return out

    def root(self, rva: int) -> int | None:
        import bisect

        i = bisect.bisect_right(self.begins, rva) - 1
        if i >= 0 and rva < self.ends[i]:
            return self.begins[i]
        return None


def unit_executed(unit, fmap: FnMap, cases: int) -> Set[int]:
    """Run the unit's own case factory and collect the executed function roots."""
    o = EmuOracle(unit.image, base=unit.base)
    if unit.state is not None:
        o.set_state(unit.state.base, unit.state.data)
    o.persist_state = bool(unit.steps)
    seen = bytearray(len(o.image))
    base = o.base

    def on_insn(_uu, addr, _size, _ud):
        off = addr - base
        if 0 <= off < len(seen):
            seen[off] = 1

    o.uc.hook_add(UC_HOOK_CODE, on_insn)
    mod = importlib.import_module(unit.arg_factory)
    if unit.steps:
        for i in range(cases):
            if unit.state is not None:
                o.set_state(unit.state.base, unit.state.data)
            for args in mod.make_steps(i):
                o.call(unit.address_v2b, args, ret_kind=unit.ret_kind)
    else:
        for i in range(cases):
            o.call(unit.address_v2b, mod.make_args(i), ret_kind=unit.ret_kind)
    roots: Set[int] = set()
    for off, hit in enumerate(seen):
        if hit:
            r = fmap.root(off)
            if r is not None and r in fmap.state:
                roots.add(r)
    return roots


def unit_root(unit, fmap: FnMap) -> int:
    return unit.address_v2b - unit.base


def all_verified(fmap: FnMap, top: bool, limit: int) -> List[str]:
    import json

    ledger = json.loads(
        (ROOT / "dev" / "verify_harness" / "ledger.json").read_text(encoding="utf-8")
    )
    ok = {e["unit_id"] for e in ledger["entries"] if e["status"] == "verified"}
    paths = sorted((ROOT / "dev" / "verify_harness" / "units").glob("*.json"))
    rows = []
    for p in paths:
        uid = json.loads(p.read_text(encoding="utf-8"))["id"]
        if uid not in ok or uid.endswith("-state"):
            continue
        unit = runner.load_unit(str(p))
        root = unit_root(unit, fmap)
        span = fmap.closure(root) if root in fmap.state else {root}
        rows.append((len(span), uid))
    rows.sort(reverse=top)
    return [uid for _n, uid in rows[:limit]]


def main(argv: List[str]) -> int:
    cases = 20
    quiet = False
    top = True
    limit = 0
    ids: List[str] = []
    i = 1
    while i < len(argv):
        a = argv[i]
        if a == "--cases":
            cases = int(argv[i + 1])
            i += 2
        elif a == "--quiet":
            quiet = True
            i += 1
        elif a == "--all-verified":
            i += 1
        elif a == "--smallest":
            top = False
            i += 1
        elif a == "--limit":
            limit = int(argv[i + 1])
            i += 2
        else:
            ids.append(a)
            i += 1
    fmap = FnMap()
    if not ids:
        ids = all_verified(fmap, top, limit or 12)
    if not ids:
        print(__doc__)
        return 2
    total: Set[int] = set()
    if not quiet:
        print(f"{'unit':42} {'exec':>5} {'bytes':>8} {'steps':>6}")
    for uid in ids:
        p = ROOT / "dev" / "verify_harness" / "units" / (uid.replace("/", "-") + ".json")
        unit = runner.load_unit(str(p))
        roots = unit_executed(unit, fmap, cases)
        total |= roots
        if not quiet:
            print(
                f"{uid:42} {len(roots):5} {sum(fmap.size.get(r, 0) for r in roots):8} "
                f"{'yes' if unit.steps else 'no':>6}"
            )
    print(
        f"{'union over ' + str(len(ids)) + ' unit(s)':42} {len(total):5} "
        f"{sum(fmap.size.get(r, 0) for r in total):8}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

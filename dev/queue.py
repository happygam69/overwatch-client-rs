"""The work queue and the progress number, by tier (`docs/tiers.md`).

Answers three questions, and nothing else:

    python dev/queue.py                 # progress: how much is done, per tier
    python dev/queue.py --next 10       # what to port next, screened
    python dev/queue.py --assign-tiers  # the system -> tier table (prints the rule)

**Progress is counted per function, not per unit.** A function counts as done
when it is *cited* by a proof that is green, and by nothing less:

- tier A: the function has a `verified` ledger entry (`dev/verify_harness`).
- tier B: the function is cited by a `*_real.rs` integration test that passed
  against the install (`dev/verify_harness/proofs.json`).
- tier C: a recorded run is linked - no function has one yet, so the column is
  zero by measurement, not by omission.

**Tiers are derived from the system tag**, which the corpus triage wrote; see
`docs/tiers.md`. A function with no tag is `unclassified` and is not counted as
done at any tier. The triage scope is 3,875 of the image's 119,802 roots; the
rest have no system tag yet, and the number says so rather than guessing.

Read-only: the corpus DB is opened `mode=ro`.
"""
from __future__ import annotations

import collections
import glob
import json
import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "dev" / "data" / "corpus" / "corpus.db"
UNITS = ROOT / "dev" / "verify_harness" / "units"
LEDGER = ROOT / "dev" / "verify_harness" / "ledger.json"
PROOFS = ROOT / "dev" / "verify_harness" / "proofs.json"
V2B = 0x7FF7C4D20000
BASE = 0x140000000

# A function with no system tag inherits the tier of the crate whose proof
# exercises it. This is a stated rule, not a guess: the crate is where the port
# lives, and the three crates split exactly along the tier lines.
CRATE_TIER = {"ow-sim": "A", "ow-engine": "A", "ow-tank": "B", "ow-casc": "B",
              "ow-stu": "B", "ow-client": "C", "ow-scene": "C", "ow-render": "C"}

# System tag -> tier. A: the player feels it. B: its product is data. C: the
# engine's structure. Anything unlisted stays "unclassified".
SYSTEM_TIERS = {
    "A": ("mover", "camera", "collision", "weapon", "ability", "hero", "input",
          "physics", "statescript", "domino"),
    "B": ("asset", "casc", "stu", "bank", "map", "effect", "sound", "texture",
          "anim", "registry", "locale", "model", "material", "chunk"),
    "C": ("frame", "world", "entity", "render", "net", "engine"),
}
TIER_ORDER = {"A": 0, "B": 1, "C": 2, "unclassified": 3}
DONE = {"A": "verified", "B": "proved", "C": "linked"}


def tier_of(systems: str | None) -> str:
    """The tier a system tag list belongs to. Mixed tags take the lowest letter."""
    if not systems:
        return "unclassified"
    tags = {t.strip() for t in systems.split(",") if t.strip()}
    for tier in ("A", "B", "C"):
        if tags & set(SYSTEM_TIERS[tier]):
            return tier
    return "unclassified"


def load_index():
    c = sqlite3.connect(f"file:{CORPUS}?mode=ro", uri=True)
    rows = list(c.execute("SELECT begin, size, systems, name, state FROM triage"))
    funcs = {r[0]: {"size": r[1], "systems": r[2], "name": r[3], "state": r[4]} for r in rows}
    roots = c.execute("SELECT count(*) FROM fn").fetchone()[0]
    callers = collections.Counter()
    for (t,) in c.execute("SELECT target FROM edges"):
        callers[t] += 1
    return funcs, roots, callers


def covers_of_test(path: Path) -> set[int]:
    """Client functions a proof declares, from `proof-covers:` header lines.

    A green test proves whatever it exercises; this declares *which client
    functions* that is, so the progress number can count them. Absent the
    header, the proof still counts as green but contributes no coverage - that
    is the honest state, and `dev/queue.py` reports how many proofs still need
    one.
    """
    out: set[int] = set()
    for line in path.read_text(encoding="utf-8", errors="ignore").splitlines()[:40]:
        if not line.lstrip().startswith("//"):
            continue
        if "proof-covers:" not in line:
            continue
        for m in ADDR.finditer(line):
            out.add(int(m.group(0), 16) - BASE)
    return out


def crate_of_entry(entry: str | None) -> str | None:
    """The crate a unit's entry function is defined in (by grep, once per name)."""
    if not entry:
        return None
    import subprocess
    # The probe symbol is `<entry>_entry` and the dispatch arm is `"<entry>" =>`,
    # so all three shapes are tried (the first alone found 31 of 121 units).
    for pattern in (f"\\b{entry}_entry\\b", f'"{entry}" *=>', f"fn {entry}\\b"):
        r = subprocess.run(["rg", "-l", pattern, "crates"], cwd=ROOT,
                           capture_output=True, text=True)
        for line in r.stdout.splitlines():
            parts = Path(line).parts
            if len(parts) > 1 and parts[0] == "crates":
                return parts[1]
    return None


def load_units():
    """unit id -> (function RVA, ledger status)."""
    status = {}
    if LEDGER.exists():
        for e in json.loads(LEDGER.read_text(encoding="utf-8")).get("entries", []):
            status[e["unit_id"]] = e.get("status")
    out = {}
    for p in glob.glob(str(UNITS / "*.json")):
        d = json.loads(Path(p).read_text(encoding="utf-8"))
        v2b = (d.get("address") or {}).get("v2b")
        if not v2b:
            continue
        out[d["id"]] = {
            "fn": int(str(v2b), 16) - V2B,
            "status": status.get(d["id"]),
            "crate": crate_of_entry(d.get("entry")),
            "sources": d.get("candidateSources") or [],
        }
    return out


ADDR = re.compile(r"0x14[0-9a-fA-F]{7}")


def cite_of_test(path: Path) -> set[int]:
    """Function RVAs a test file cites by literal address."""
    out = set()
    for m in ADDR.finditer(path.read_text(encoding="utf-8", errors="ignore")):
        rva = int(m.group(0), 16) - BASE
        if 0 <= rva:
            out.add(rva)
    return out


def load_proofs():
    """Tier-B proofs: every `*_real.rs` test, with the status last recorded."""
    recorded = {}
    if PROOFS.exists():
        recorded = json.loads(PROOFS.read_text(encoding="utf-8")).get("proofs", {})
    out = {}
    for p in sorted(ROOT.glob("crates/*/tests/*_real.rs")):
        crate = p.parts[-3]
        name = p.stem
        key = f"{crate}/{name}"
        out[key] = {
            "crate": crate,
            "test": name,
            "file": str(p.relative_to(ROOT)).replace("\\", "/"),
            "cites": cite_of_test(p),
            "covers": covers_of_test(p),
            "status": (recorded.get(key) or {}).get("status"),
        }
    return out


def universe(funcs, units, proofs) -> dict:
    """Every function a claim could be made about, with its tier.

    The triage scope plus anything a unit or proof cites. A cited function with
    no system tag takes the tier of the crate that proves it (`CRATE_TIER`).
    """
    uni = {}
    for rva, f in funcs.items():
        uni[rva] = {"tier": tier_of(f["systems"]), "size": f["size"], "name": f["name"],
                    "systems": f["systems"], "in_scope": True}
    for u in units.values():
        rva = u["fn"]
        if rva in uni:
            continue
        # No system tag, so the tier comes from the crate that proves it. No
        # default: a unit whose crate is unknown (a C-loader control unit) stays
        # unclassified, because guessing "A" would promote data code to the
        # emulator tier - the exact error this file exists to avoid.
        tier = CRATE_TIER.get(u.get("crate") or "", "unclassified")
        uni[rva] = {"tier": tier,
                    "size": 0, "name": None, "systems": None, "in_scope": False}
    for pr in proofs.values():
        for rva in pr["covers"]:
            if rva in uni:
                continue
            uni[rva] = {"tier": CRATE_TIER.get(pr["crate"], "unclassified"),
                        "size": 0, "name": None, "systems": None, "in_scope": False}
    return uni


def progress(funcs, roots, units, proofs) -> dict:
    uni = universe(funcs, units, proofs)
    done: dict[str, set] = {t: set() for t in ("A", "B", "C", "unclassified")}
    for u in units.values():
        if u["status"] == "verified":
            tier = uni[u["fn"]]["tier"]
            if tier != "unclassified":  # an unclassified function is not done
                done[tier].add(u["fn"])
    for pr in proofs.values():
        if pr["status"] != "passed":
            continue
        for rva in pr["covers"]:
            # A tier-B proof credits tier-B functions only. Crediting a
            # tier-A function from a data test would let it reach `verified`
            # without a single emulator case - the loophole `docs/tiers.md`
            # denies.
            if rva in uni and uni[rva]["tier"] == "B":
                done["B"].add(rva)
    per = {}
    for tier in ("A", "B", "C", "unclassified"):
        members = {r for r, f in uni.items() if f["tier"] == tier}
        scope = {r for r in members if uni[r]["in_scope"]}
        per[tier] = {"members": len(members), "scope": len(scope),
                     "done": len(members & done[tier])}
    per["_roots"] = roots
    per["_uni"] = len(uni)
    per["_scope"] = len(funcs)
    return per


def cmd_progress(funcs, roots, units, proofs) -> int:
    per = progress(funcs, roots, units, proofs)
    print()
    print(f"triage scope {len(funcs)}   cited {per['_uni']}   image {roots} roots")
    print(f"  {'tier':14} {'what proves it':22} {'known':>7} {'done':>6} {'share':>7}")
    labels = {"A": "emulator, bit-exact", "B": "install data, exact",
              "C": "trace, first diff", "unclassified": "-"}
    for tier in ("A", "B", "C", "unclassified"):
        p = per[tier]
        share = 100.0 * p["done"] / p["members"] if p["members"] else 0.0
        print(f"  {tier:14} {labels[tier]:22} {p['members']:7} {p['done']:6} {share:6.1f}%")
    total = sum(per[t]["done"] for t in ("A", "B", "C"))
    print()
    print(f"  done at any tier: {total} of {per['_uni']} known functions"
          f"   ({100.0 * total / max(1, per['_uni']):.1f}%)")
    print(f"  not queued: {roots - len(funcs)} image roots have no triage tag")
    other = collections.Counter(u["status"] for u in units.values())
    print()
    print(f"  units: {len(units)}  " + "  ".join(
        f"{k or 'unrun'} {v}" for k, v in other.most_common()))
    passing = sum(1 for p in proofs.values() if p["status"] == "passed")
    need = sum(1 for p in proofs.values() if not p["covers"])
    print(f"  tier-B proofs: {len(proofs)} real-data tests, {passing} passing,"
          f" {need} without a `proof-covers:` header")
    return 0


def cmd_next(funcs, units, proofs, count: int, scan: int = 400) -> int:
    """The next functions to port: **portable ones first**, then the blocked.

    The screener (`dev/cluster.py`) is the point: a function that cannot be
    proved must not reach the top of the queue. Each candidate costs a
    classification, so the scan is bounded - and what it did not reach is said
    out loud rather than implied.
    """
    done_fns = {u["fn"] for u in units.values()}
    for pr in proofs.values():
        done_fns |= pr["covers"]
    cands = [
        (rva, f) for rva, f in funcs.items()
        if rva not in done_fns and f["size"] and f["state"] in ("unit", "entry")
    ]
    if not cands:
        print("nothing left to queue in the triage scope")
        return 0
    cands.sort(key=lambda c: (TIER_ORDER[tier_of(c[1]["systems"])], c[1]["size"]))
    sys.path.insert(0, str(ROOT / "dev"))
    from cluster import Index

    idx = Index()
    portable: list = []
    blocked: list = []
    scanned = 0
    for rva, f in cands:
        if len(portable) >= count or scanned >= scan:
            break
        scanned += 1
        verdict, why = idx.classify(rva)
        (portable if verdict == "ok" else blocked).append((rva, f, why))

    shown = 0
    for rva, f, _why in portable[:count]:
        tier = tier_of(f["systems"])
        print(f"  {BASE + rva:#x}  PORTABLE  tier {tier:<10} {f['size']:5}B  "
              f"{(f['name'] or '')[:22]:22} {(f['systems'] or '')[:24]}")
        shown += 1
    short = count - shown
    if short > 0 and blocked:
        print()
        print(f"  {short} more needed; the next blocked ones, with the reason:")
        for rva, f, why in blocked[:short]:
            tier = tier_of(f["systems"])
            print(f"  {BASE + rva:#x}  blocked   tier {tier:<10} {f['size']:5}B  "
                  f"{(f['name'] or '')[:22]:22}")
            print(f"      {why}")
    print()
    print(f"  scanned {scanned} of {len(cands)} candidates; "
          f"{len(portable)} portable in that window")
    return 0


def cmd_assign(funcs) -> int:
    per = collections.Counter()
    for f in funcs.values():
        per[(tier_of(f["systems"]), f["systems"] or "(untagged)")] += 1
    print(f"  {'tier':14} {'system tag':44} {'functions':>9}")
    for (tier, systems), n in sorted(per.items(), key=lambda kv: (TIER_ORDER[kv[0][0]], -kv[1])):
        print(f"  {tier:14} {systems[:44]:44} {n:9}")
    return 0


def main(argv) -> int:
    count = 10
    mode = "progress"
    if "--next" in argv:
        mode = "next"
        i = argv.index("--next")
        if i + 1 < len(argv) and argv[i + 1].isdigit():
            count = int(argv[i + 1])
    elif "--assign-tiers" in argv:
        mode = "assign"
    funcs, roots, _callers = load_index()
    units, proofs = load_units(), load_proofs()
    if mode == "progress":
        return cmd_progress(funcs, roots, units, proofs)
    if mode == "next":
        return cmd_next(funcs, units, proofs, count)
    return cmd_assign(funcs)


if __name__ == "__main__":
    sys.exit(main(sys.argv))

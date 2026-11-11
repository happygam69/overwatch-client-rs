"""Which client functions does a crate implement, and in what state.

    python dev/crate_coverage.py ow-casc
    python dev/crate_coverage.py ow-tank
    python dev/crate_coverage.py all --queue     # the next units, playbook order

Answers "find all the functions this crate is about" **by computation, not by a
hand list**: every `0x14xxxxxxx` address cited in `crates/<crate>/**/*.rs` is
resolved against the image's function register and the harness units.

States, in the order they are reported:

  verified    a `dev/verify_harness/units/fn-*.json` unit runs that address
              (the unit's v2b address converted to the exe frame)
  entry       the image's `.pdata` register has a function at that address
  interior    the address is inside a registered function's extent (a chained
              fragment, or a named leaf with no `.pdata` record of its own)
  code        a `.text` address with no `.pdata` extent and no owner
  data        outside `.text` (a table, vtable or constant), with `zero-fill`
              marked when it sits past the section's raw bytes (BSS)

`--queue` prints the unverified function entries in the port playbook's order
(`docs/port-playbook.md` section 1): most already-verified callees first, then
smallest body, so the next unit grows a proved cluster instead of starting one.

Reads: the recovered image, `dev/data/corpus/corpus.db` (the function and edge
index), `docs/function-names.csv`, the unit descriptors. Writes nothing.
"""

from __future__ import annotations

import argparse
import bisect
import csv
import json
import pathlib
import re
import sqlite3
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
IMAGE = ROOT / "dev/data/offline_recover/module_offline.bin"
CORPUS = ROOT / "dev/data/corpus/corpus.db"
NAMES = ROOT / "docs/function-names.csv"
UNITS = ROOT / "dev/verify_harness/units"
BASE = 0x140000000
# `runner.py` states this delta: v2b - 0x7ff7c4d20000 == exe - 0x140000000.
V2B_DELTA = 0x7FF7C4D20000 - BASE

ADDR_RE = re.compile(r"\b(0x14[0-9a-fA-F]{7})\b")
# The same address written the way a human writes it when the Ghidra name is
# in the prose: `FUN_1421d88c0`. 16 of `ow-tank`'s citations are in this form
# only (the effect builder `FUN_142e4a780`, the STU loader `FUN_1421d88c0`,
# the material job registration `FUN_141b19160`), and a coverage report that
# misses them understates what the crate is about.
FUN_RE = re.compile(r"\bFUN_(1[0-9a-f]{8})\b")


def image_bytes() -> bytes:
    if not IMAGE.exists():
        raise SystemExit(f"recovered image missing: {IMAGE}")
    return IMAGE.read_bytes()


def sections(image: bytes) -> list[dict]:
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = image[o : o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", image, o + 8)
        out.append(
            {
                "name": name,
                "va": va,
                "vsz": vsz,
                "raw_end": va + rsz,
                "end": va + max(vsz, rsz),
            }
        )
    return out


def section_of(secs: list[dict], rva: int) -> dict | None:
    for s in secs:
        if s["va"] <= rva < s["end"]:
            return s
    return None


def function_register(con: sqlite3.Connection) -> dict[int, tuple[int, int]]:
    """rva -> (begin rva, end rva) for every registered function."""
    return {
        begin: (begin, end)
        for begin, end in con.execute("SELECT begin,end FROM fn")
    }


def owner_of(register: dict[int, tuple[int, int]], begins: list[int], rva: int) -> int | None:
    """The registered function whose extent contains `rva`, if any."""
    i = bisect.bisect_right(begins, rva) - 1
    if i < 0:
        return None
    begin = begins[i]
    _b, end = register[begin]
    return begin if rva < end else None


def names_table() -> dict[int, str]:
    out: dict[int, str] = {}
    with NAMES.open(newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            try:
                exe = int(row["address"], 16)
            except (TypeError, ValueError):
                continue
            name = row.get("name") or ""
            if name:
                out.setdefault(exe, name)
    return out


def unit_addresses() -> dict[int, list[str]]:
    """exe address -> unit ids that run it."""
    out: dict[int, list[str]] = {}
    for path in sorted(UNITS.glob("fn-*.json")):
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            continue
        addr = raw.get("address") or {}
        if addr.get("v2b"):
            exe = int(addr["v2b"], 16) - V2B_DELTA
        elif addr.get("exe"):
            # A unit that runs the image in its own frame (`base: 0x140000000`)
            # names the link address directly.
            exe = int(addr["exe"], 16)
        else:
            continue
        out.setdefault(exe, []).append(raw.get("id", path.stem))
    return out


def cited(crate: str) -> dict[int, set[str]]:
    """exe address -> the crate files that cite it."""
    root = ROOT / "crates" / crate
    if not root.is_dir():
        raise SystemExit(f"no such crate: {root}")
    out: dict[int, set[str]] = {}
    for path in sorted(root.rglob("*.rs")):
        text = path.read_text(encoding="utf-8", errors="replace")
        hits = {int(h, 16) for h in ADDR_RE.findall(text)}
        hits |= {int(h, 16) for h in FUN_RE.findall(text)}
        for hit in hits:
            out.setdefault(hit, set()).add(
                str(path.relative_to(ROOT)).replace("\\", "/")
            )
    return out


def callees(con: sqlite3.Connection) -> dict[int, set[int]]:
    out: dict[int, set[int]] = {}
    for src, target in con.execute(
        "SELECT src,target FROM edges WHERE form IN ('call','jmp','jcc')"
    ):
        if src == target:
            continue
        out.setdefault(src + BASE, set()).add(target + BASE)
    return out


def classify(exe: int, register, begins, secs, units) -> tuple[str, str]:
    rva = exe - BASE
    if exe in units:
        return "verified", ""
    if rva in register:
        return "entry", ""
    owner = owner_of(register, begins, rva)
    if owner is not None:
        return "interior", f"inside {owner + BASE:#x}"
    sec = section_of(secs, rva)
    if sec is None:
        if secs and rva < secs[0]["va"]:
            return "header", "PE headers (not code)"
        return "unmapped", ""
    tag = "zero-fill" if rva >= sec["raw_end"] else "raw"
    name = sec["name"]
    if name == ".text":
        return "code", f"{name} {tag} (no .pdata extent)"
    return "data", f"{name} {tag}"


def report(crate: str, wanted: int, queue_only: bool) -> int:
    image = image_bytes()
    secs = sections(image)
    con = sqlite3.connect(CORPUS)
    register = function_register(con)
    begins = sorted(register)
    names = names_table()
    units = unit_addresses()
    calls = callees(con)

    cites = cited(crate)
    rows = []
    for exe in sorted(cites):
        state, detail = classify(exe, register, begins, secs, units)
        rows.append((exe, state, detail))

    counts: dict[str, int] = {}
    for _exe, state, _detail in rows:
        counts[state] = counts.get(state, 0) + 1

    if not queue_only:
        print(f"{crate}: {len(rows)} distinct client addresses cited")
        for state in ("verified", "entry", "interior", "code", "data",
                      "header", "unmapped"):
            if counts.get(state):
                print(f"  {state:<10} {counts[state]:>4}")
        print()
        for exe, state, detail in rows:
            name = names.get(exe, "")
            unit = ", ".join(units.get(exe, []))
            line = f"  {exe:#x}  {state:<9} {name or '-':<28}"
            if unit:
                line += f" [{unit}]"
            if detail:
                line += f" {detail}"
            print(line.rstrip())

    # Verified callee count per unverified entry, for the playbook's ordering.
    pending = []
    for exe, state, _detail in rows:
        if state != "entry":
            continue
        rva = exe - BASE
        size = register[rva][1] - register[rva][0]
        verified_callees = sum(1 for c in calls.get(exe, ()) if c in units)
        pending.append((exe, size, verified_callees, len(calls.get(exe, ()))))
    pending.sort(key=lambda t: (-t[2], t[1]))
    if pending:
        print()
        print(f"{crate}: {len(pending)} function entries without a unit "
              f"(playbook order: proved callees, then size)")
        shown = pending[:wanted] if queue_only else pending
        for exe, size, done, total in shown:
            print(
                f"  {exe:#x}  {size:>5} B  callees {done}/{total} verified  "
                f"{names.get(exe, '-')}"
            )
    if counts.get("entry", 0) == 0:
        print(f"{crate}: every cited function entry has a unit")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("crate", nargs="?", default="all")
    ap.add_argument("--queue", action="store_true", help="only the next-unit queue")
    ap.add_argument("--top", type=int, default=15, help="queue rows to print")
    args = ap.parse_args(argv)
    crates = ["ow-casc", "ow-tank"] if args.crate == "all" else [args.crate]
    for crate in crates:
        report(crate, args.top, args.queue)
        if len(crates) > 1:
            print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

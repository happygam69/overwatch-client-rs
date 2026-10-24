"""Carry the name table into the next client build.

    python dev/carry_names.py --old <old image> --new <new image>

The client updates itself and the code moves, so an address that names a
function in one build is wrong in the next. This takes
`docs/function-names.csv`, translates each address into the new build (by the
function's own bytes, `dev/build_translate.py`), and writes the table back
with one row per program: the old address for the old program, the new one
for the new.

An address that cannot be translated (the patch rewrote the function) is
kept for the old program only, and the row says so.

The defaults remember the 2026-10-23 migration (into `module_v21_clean.bin`,
since deleted from the Ghidra project); pass --from-program/--to-program
explicitly for any new migration, and mind the address frame of the program
you tag (the v2b flat images vs `Overwatch.exe`'s exec frame).
"""

from __future__ import annotations

import argparse
import csv
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import build_translate as bt  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE = ROOT / "docs" / "function-names.csv"


def translate(old: bytes, new: bytes, addr: int) -> int | None:
    """The address the same code has in the new build, if it is there."""
    base = 0x7FF7C4D20000
    off = addr - base
    if off < 0 or off + 96 > len(old):
        return None
    needle = old[off : off + 96]
    if needle.count(0) == 96:
        return None
    at = new.find(needle)
    if at >= 0:
        return base + at
    for smaller in (48, 24):
        at = new.find(old[off : off + smaller])
        if at >= 0:
            return base + at
    return None


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--old", required=True)
    ap.add_argument("--new", required=True)
    ap.add_argument("--from-program", default="module_after_diff_v2.bin")
    ap.add_argument("--to-program", default="module_v21_clean.bin")
    args = ap.parse_args()

    old = pathlib.Path(args.old).read_bytes()
    new = pathlib.Path(args.new).read_bytes()
    with TABLE.open(encoding="utf-8", newline="") as f:
        raw = list(csv.reader(f))
    header, body = raw[0], raw[1:]
    rows = []
    for parts in body:
        # the role may hold commas: it is the last column
        row = dict(zip(header, parts))
        if None in row:
            row["role"] = ",".join(parts[len(header) - 1 :])
            row = {k: v for k, v in row.items() if k is not None}
        rows.append(row)
    out: list[dict[str, str]] = []
    moved = lost = 0
    for row in rows:
        programs = [p.strip() for p in row["program"].split(";")]
        if args.from_program not in programs:
            out.append(row)
            continue
        programs = [args.to_program if p == args.from_program else p for p in programs]
        new_addr = translate(old, new, int(row["address"], 16))
        if new_addr is None:
            # the patch rewrote it: the name stays on the old program only
            out.append({**row, "program": ";".join(p for p in programs
                                                   if p != args.to_program)})
            lost += 1
            print(f"{row['name']:<24} {row['address']} -> not found in the new build")
            continue
        out.append({**row, "program": args.to_program, "address": f"0x{new_addr:x}"})
        moved += 1
        print(f"{row['name']:<24} {row['address']} -> 0x{new_addr:x} in {args.to_program}")
    with TABLE.open("w", encoding="utf-8", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["program", "address", "doc_address", "name", "role"])
        w.writeheader()
        w.writerows(out)
    print(f"\n{moved} names carried into the new build, {lost} left behind")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

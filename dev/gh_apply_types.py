"""Apply the understood struct layouts and function prototypes to the Ghidra program.

    python dev/gh_apply_types.py            # import the structs, set the prototypes
    python dev/gh_apply_types.py --check    # report the C to be imported and the prototypes

The layouts live in `docs/struct-layouts.csv`: one row per understood field
(struct, offset, name, type, role), plus a header row per struct whose `name`
cell carries the total size (hex). The source of every field is whatever doc
the role cell cites; unre值得注意的是ad regions are neutral padding, never a guess.
The script:

  1. builds plain C from the table and imports it with the bridge's C parser
     (all documented offsets are naturally aligned, so no pack pragma);
  2. walks `docs/function-names.csv`; rows whose `prototype` column is filled
     get the signature applied, so `MoverAdvance` decompiles read `mover` and
     its typed fields instead of `param_1`.

Only layouts that have been read go in the table. Read-only against the image.
"""

from __future__ import annotations

import argparse
import csv
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import gh  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
LAYOUTS = ROOT / "docs" / "struct-layouts.csv"
NAMES = ROOT / "docs" / "function-names.csv"


def parse_rows() -> list[tuple[str, dict]]:
    """[(struct_name, {size, fields=[{offset,name,type,role}...] sorted})]

    A size row has empty `name` and `type`; its `offset` cell is the struct's
    total byte size (hex). Field rows carry offset/name/type/role.
    """
    with LAYOUTS.open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    structs: dict[str, dict] = {}
    for row in rows:
        sname = row["struct"].strip()
        s = structs.setdefault(sname, {"size": 0, "fields": []})
        offset_raw = (row["offset"] or "").strip()
        field_name = (row["name"] or "").strip()
        field_type = (row["type"] or "").strip()
        if not field_name and not field_type:
            if offset_raw:
                s["size"] = int(offset_raw, 16)
            continue
        s["fields"].append(
            {
                "offset": int(offset_raw, 16),
                "name": field_name,
                "type": field_type,
                "role": (row["role"] or "").strip(),
            }
        )
    return sorted(
        (name, {"size": s["size"], "fields": sorted(s["fields"], key=lambda x: x["offset"])})
        for name, s in structs.items()
    )


def c_declare(t: str, name: str) -> str:
    """The table spells array types `float [4]`; C wants `float name[4]`."""
    br = re.search(r"\[\s*([0-9a-xfx]+)\s*\]", t)
    if br:
        head = t[: br.start()].rstrip()
        if name in head:  # already inline (e.g. `unsigned char name[4]`)
            return head.rstrip() + ";"
        return f"{head} {name}{br.group(0)};"
    return f"{t} {name};"


def type_size(t: str) -> int:
    """Byte size of one table type; 0 marks 'cannot size' (then recorded 1)."""
    t = t.strip()
    m = re.match(r"static\s+", t)
    if m:
        t = t[m.end():]
    if "*" in t:
        return 8
    base = 4 if t.startswith("float") else 4 if "int" in t else 8 if t.startswith("unsigned long long") else 1
    br = re.search(r"\[([0-9a-fx]+)\]", t)
    n = int(br.group(1), 0) if br else 1
    return base * n


def build_c() -> tuple[str, list[tuple[str, dict]]]:
    structs = parse_rows()
    lines = ["// generated from docs/struct-layouts.csv - the docs are the source"]
    for sname, s in structs:
        lines.append("")
        if not s["fields"]:
            lines.append(f"// struct {sname}: no fields read yet")
            continue
        end_prev = 0
        lines.append(f"// {sname}: declared total 0x{s['size']:x}")
        lines.append(f"struct {sname} {{")
        for fld in s["fields"]:
            gap = fld["offset"] - end_prev
            if gap > 0:
                lines.append(f"  unsigned char _pad{end_prev:04x}[{gap:#x}];")
            lines.append("  " + c_declare(fld["type"], fld["name"]))
            end_prev = fld["offset"] + type_size(fld["type"])
        if s["size"] and s["size"] > end_prev:
            lines.append(f"  unsigned char _padend{end_prev:04x}[{s['size'] - end_prev:#x}];")
        lines.append("};")
    return "\n".join(lines) + "\n", structs


def named_prototypes() -> list[dict]:
    with NAMES.open(encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f))
    return [r for r in rows if (r.get("prototype") or "").strip()]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--program", default="Overwatch.exe")
    args = ap.parse_args()

    csrc, structs = build_c()
    save = pathlib.Path.home() / ".cache" / "ow-struct-layouts.c"
    save.parent.mkdir(parents=True, exist_ok=True)
    save.write_text(csrc, encoding="utf-8")
    print(f"C source ({len(csrc.splitlines())} lines) -> {save}")

    if args.check:
        for row in named_prototypes():
            print(f"prototype {row['name']}: {row['prototype']}")
        return 0

    out = gh.call("/import_data_types", {"program": args.program, "source": csrc})
    print("import:", str(out)[:400])
    # the parser sizes a struct to its text; the declared total must come back
    for sname, s in structs:
        got = gh.call("/get_type_size", {"type_name": sname, "program": args.program})
        size = got.get("size") if isinstance(got, dict) else None
        if size == s["size"]:
            print(f"size {sname}: {size:#x} matches the declared total")
            continue
        print(f"size {sname}: parsed {size!r} vs declared {s['size']:#x} -> resizing")
        out = gh.call(
            "/resize_struct",
            {"name": sname, "new_size": s["size"], "program": args.program},
        )
        print(f"  resize {sname}:", str(out)[:160])

    for row in named_prototypes():
        body = {
            "address": row["address"],
            "name": row["name"],
            "prototype": row["prototype"],
            "program": args.program,
            "score": False,
        }
        out = gh.call("/apply_documentation", body)
        print(f"prototype {row['name']}:", str(out)[:400])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

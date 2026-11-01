"""Name the client's functions in the Ghidra project, and carry the written findings with them.

    python dev/gh_apply_names.py            # apply every name, comment and tag
    python dev/gh_apply_names.py --check    # report what is named and what is not

The table is `docs/function-names.csv`: program, address, name, the role text
the docs already state about the function, and (optionally) a system tag. Two
things are applied per row:

  - the name, so the decompiler and the documentation say the same thing; and
  - the role as the function's plate comment, plus the tag as a Ghidra function
    tag, so an MCP read brings its own documentation and tag-filtered lists
    work. A row's comments carry what the repo already claims - no new facts.

A row is added when a function's role is understood, never before. A function
the project has not analysed yet is created first (Ghidra makes one at the
address and names it), so a row never silently does nothing.
"""

from __future__ import annotations

import csv
import pathlib
import sys

import gh

ROOT = pathlib.Path(__file__).resolve().parent.parent
TABLE = ROOT / "docs" / "function-names.csv"


def rows() -> list[dict[str, str]]:
    with TABLE.open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def ok(out: dict) -> bool:
    message = str(out.get("message", ""))
    return (
        out.get("status") == "success"
        or message.startswith("Success")
        or "created successfully" in message
        or "Renamed" in message
    )


def restore_saved_plates(programs, rows_: list[dict[str, str]]) -> None:
    """Bulk plate writes overwrite by design; a hand-written plate comment that
    said something the row did not would then vanish. The recorder below saved
    the pre-run plates; re-apply any that differ, merged with the row's role."""
    saved = pathlib.Path.home() / ".cache" / "ow-plate-backup.json"
    if not saved.is_file():
        return
    import json as _json

    backup = _json.loads(saved.read_text(encoding="utf-8"))
    by_addr = {row["address"]: row for row in rows_}
    fixed = 0
    entries = []
    for address, plate in backup.items():
        old = (plate or "").strip()
        row = by_addr.get(address)
        if not old or row is None:
            continue
        merged = old.rstrip() + "\n\n" + row["role"].strip()
        entries.append({"address": address, "plate_comment": merged})
    for i in range(0, len(entries), 50):
        out = gh.call("/apply_documentation", {"entries": entries[i : i + 50]})
        fixed += out.get("succeeded", 0)
    if fixed:
        print(f"{fixed} pre-existing plate comments re-applied, merged with the row's role")
    return


def make_and_name(program: str, address: str, name: str) -> dict:
    """Name a function, creating a small one first if the project has none.

    The window is deliberately small: a function whose body Ghidra has to
    discover by following the flow becomes enormous in a flat image, and the
    decompiler then chokes on it.
    """
    body = {
        "function": address,
        "new_name": name,
        "program": program,
        "strict_mode": "warn",
    }
    out = gh.call("/rename_function", body)
    if ok(out):
        return out
    gh.call(
        "/disassemble_bytes",
        {
            "start_address": address,
            "length": 64,
            "program": program,
            "include_instructions": False,
            "restrict_to_execute_memory": False,
        },
    )
    made = gh.call(
        "/create_function",
        {
            "address": address,
            "name": name,
            "program": program,
            "disassemble_first": False,
        },
    )
    if not ok(made):
        return made
    return gh.call("/rename_function", body)


def main() -> int:
    check = "--check" in sys.argv
    named = 0
    missing = 0
    documented = 0
    touched: set[str] = set()
    rows_ = rows()
    for row in rows_:
        programs = [p.split("/")[-1] for p in row["program"].split(";")]
        address = row["address"]
        name = row["name"]
        for program in programs:
            out = make_and_name(program, address, name)
            if ok(out):
                named += 1
                touched.add(program)
                print(f"named {name:<28} {address} in {program}")
            else:
                missing += 1
                why = out.get("message") or out.get("error") or str(out)[:120]
                print(f"MISS  {name:<28} {address} in {program}: {why}")
    print(f"\n{named} named, {missing} not found")
    if check and missing:
        return 1

    # Every named function also gets the docs' role text as its plate comment
    # and the system tag, best-effort and per program - reading a decompile
    # then brings its own documentation.
    for program in sorted(touched):
        rows_here = [
            {"address": row["address"], "plate_comment": row["role"], "tags": row.get("tag", "")}
            for row in rows_
            if program in row["program"]
        ]
        for i in range(0, len(rows_here), 50):
            chunk = rows_here[i : i + 50]
            out = gh.call("/apply_documentation", {"entries": chunk, "program": program})
            if "succeeded" in out:
                documented += out.get("succeeded", 0)
                if out.get("failed"):
                    print(f"note: {out.get('failed')} plate/tag step(s) failed in batch {i // 50}")
            else:
                print(f"description batch {i // 50} of {program}: {str(out)[:160]}")
    if documented:
        print(f"{documented} functions documented (plate comment + tag), best-effort")
    restore_saved_plates([program for program in sorted(touched)], rows_)
    # A rename that is not saved is lost the next time Ghidra restarts, and
    # Ghidra's save can take a while on a big image, so save each program
    # that changed and say so.
    for program in sorted(touched):
        out = gh.call("/save_program", {"program": program}, timeout=600)
        print(f"saved {program}: {out.get('message', out)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

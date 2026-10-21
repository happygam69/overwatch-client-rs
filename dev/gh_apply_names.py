"""Name the client's functions in the Ghidra project.

    python dev/gh_apply_names.py            # apply every name in the table
    python dev/gh_apply_names.py --check    # report what is named and what is not

The table is `docs/function-names.csv`: program, address, name, what the
function does. The names are the ones the docs already use when they talk
about the function, so the decompiler and the documentation say the same
thing. A row is added when a function's role is understood, never before.

A function the project has not analysed yet is created first (Ghidra makes
one at the address and names it), so a row never silently does nothing.
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
    for row in rows():
        programs = [p.split("/")[-1] for p in row["program"].split(";")]
        address = row["address"]
        name = row["name"]
        for program in programs:
            out = make_and_name(program, address, name)
            if ok(out):
                named += 1
                print(f"named {name:<28} {address} in {program}")
            else:
                missing += 1
                why = out.get("message") or out.get("error") or str(out)[:120]
                print(f"MISS  {name:<28} {address} in {program}: {why}")
    print(f"\n{named} named, {missing} not found")
    if check and missing:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Check that the fitted ledger and the code agree.

    python dev/fit_check.py            # report
    python dev/fit_check.py --check    # report and exit non-zero on a problem

The rule this enforces (`AGENTS.md`): a fitted, assumed, stand-in or supplied
implementation is never silent. Every such implementation has an id
(`FIT-CAM-01` and friends) in the ledger and the same id next to the code that
carries it, so neither side can drift without the check noticing.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "fitted-ledger.md"
ID = re.compile(r"FIT-[A-Z]+-\d+")
CODE_SUFFIXES = (".rs", ".wgsl")


def ledger_ids() -> list[str]:
    """The ids of the ledger's rows (the prose may mention them as examples)."""
    text = LEDGER.read_text(encoding="utf-8")
    rows = [line for line in text.splitlines() if line.startswith("| ")]
    return ID.findall("\n".join(rows))


def code_ids() -> dict[str, list[str]]:
    found: dict[str, list[str]] = {}
    for path in ROOT.joinpath("crates").rglob("*"):
        if path.suffix not in CODE_SUFFIXES or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for m in ID.findall(text):
            found.setdefault(m, []).append(str(path.relative_to(ROOT)))
    return found


def main() -> int:
    check = "--check" in sys.argv
    ledger = ledger_ids()
    code = code_ids()
    problems = 0

    dupes = sorted({i for i in ledger if ledger.count(i) > 1})
    for i in dupes:
        print(f"FAIL  {i} is listed more than once in the ledger")
        problems += 1

    for i in sorted(set(ledger)):
        if i not in code:
            print(f"FAIL  {i} is in the ledger but has no marker in the code")
            problems += 1

    for i, files in sorted(code.items()):
        if i not in ledger:
            print(f"FAIL  {i} is marked in {', '.join(files)} but not in the ledger")
            problems += 1
        elif len(files) > 1:
            print(f"WARN  {i} is marked in more than one file: {', '.join(files)}")

    print(f"\n{len(set(ledger))} ledger rows, {len(code)} marked code sites")
    if problems:
        print(f"{problems} problem(s)")
        return 1 if check else 0
    print("the ledger and the code agree")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Check that the legacy deviations and the code agree.

    python dev/deviation_check.py            # report
    python dev/deviation_check.py --check    # report and exit non-zero

The rule (`AGENTS.md`): gameplay is 1 to 1 or not implemented. The `FIT-*`
ids below are the legacy exceptions that predate the rule. They must be
removed, and no new one may be introduced: an id in the code that is not in
the frozen set is a failure even if a ledger row was added for it.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs" / "deviations.md"
ID = re.compile(r"FIT-[A-Z]+-\d+")
CODE_SUFFIXES = (".rs", ".wgsl")

# The only deviations allowed to exist. This set is frozen: removing a row is
# progress; adding one is a violation of the 1-to-1 rule.
KNOWN = {
    "FIT-ANI-02", "FIT-ANI-04", "FIT-ANI-07",
    "FIT-CAM-03", "FIT-CAM-05", "FIT-CAM-06",
    "FIT-PRE-01", "FIT-PRE-02",
}


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
        print(f"FAIL  {i} is listed more than once in the deviations table")
        problems += 1

    for i in sorted(set(ledger)):
        if i not in code:
            print(f"FAIL  {i} is in the table but has no marker in the code")
            problems += 1
        if i not in KNOWN:
            print(f"FAIL  {i} is a new deviation; the 1-to-1 rule forbids it")
            problems += 1

    for i, files in sorted(code.items()):
        if i not in KNOWN:
            print(
                f"FAIL  {i} is marked in {', '.join(files)}; "
                "new deviations are not allowed"
            )
            problems += 1
        if i not in ledger:
            print(f"FAIL  {i} is marked in {', '.join(files)} but not in the table")
            problems += 1
        elif len(files) > 1:
            print(f"WARN  {i} is marked in more than one file: {', '.join(files)}")

    print(f"\n{len(set(ledger))} deviation rows, {len(code)} marked code sites")
    if problems:
        print(f"{problems} problem(s)")
        return 1 if check else 0
    print("the deviations and the code agree; no new deviation was added")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

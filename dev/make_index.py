"""Build the tool tables in docs/tools.md from the tools' own headers.

    python make_index.py            # refresh every generated table
    python make_index.py --check    # fail if a table is stale

Every tracked script under `dev/` carries a one-line docstring saying what
question it answers. Those lines are the tables between the marker comments in
`docs/tools.md`, which is the project's only tool list; the gate runs this
with `--check`. Run it after adding, renaming or re-scoping a script.
"""

from __future__ import annotations

import ast
import pathlib
import subprocess
import sys

DEV = pathlib.Path(__file__).resolve().parent
ROOT = DEV.parent
DOC = ROOT / "docs" / "tools.md"

# (marker name, what the table holds). The doc carries one `begin`/`end` pair
# of comments per entry; the text between them is replaced wholesale.
REGIONS = (
    ("dev-tools", "the analysis scripts in dev/"),
    ("harness-tools", "the verification harness itself"),
    ("harness-fixtures", "the per-unit fixtures"),
)


def summary(path: pathlib.Path) -> str:
    """The first meaningful line of the module docstring."""
    try:
        tree = ast.parse(path.read_text(encoding="utf-8", errors="replace"))
    except SyntaxError:
        return "(no parseable header)"
    doc = ast.get_docstring(tree) or ""
    for line in doc.splitlines():
        line = line.strip()
        if line:
            return line
    return "(no header)"


def tool_paths() -> list[pathlib.Path]:
    """Every `.py` under `dev/` that git would ship.

    Ignored files are excluded on purpose. `dev/data/`, `dev/decompiles/` and
    the rest are untracked analysis output, and indexing them made the tables
    unreproducible: they carried rows a clean checkout does not have, and
    `--check` failed there. `--others --exclude-standard` still picks up a new
    tool that has not been `git add`ed.
    """
    try:
        out = subprocess.run(
            [
                "git",
                "-C",
                str(ROOT),
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--",
                "dev",
            ],
            check=True,
            capture_output=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        # No git (an unpacked tree): nothing in it is ignored anyway.
        found = list(DEV.rglob("*.py"))
        return sorted(p for p in found if p.is_file())
    found = {ROOT / p for p in out.decode("utf-8", "replace").split("\0") if p}
    return sorted(p for p in found if p.suffix == ".py" and p.is_file())


def groups() -> dict[str, list[pathlib.Path]]:
    """The tools, split by the table they belong in.

    A fixture is `dev/verify_harness/*_fixture.py`: the deterministic cases a
    unit is driven with. Everything else in `verify_harness/` is the harness's
    own machinery. Everything outside it is an analysis script.
    """
    out: dict[str, list[pathlib.Path]] = {name: [] for name, _ in REGIONS}
    for path in tool_paths():
        if path.parent.name == "verify_harness":
            key = "harness-fixtures" if path.name.endswith("_fixture.py") else "harness-tools"
        else:
            key = "dev-tools"
        out[key].append(path)
    return out


def render(paths: list[pathlib.Path]) -> str:
    """One markdown table: repo-relative path, then the tool's own header."""
    out = ["| tool | what it answers |", "|---|---|"]
    for path in paths:
        name = path.relative_to(ROOT).as_posix()
        text = summary(path).replace("|", "\\|")
        out.append(f"| `{name}` | {text} |")
    return "\n".join(out)


def markers(name: str) -> tuple[str, str]:
    return f"<!-- {name}:begin -->", f"<!-- {name}:end -->"


def region(text: str, name: str) -> tuple[int, int, str]:
    """The (content start, content end, error) of one marked region."""
    begin, end = markers(name)
    start = text.find(begin)
    stop = text.find(end)
    if start < 0 or stop < 0 or stop <= start:
        return 0, 0, f"docs/tools.md: missing marker pair {begin} / {end}"
    return start + len(begin), stop, ""


def replace(text: str, name: str, body: str) -> str:
    start, stop, error = region(text, name)
    if error:
        raise SystemExit(error)
    return text[:start] + "\n" + body + "\n" + text[stop:]


def main() -> int:
    found = groups()
    if not DOC.exists():
        print(f"missing {DOC}")
        return 1
    text = DOC.read_text(encoding="utf-8")
    if "--check" in sys.argv:
        stale = []
        for name, what in REGIONS:
            start, stop, error = region(text, name)
            if error:
                print(error)
                return 1
            if text[start:stop].strip() != render(found[name]).strip():
                stale.append(what)
        if stale:
            print("docs/tools.md is stale: " + ", ".join(stale))
            print("run: python dev/make_index.py")
            return 1
        total = sum(len(v) for v in found.values())
        print(f"docs/tools.md is up to date ({total} tools)")
        return 0
    for name, _ in REGIONS:
        text = replace(text, name, render(found[name]))
    DOC.write_text(text, encoding="utf-8")
    total = sum(len(v) for v in found.values())
    print(f"wrote {DOC} ({total} tools)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

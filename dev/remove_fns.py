"""Remove a named set of functions from Rust files.

    python dev/remove_fns.py crates/ow-tank/src/unlocks.rs skin_themes read_unlock

The function body is cut from its `pub fn` line to the matching closing brace,
together with the doc comment directly above it. Blank lines left behind are
collapsed. The compiler is the judge: run `cargo check` afterwards.
"""

from __future__ import annotations

import sys


def cut(path: str, names: list[str]) -> int:
    src = open(path, encoding="utf-8").read()
    lines = src.splitlines(keepends=True)
    out: list[str] = []
    i = 0
    removed = 0
    while i < len(lines):
        line = lines[i]
        hit = None
        for name in names:
            if line.lstrip().startswith(f"pub fn {name}(") or line.lstrip().startswith(
                f"pub(crate) fn {name}("
            ):
                hit = name
                break
        if hit is None:
            out.append(line)
            i += 1
            continue
        # drop the doc comment block directly above
        while out and out[-1].lstrip().startswith("///"):
            out.pop()
        # skip the body
        depth = 0
        started = False
        while i < len(lines):
            depth += lines[i].count("{") - lines[i].count("}")
            if "{" in lines[i]:
                started = True
            i += 1
            if started and depth <= 0:
                break
        # drop a trailing blank line
        if i < len(lines) and lines[i].strip() == "":
            i += 1
        removed += 1
    open(path, "w", encoding="utf-8").write("".join(out))
    return removed


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    path = sys.argv[1]
    names = sys.argv[2:]
    n = cut(path, names)
    print(f"removed {n} function(s) from {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

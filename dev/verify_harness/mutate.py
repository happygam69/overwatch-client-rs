"""Behaviour-changing mutations of a C candidate, for measuring whether the
differential had any power to detect a defect at all.

Port of the Sandustry harness's mutate.js to C source: comparison flips,
boundary shifts, operator swaps, and constant edits. Each mutant must cause
the differential to report a divergence; a survivor means the fixture is too
weak or the mutant is observationally equivalent - both must be handled
explicitly, never absorbed.

Mutants are line-targeted, not AST-based: the decompiles are small and
regular enough that line+kind is sufficient, and it avoids a C parser
dependency. A mutation that produces invalid C is counted as killed (the
build fails; the original still verified; detection power for the mutants
that do build is what the gate measures).
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass
from typing import Any, Dict, List


@dataclass
class Mutant:
    kind: str
    line: int  # 1-based
    excerpt: str
    source: str


def _expand_includes(source: str, base_dir: str) -> str:
    """Inline Rust `include!("path")` directives (one level is enough for
    rust_probe.rs -> rust_rewrites/*.rs). The mutator walks the expanded
    text so line edits land in the real function body; the mutant source
    is the expanded form, which still compiles as a single file."""
    out_lines: List[str] = []
    inc_re = re.compile(r'^\s*include!\s*\(\s*"([^"]+)"\s*\)\s*;')
    for line in source.splitlines(keepends=True):
        m = inc_re.match(line)
        if m:
            path = os.path.join(base_dir, m.group(1))
            try:
                with open(path, "r", encoding="utf-8") as f:
                    out_lines.append(f.read())
            except OSError:
                out_lines.append(line)  # leave the directive; build will fail loudly
        else:
            out_lines.append(line)
    return "".join(out_lines)


_CMP_FLIPS = [
    ("<=", "<"),
    ("<=", "=="),  # a second, nastier flip: boundary to equality
    (">=", ">"),
    (">=", "=="),
    ("<", "<="),
    (">", ">="),
    ("==", "!="),
    ("!=", "=="),
]

_OP_SWAPS = [
    (" + ", " - "),
    (" - ", " + "),
    (" * ", " / "),
    (" / ", " * "),
]


def generate_mutants(
    source: str, entry: str, count: int = 12, base_dir: str = "."
) -> List[Mutant]:
    """Produce up to `count` distinct behaviour-changing mutants of `source`.

    Walks the lines inside the entry function only (from the first line
    containing the entry name to the first line that is just `}` at column 0
    after that, a simple and honest bound for these files). Mutations that
    would be no-ops on a given line are skipped. Rust `include!` directives
    are expanded first so the mutator sees the real function body.
    """
    source = _expand_includes(source, base_dir)
    lines = source.splitlines(keepends=True)
    # Find the entry function body. Ghidra C style: the signature line has
    # the name and parens but no brace; the brace is on its own line after
    # it. Rust style: `fn name(...)` on one line. Skip comment lines so a
    # doc comment mentioning the name is not mistaken for the definition.
    # Prefer an actual Rust definition: a call site earlier in the file (for
    # example `be_u32`'s caller invoking `salsa20(...)`) is not the function
    # the mutation stage must edit.
    sig = -1
    def_re = re.compile(r"\bfn\s+" + re.escape(entry) + r"\s*\(")
    for i, ln in enumerate(lines):
        stripped = ln.strip()
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
            continue
        if def_re.search(ln):
            sig = i
            break
    if sig < 0:
        for i, ln in enumerate(lines):
            stripped = ln.strip()
            if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
                continue
            if entry in ln and "(" in ln:
                sig = i
                break
    if sig < 0:
        return []
    start = sig
    for i in range(sig, len(lines)):
        if "{" in lines[i]:
            start = i
            break
    end = len(lines)
    depth = 0
    opened = False
    for i in range(start, len(lines)):
        depth += lines[i].count("{") - lines[i].count("}")
        if "{" in lines[i]:
            opened = True
        if opened and depth <= 0:
            end = i + 1
            break

    mutants: List[Mutant] = []
    seen: set = set()

    def add(kind: str, lineno: int, newline: str) -> None:
        key = (kind, lineno, newline)
        if key in seen:
            return
        seen.add(key)
        out = lines[:]
        out[lineno - 1] = newline
        mutants.append(
            Mutant(
                kind=kind,
                line=lineno,
                excerpt=lines[lineno - 1].rstrip()[:80],
                source="".join(out),
            )
        )

    for i in range(start, end):
        raw = lines[i]
        s = raw.rstrip("\n")
        lineno = i + 1
        # Skip comments and the function signature line.
        if s.strip().startswith("//") or s.strip().startswith("/*"):
            continue
        if entry in s and "(" in s:
            continue

        # Split off any trailing line comment so mutations only touch code
        # (a mutation inside a comment would be a guaranteed-equivalent
        # mutant - pure noise in the kill-rate denominator).
        cpos = s.find("//")
        if cpos >= 0:
            code_part = s[:cpos]
            comment = s[cpos:]
        else:
            code_part = s
            comment = ""
        if not code_part.strip():
            continue

        def rebuilt(new_code: str) -> str:
            return new_code + (comment if comment else "") + "\n"

        # Comparison flips, via regex so a shift (<<, >>) is never touched:
        # '<<' must not become '<<=', '>>' must not become '>='.
        _flip_res = [
            ("cmp-flip <to<=", re.compile(r"(?<![<])<(?![<=])"), "<="),
            ("cmp-flip >to>=", re.compile(r"(?<![=>])>(?![=>])"), ">="),
            ("cmp-flip <=to<", re.compile(r"(?<![<])<="), "<"),
            ("cmp-flip >=to>", re.compile(r"(?<![=])>="), ">"),
            ("cmp-flip ==to!=", re.compile(r"(?<!=)==(?!=)"), "!="),
            ("cmp-flip !=to==", re.compile(r"!="), "=="),
        ]
        for name, rx, rep in _flip_res:
            m2 = rx.search(code_part)
            if m2:
                add(name, lineno,
                    rebuilt(code_part[: m2.start()] + rep + code_part[m2.end():]))

        for a, b in _OP_SWAPS:
            if a in code_part:
                add(f"op-swap {a.strip()}->{b.strip()}",
                    lineno, rebuilt(code_part.replace(a, b, 1)))

        # Boundary shift: integer literals inside comparisons +/- 1.
        m = re.search(r"(\s)(\d+)(\s*[;,)])", code_part)
        if m and any(c in code_part for c in "<>="):
            n = int(m.group(2))
            add(
                "boundary +1",
                lineno,
                rebuilt(code_part[: m.start(2)] + str(n + 1) + code_part[m.end(2) :]),
            )
            if n > 0:
                add(
                    "boundary -1",
                    lineno,
                    rebuilt(code_part[: m.start(2)] + str(n - 1) + code_part[m.end(2) :]),
                )

        # Same shift for hex literals (the `count > 0x3f` style gate): the
        # decimal pattern above never matched them, so boundary mutants for
        # hex gates silently never existed. Require a real comparison
        # operator - a bare `=` in `let x = 0x5c` is an assignment, not a
        # gate, and mutating offsets would just break the build.
        hm = re.search(r"(?i)(?<![\w.])0x([0-9a-f]+)(?=[;,)])", code_part)
        if hm and re.search(r"[<>]=?|==|!=", code_part) and "<<" not in code_part:
            n = int(hm.group(1), 16)
            # Replace only the digit run (group 1); hex() includes the 0x
            # prefix, so writing it back at start(1) would produce 0x0x..
            add(
                "boundary +1",
                lineno,
                rebuilt(code_part[: hm.start(1)] + hex(n + 1)[2:] + code_part[hm.end(1) :]),
            )
            if n > 0:
                add(
                    "boundary -1",
                    lineno,
                    rebuilt(code_part[: hm.start(1)] + hex(n - 1)[2:] + code_part[hm.end(1) :]),
                )

        # Float constant perturbation (the 0.004 / 0.235 / 3.0 style).
        # Skip 0.0 for x1.5 (0*1.5==0, an observational no-op) and skip
        # "zero" when the constant already is 0.0.
        fm = re.search(r"(\d+\.\d+)", code_part)
        if fm:
            v = float(fm.group(1))
            if v != 0.0:
                add(
                    "float-const x1.5",
                    lineno,
                    rebuilt(code_part[: fm.start(1)] + repr(v * 1.5) + code_part[fm.end(1) :]),
                )
            if v != 0.0:
                add(
                    "float-const zero",
                    lineno,
                    rebuilt(code_part[: fm.start(1)] + "0.0" + code_part[fm.end(1) :]),
                )

        # Bit-mask change: `& 1` -> `& 3` etc (flag tests).
        bm = re.search(r"& (0x[0-9a-fA-F]+|\d+)", code_part)
        if bm:
            mask = int(bm.group(1), 0)
            add(
                "mask widen",
                lineno,
                rebuilt(code_part[: bm.start(1)] + str(mask | (mask << 1)) + code_part[bm.end(1) :]),
            )

        if len(mutants) >= count * 4:
            break

    # The *4 during generation is a safety valve against one line matching
    # many patterns; the OUTPUT is capped at the requested count so the
    # runner's kill-rate denominator stays what the unit asked for.
    return mutants[:count]

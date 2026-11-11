"""One build for a unit's whole mutant set.

The mutation stage is the harness's expensive stage: a cargo unit's mutants
are each a full crate rebuild (measured: 4.4 s with a warm target dir), and a
unit has a dozen of them. That is ~80% of the harness's wall time, and it is
paid again every time a bound source file changes.

The batch removes the repetition instead of the work: every mutant of a unit
becomes an *arm* of one dispatch inside the function under test, so the crate
is compiled **once** and the harness picks the arm at run time with an
environment variable.

    // generated, for a function `f` with mutants 0 and 1
    match __ow_mutant_index() {
        0 => { <mutant 0's body> }
        1 => { <mutant 1's body> }
        _ => { <the original body> }
    }

The arms are the *original* interior with the mutation applied, so a mutant
is exactly what it was when it was built one at a time. The `_` arm is the
original body: a batch binary with no `OW_MUTANT_INDEX` set behaves like the
unmutated code, which is what the harness's safety check compares against the
baseline candidate.

What this module does NOT do: decide whether a mutant is killed. The
mutation stage still runs every mutant's corpus through the batch binary, one
process per mutant (the index is fixed at process start), and the kill
verdict is unchanged.

A batch that does not compile is not a failure of the unit: the compiler
caught a mutant, which is a kill. The build error names the line, the line
names the arm, and the arm is dropped and the batch rebuilt. If the error
cannot be attributed to an arm, the caller falls back to building the
mutants one at a time.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from typing import Any, List, Optional, Tuple

from mutate import expand_includes, function_extent

# The environment variable the batch binary reads, and the helper the
# generated file carries to read it.
ENV_NAME = "OW_MUTANT_INDEX"
HELPER_NAME = "__ow_mutant_index"

# The generated helper. It is a free function in the module that holds the
# function under test, so it needs no import and no pub.
HELPER = f'''/// The mutation harness's dispatch index (`{ENV_NAME}`).
///
/// The batch build carries one arm per mutant of this function; the harness
/// sets the index in the environment before it runs a mutant. Without the
/// variable the index is out of range and the original body runs.
#[inline]
fn {HELPER_NAME}() -> usize {{
    match std::env::var("{ENV_NAME}") {{
        Ok(v) => v.parse::<usize>().unwrap_or(usize::MAX),
        Err(_) => usize::MAX,
    }}
}}
'''


@dataclass
class BatchArm:
    """One arm of the generated dispatch."""

    index: int
    mutant: Any
    # The arm's line range in the generated file, 1-based and inclusive, as
    # the compiler reports them. An error inside this range belongs to this
    # mutant.
    line_start: int
    line_end: int


@dataclass
class BatchPlan:
    """A generated source file plus the arm map back to the mutants."""

    source: str
    arms: List[BatchArm] = field(default_factory=list)
    # The original body, as the `_` arm.
    original_arm: Optional[Tuple[int, int]] = None

    def arm_for_line(self, line: int) -> Optional[BatchArm]:
        """The arm a compiler error at `line` (1-based) belongs to."""
        for arm in self.arms:
            if arm.line_start <= line <= arm.line_end:
                return arm
        return None


def _module_insert_point(lines: List[str]) -> int:
    """Where the dispatch helper goes: the module's own top level.

    The helper is a free function, so it has to sit at module scope - a
    function under test that is *nested* inside another function would
    otherwise get the helper inside that function, where it cannot be
    defined. The insertion point is after the file's leading inner
    attributes (`#![...]`) and `use` lines, because an inner attribute must
    come first in a file and a `use` before a function is the file's own
    order.
    """
    i = 0
    while i < len(lines):
        stripped = lines[i].strip()
        if (
            stripped == ""
            or stripped.startswith("use ")
            or stripped.startswith("pub use ")
            or stripped.startswith("#![")
            or stripped.startswith("//")
        ):
            i += 1
            continue
        # A `use` or attribute that spans lines ends at the first line that
        # closes it.
        if stripped.startswith("use") or stripped.startswith("#[") or stripped.startswith("pub use"):
            depth = lines[i].count("{") - lines[i].count("}")
            j = i
            while depth > 0 and j + 1 < len(lines):
                j += 1
                depth += lines[j].count("{") - lines[j].count("}")
            i = j + 1
            continue
        break
    return i


def plan_batch(
    original_source: str,
    entry: str,
    mutants: List[Any],
    base_dir: str = ".",
) -> Optional[BatchPlan]:
    """Build the batch source for `mutants` of `entry` in `original_source`.

    `mutants` are `mutate.Mutant` objects; each carries the whole source
    with its own mutation applied. The batch slices the function's interior
    out of each of them, so a mutant's arm is byte-for-byte the body that
    mutant would have been built with on its own.

    Returns None when the function cannot be located, or when a mutant
    changed the line count - the caller then builds the mutants one at a time.
    """
    source = expand_includes(original_source, base_dir)
    lines = source.splitlines(keepends=True)
    extent = function_extent(source, entry)
    if extent is None:
        return None
    open_line, close_line, _sig = extent
    if close_line <= open_line:
        return None

    interior = lines[open_line + 1 : close_line]
    helper_lines = HELPER.splitlines(keepends=True)
    helper_at = _module_insert_point(lines)

    out: List[str] = []
    out.extend(lines[:helper_at])
    out.extend(helper_lines)
    out.extend(lines[helper_at : open_line + 1])  # up to the function's "{"

    out.append("    match " + HELPER_NAME + "() {\n")
    arms: List[BatchArm] = []
    index = 0
    for mutant in mutants:
        variant_lines = mutant.source.splitlines(keepends=True)
        if len(variant_lines) != len(lines):
            # The mutant changed the line count (it never should): the arms
            # would not line up with the compiler's line numbers.
            return None
        # The dispatch replaces the function's BODY, so a mutation outside the
        # body cannot be an arm - a mutation on the signature (the line that
        # carries the opening brace is part of the signature) would be lost.
        # Such a mutant is simply not in the batch; the caller builds it on
        # its own, as it did before.
        changed = [
            i for i, (a, b) in enumerate(zip(lines, variant_lines)) if a != b
        ]
        if any(not (open_line < i < close_line) for i in changed):
            continue
        body = variant_lines[open_line + 1 : close_line]
        arm_start = len(out) + 1  # 1-based: the `index => {` line
        out.append(f"        {index} => {{\n")
        for bl in body:
            out.append("    " + bl if bl.strip() else bl)
        out.append("        }\n")
        arm_end = len(out)  # 1-based inclusive: the closing `}` line
        arms.append(
            BatchArm(
                index=index,
                mutant=mutant,
                line_start=arm_start,
                line_end=arm_end,
            )
        )
        index += 1
    if not arms:
        # Every mutation of this function is outside its body, so there is
        # nothing to dispatch: the caller builds them one at a time.
        return None
    original_start = len(out) + 1
    out.append("        _ => {\n")
    for bl in interior:
        out.append("    " + bl if bl.strip() else bl)
    out.append("        }\n")
    original_end = len(out)
    out.append("    }\n")
    out.extend(lines[close_line:])

    plan = BatchPlan(source="".join(out), arms=arms)
    plan.original_arm = (original_start, original_end)
    return plan


def error_lines(build_output: str) -> List[int]:
    """The source lines a compiler error names, from a cargo/rustc log.

    Both formats are recognised: rustc's `-->` pointer and the older
    `file:line:column:` form.
    """
    out: List[int] = []
    for m in re.finditer(r"--> ([^\r\n:]+):(\d+):(\d+)", build_output):
        out.append(int(m.group(2)))
    if not out:
        for m in re.finditer(r"([^\r\n:]+):(\d+):(\d+): error", build_output):
            out.append(int(m.group(2)))
    return out

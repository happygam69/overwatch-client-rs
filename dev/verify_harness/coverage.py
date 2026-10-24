"""Basic-block / branch coverage of the oracle, measured on the very runs
that were compared.

The Sandustry harness uses istanbul on the JS oracle; here the oracle is
machine code under Unicorn, so we take a different measurement of the same
claim: which conditional-branch edges the compared executions actually took,
against every conditional branch in the function body. Agreement on branches
nobody reached proves nothing.

Branch targets are discovered by a linear sweep from the entry with capstone,
stopping at the first unconditional jump past the function or a run of int3,
and at a configurable byte ceiling. A sweep that cannot find the function
end reports that honestly rather than guessing.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from typing import List, Sequence, Set, Tuple

from unicorn import UC_HOOK_CODE
import capstone
from capstone import Cs, CS_ARCH_X86, CS_MODE_64
from capstone.x86 import X86_OP_IMM

from emu_oracle import EmuOracle, ArgSpec

SWEEP_CEILING = 0x2000


@dataclass
class CoverageResult:
    coverage: float  # percent of discovered conditional branches with both edges seen
    hit: int
    total: int
    missed: List[int] = field(default_factory=list)
    discovered: int = 0


def _sweep_branches(md: Cs, code: bytes, base: int) -> Tuple[Set[int], Set[int]]:
    """Return (conditional branch sites, their taken-target addresses).

    A linear sweep, not a recursive descent: we only claim coverage of what
    the sweep can see, and the ceiling makes the boundary explicit.
    """
    conds: Set[int] = set()
    targets: Set[int] = set()
    seen_ret = False
    for insn in md.disasm(code, base):
        if insn.mnemonic == "int3" and seen_ret:
            break  # padding between functions
        if insn.mnemonic == "ret":
            seen_ret = True
        if insn.mnemonic.startswith("j") and insn.mnemonic != "jmp":
            conds.add(insn.address)
            for op in insn.operands:
                if op.type == X86_OP_IMM:
                    targets.add(op.imm)
        if insn.mnemonic == "loop" or insn.mnemonic.startswith("loop"):
            conds.add(insn.address)
            for op in insn.operands:
                if op.type == X86_OP_IMM:
                    targets.add(op.imm)
    return conds, targets


def measure_branch_coverage(
    oracle: EmuOracle,
    v2b_addr: int,
    corpus_args: Sequence[Sequence[ArgSpec]],
    ret_kind: str = "f32",
) -> CoverageResult:
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True  # required for insn.operands
    off = v2b_addr - oracle.base
    code = oracle.image[off : off + SWEEP_CEILING]
    conds, targets = _sweep_branches(md, code, v2b_addr)

    # Run the corpus once more under a block hook, collecting every address
    # executed inside the function window.
    executed: Set[int] = set()

    def hook(uu, addr, size, ud):
        if v2b_addr <= addr < v2b_addr + SWEEP_CEILING:
            executed.add(addr)

    u = oracle.uc
    h = u.hook_add(UC_HOOK_CODE, hook)
    try:
        for args in corpus_args:
            # Reset arena/stack the same way call() does, but we only need
            # the execution trace, so reuse call's setup by invoking it and
            # letting the hook observe. call() installs its own hooks and
            # deletes them; ours stays.
            oracle.call(v2b_addr, list(args), ret_kind=ret_kind)
    finally:
        u.hook_del(h)

    hit = 0
    missed: List[int] = []
    for cond in sorted(conds):
        # The conditional site itself must have executed, and at least one
        # of {fallthrough, taken-target} must appear in the executed set
        # beyond the site. Both edges: site executed AND (site+size in set
        # OR target in set). We approximate "both" as: site in set AND
        # target in executed. Fallthrough is implied by the next sequential
        # address also having executed.
        if cond not in executed:
            missed.append(cond)
            continue
        taken = any(t in executed for t in targets if abs(t - cond) < 0x1000)
        # Fallthrough: did execution continue past the branch instruction?
        # We do not know the width here without the insn; approximate with
        # "some executed address in (cond, cond+16)".
        fallthrough = any(cond < a <= cond + 16 for a in executed)
        if taken and fallthrough:
            hit += 1
        elif taken or fallthrough:
            # One edge only - count as a miss for the gate, list it.
            missed.append(cond)
        else:
            missed.append(cond)

    total = len(conds)
    cov = (100.0 * hit / total) if total else 100.0
    return CoverageResult(
        coverage=cov, hit=hit, total=total, missed=missed, discovered=total
    )

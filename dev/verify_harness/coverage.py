"""Basic-block / branch coverage of the oracle, measured on the very runs
that were compared.

The Sandustry harness uses istanbul on the JS oracle; here the oracle is
machine code under Unicorn, so we take a different measurement of the same
claim: which conditional-branch EDGES the compared executions actually took,
against every conditional branch in the function body. Agreement on branches
nobody reached proves nothing.

Branch sites are discovered by a linear sweep from the entry with capstone,
bounded by the function's .pdata RUNTIME_FUNCTION entry when the image
carries one, else by the first unconditional jump past the function / a run
of int3, and at a hard byte ceiling. A sweep that cannot find the function
end reports that honestly rather than guessing.

Edge attribution is EXACT, not approximated: a per-instruction hook remembers
the previously executed instruction, and whenever that was a discovered
conditional branch, the address actually executed next IS the resolved edge
(fallthrough or target). A branch counts as covered only when both of its
edges were observed on the compared runs. The previous implementation kept
one function-wide target set and credited "taken" to any branch near an
executed target - nearby branches that shared a target or never ran could be
counted covered, over-reporting coverage and weakening the gate.
"""

from __future__ import annotations

import struct
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Sequence, Set, Tuple

from unicorn import UC_HOOK_CODE
import capstone
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

from emu_oracle import EmuOracle, ArgSpec

SWEEP_CEILING = 0x2000


@dataclass
class CoverageResult:
    coverage: float  # percent of discovered conditional branches with both edges seen
    hit: int
    total: int
    missed: List[int] = field(default_factory=list)
    discovered: int = 0
    # Branch sites a unit declared unreachable, after the validation below
    # accepted them. They are reported, never silently dropped.
    declared_unreachable: int = 0
    # Declarations that did not hold (site absent, both edges seen, dead edge
    # observed, or no rationale). Any entry here fails the unit: a declaration
    # must describe a fact, not assert one.
    invalid_declarations: List[str] = field(default_factory=list)


def _sweep_branches(md: Cs, code: bytes, base: int) -> Set[int]:
    """Return the conditional branch sites in `code`.

    A linear sweep, not a recursive descent: we only claim coverage of what
    the sweep can see, and the ceiling makes the boundary explicit.
    """
    conds: Set[int] = set()
    seen_ret = False
    for insn in md.disasm(code, base):
        if insn.mnemonic == "int3" and seen_ret:
            break  # padding between functions
        if insn.mnemonic == "ret":
            seen_ret = True
        if insn.mnemonic.startswith("j") and insn.mnemonic != "jmp":
            conds.add(insn.address)
        elif insn.mnemonic.startswith("loop"):
            conds.add(insn.address)
    return conds


def _pdata_function_bounds(image: bytes, rva: int) -> Optional[Tuple[int, int]]:
    """Exact [begin, end) of the function containing `rva`, from .pdata.

    The offline image is laid out with file offset == RVA, so the PE headers
    can be walked in place. Returns None (and the caller falls back to the
    heuristic sweep bound) when the image has no PE/.pdata - captures of some
    builds do not - or when no RUNTIME_FUNCTION covers `rva`.
    """
    if len(image) < 0x200 or image[:2] != b"MZ":
        return None
    try:
        e_lfanew = struct.unpack_from("<I", image, 0x3C)[0]
        if e_lfanew + 24 > len(image) or image[e_lfanew : e_lfanew + 4] != b"PE\x00\x00":
            return None
        coff = e_lfanew + 4
        num_sections = struct.unpack_from("<H", image, coff + 2)[0]
        opt_size = struct.unpack_from("<H", image, coff + 16)[0]
        sec_off = coff + 20 + opt_size
        pdata_va = pdata_size = 0
        for i in range(num_sections):
            s = sec_off + 40 * i
            if s + 40 > len(image):
                return None
            name = bytes(image[s : s + 8]).rstrip(b"\x00")
            vsize, va = struct.unpack_from("<II", image, s + 8)
            if name == b".pdata":
                pdata_va, pdata_size = va, min(vsize, len(image) - va)
                break
        if not pdata_va or pdata_size < 12:
            return None

        def entry(k: int) -> Optional[Tuple[int, int]]:
            off = pdata_va + 12 * k
            if off + 12 > len(image):
                return None
            b0, e0, _u = struct.unpack_from("<III", image, off)
            return b0, e0

        lo, hi = 0, pdata_size // 12 - 1
        while lo <= hi:
            mid = (lo + hi) // 2
            e = entry(mid)
            if e is None:
                return None
            b0, e0 = e
            if e0 <= b0:  # malformed entry; do not trust the table
                return None
            if rva < b0:
                hi = mid - 1
            elif rva >= e0:
                lo = mid + 1
            else:
                return b0, e0
    except struct.error:
        return None
    return None


class BranchTracker:
    """Records exactly which branch edges executed, across a series of
    oracle calls. Install it before the runs that are compared, close it
    after; `result()` then reports both-edge coverage."""

    def __init__(self, oracle: EmuOracle, v2b_addr: int, window: Optional[int] = None):
        self.oracle = oracle
        self.base_addr = v2b_addr
        self.window = window or SWEEP_CEILING
        md = Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        off = v2b_addr - oracle.base
        code = bytes(oracle.image[off : off + self.window])
        self.conds: Set[int] = _sweep_branches(md, code, v2b_addr)
        # branch site -> set of next-executed addresses (the observed edges)
        self.edges: Dict[int, Set[int]] = {c: set() for c in self.conds}
        self._prev: Optional[int] = None
        self._hooks: List[Tuple[EmuOracle, int]] = []
        self.attach(oracle)

    def attach(self, oracle: EmuOracle) -> None:
        """Install the same recorder on another oracle instance.

        A unit that builds a fresh oracle per case (to keep a client-side
        accumulator from leaking between cases) still measures one edge set:
        the runs are independent, the coverage is not."""
        self._hooks.append((oracle, oracle.uc.hook_add(UC_HOOK_CODE, self._on_insn)))

    def _on_insn(self, uu, addr, size, ud):
        prev = self._prev
        if prev is not None and prev in self.conds:
            # `addr` is the instruction actually executed after the branch:
            # the resolved edge, exact, whatever it was.
            self.edges[prev].add(addr)
        self._prev = addr

    def begin_call(self) -> None:
        """Call before each oracle.call: a fault can abort emulation right
        after a conditional branch, and without the reset the next call's
        first instruction would be misattributed as that branch's edge."""
        self._prev = None

    def result(
        self,
        unreachable: Optional[Sequence[Tuple[int, int, str]]] = None,
    ) -> CoverageResult:
        """Both-edge coverage, with a unit's declared dead edges validated.

        A declaration names a branch site and the edge that cannot execute
        from the function's entry, plus the reason. It is accepted only when
        the site is a discovered conditional branch, it WAS executed (so the
        exclusion cannot hide a branch nobody reached), exactly one edge was
        observed, and that edge is not the one declared dead. Anything else
        lands in ``invalid_declarations`` and the runner fails the unit.
        """
        declared: Set[int] = set()
        invalid: List[str] = []
        for site, dead_edge, why in unreachable or ():
            if site not in self.conds:
                invalid.append(f"{site:#x} is not a conditional branch in the sweep")
                continue
            seen = self.edges.get(site, set())
            if dead_edge in seen:
                invalid.append(
                    f"{site:#x}: declared dead edge {dead_edge:#x} was observed"
                )
                continue
            if len(seen) != 1:
                invalid.append(
                    f"{site:#x}: declared unreachable needs exactly one observed "
                    f"edge (site executed with the other edge dead), saw {len(seen)}"
                )
                continue
            if not why.strip():
                invalid.append(f"{site:#x}: no rationale for the dead edge")
                continue
            declared.add(site)

        hit = 0
        missed: List[int] = []
        for cond in sorted(self.conds):
            if cond in declared:
                continue
            seen = self.edges.get(cond, set())
            if len(seen) >= 2:
                hit += 1
            else:
                missed.append(cond)
        total = len(self.conds) - len(declared)
        cov = (100.0 * hit / total) if total else 100.0
        return CoverageResult(
            coverage=cov,
            hit=hit,
            total=total,
            missed=missed,
            discovered=len(self.conds),
            declared_unreachable=len(declared),
            invalid_declarations=invalid,
        )

    def close(self) -> None:
        for oracle, hook in self._hooks:
            try:
                oracle.uc.hook_del(hook)
            except Exception:
                pass
        self._hooks.clear()


def measure_branch_coverage(
    oracle: EmuOracle,
    v2b_addr: int,
    corpus_args: Sequence[Sequence[ArgSpec]],
    ret_kind: str = "f32",
) -> CoverageResult:
    """Standalone helper: run `corpus_args` through the oracle under a
    BranchTracker and report both-edge coverage. The runner instead installs
    the tracker around the differential stage itself, so the compared runs
    and the measured runs are literally the same executions."""
    off = v2b_addr - oracle.base
    bounds = _pdata_function_bounds(oracle.image, off)
    window = min(SWEEP_CEILING, bounds[1] - bounds[0]) if bounds else SWEEP_CEILING
    tracker = BranchTracker(oracle, v2b_addr, window=window)
    try:
        for args in corpus_args:
            tracker.begin_call()
            oracle.call(v2b_addr, list(args), ret_kind=ret_kind)
        return tracker.result()
    finally:
        tracker.close()

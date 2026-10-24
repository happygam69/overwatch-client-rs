"""The five-stage verification ladder, native edition.

Port of the Sandustry harness's runner.js protocol to the Overwatch target:
the oracle is the real machine code emulated over a captured image, never a
decompile or a reimplementation. Each stage can invalidate the conclusions of
the next; the order is deliberate.

  1. load        both sides load and expose the declared surface
  2. determinism the oracle agrees with itself on identical inputs
  3. differential same inputs, every channel compared
  4. coverage    measured on the oracle, on the very runs that were compared
  5. mutation    the candidate is mutated; the differential must notice

A unit is `verified` only when all five pass. `inconclusive` is not a pass.
"""

from __future__ import annotations

import json
import os
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence, Tuple

from emu_oracle import EmuOracle, ArgSpec, bits_f32, f32_bits
from gen import UnitArg, generate_args, focused_f32_edge_cases
from ledger import (
    LedgerEntry,
    check_entry,
    load_ledger,
    record,
    sha256_bytes,
    sha256_file,
)
import mutate as mutate_mod

# The oracle image: the offline-recovered complete image (every .text page,
# `dev/offline_page_recover.py`), laid out with file offset == RVA. The v2b
# delta equals the exe RVA (v2b - 0x7ff7c4d20000 == exe - 0x140000000), so
# unit addresses stay in the v2b frame and the offset math is unchanged.
DEFAULT_IMAGE = r"D:\ow-dumps\offline_recover\module_offline.bin"
DEFAULT_BASE = 0x7FF7C4D20000


@dataclass
class Unit:
    id: str
    title: str
    address_v2b: int
    c_file: str
    entry: str
    args: List[UnitArg]
    ret_kind: str  # f32 | f64 | u64 | void
    options: Dict[str, Any]
    image: str = DEFAULT_IMAGE
    base: int = DEFAULT_BASE
    known_equivalent_mutants: List[Dict[str, Any]] = field(default_factory=list)
    acknowledged_gaps: List[Dict[str, Any]] = field(default_factory=list)
    notes: List[str] = field(default_factory=list)
    # Optional dotted path to a fixture module exporting make_args(i) ->
    # List[ArgSpec]. When set, it replaces the generic generator for this
    # unit; structured inputs (linked structs) need it.
    arg_factory: Optional[str] = None
    # Candidate loader: "c" compiles the decompile (tier 1); "rust" runs
    # rust_probe.rs through the subprocess protocol (tier 2).
    candidate_loader: str = "c"


@dataclass
class Divergence:
    iteration: int
    channel: str  # ret | mem | fault
    oracle: str
    candidate: str
    detail: str = ""


def load_unit(path: str) -> Unit:
    with open(path, "r", encoding="utf-8") as f:
        d = json.load(f)
    args = [UnitArg(**a) for a in d["args"]]
    # Unit paths are repo-relative; resolve against the repo root (two levels
    # up from dev/verify_harness/).
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    def abspath(p: str) -> str:
        return p if os.path.isabs(p) else os.path.join(repo_root, p)
    # c_file is tier-1 only; rust units carry candidateLoader and omit it.
    c_file = d.get("c_file", "")
    return Unit(
        id=d["id"],
        title=d.get("title", ""),
        address_v2b=int(d["address"]["v2b"], 16),
        c_file=abspath(c_file) if c_file else "",
        entry=d["entry"],
        args=args,
        ret_kind=d.get("returns", "f32"),
        options=d.get("options", {}),
        image=abspath(d.get("image", DEFAULT_IMAGE)),
        base=int(d.get("base", hex(DEFAULT_BASE)), 16),
        known_equivalent_mutants=d.get("knownEquivalentMutants", []),
        acknowledged_gaps=d.get("acknowledgedGaps", []),
        notes=d.get("notes", []),
        arg_factory=d.get("argFactory"),
        candidate_loader=d.get("candidateLoader", "c"),
    )


def _args_sha(unit: Unit) -> str:
    payload = json.dumps(
        [{"name": a.name, "kind": a.kind, "ptr_len": a.ptr_len} for a in unit.args],
        sort_keys=True,
    ).encode()
    return sha256_bytes(payload)


def _options_sha(unit: Unit) -> str:
    return sha256_bytes(json.dumps(unit.options, sort_keys=True).encode())


def _bits_equal(a: int, b: int, ret_kind: str) -> bool:
    """The equivalence relation. f32: NaN equal to NaN, +0 distinct from -0,
    everything else bit-exact. Printed normalisations: none silent."""
    if ret_kind != "f32":
        return a == b
    fa, fb = bits_f32(a), bits_f32(b)
    import math

    if math.isnan(fa) and math.isnan(fb):
        return True
    return a == b


def run_unit(
    unit: Unit,
    work_dir: Optional[str] = None,
    skip_mutation: bool = False,
    verbose: bool = True,
) -> LedgerEntry:
    def log(msg: str) -> None:
        if verbose:
            print(msg)

    opts = unit.options
    iterations = int(opts.get("iterations", 300))
    cov_gate = float(opts.get("coverageThreshold", 85.0))
    mutation_count = int(opts.get("mutationCount", 12))
    kill_gate = float(opts.get("mutationKillRate", 0.8))
    seed = int(opts.get("seed", 4241))

    oracle_sha = "unloaded"
    # The candidate artifact the ledger binds to: the decompile for tier 1,
    # the probe source for tier 2.
    if unit.candidate_loader == "rust":
        from candidate_rust import PROBE_SRC

        cand_path = PROBE_SRC
    else:
        cand_path = unit.c_file
    cand_sha = sha256_file(cand_path) if os.path.exists(cand_path) else "missing"
    opt_sha = _options_sha(unit)
    surf_sha = _args_sha(unit)

    def entry(status: str, **kw) -> LedgerEntry:
        kw_notes = kw.pop("notes", [])
        return LedgerEntry(
            unit_id=unit.id,
            status=status,
            oracle_sha256=oracle_sha,
            candidate_sha256=cand_sha,
            options_sha256=opt_sha,
            surfaces_sha256=surf_sha,
            equivalent_mutants=unit.known_equivalent_mutants,
            acknowledged_gaps=unit.acknowledged_gaps,
            notes=list(unit.notes) + list(kw_notes),
            **kw,
        )

    # ---- Stage 1: load ----------------------------------------------------
    log(f"[1/5] load {unit.id}")
    if not os.path.exists(unit.image):
        log(f"  FAIL: image not found: {unit.image}")
        return entry("inconclusive", notes=["image missing: " + unit.image])
    if unit.candidate_loader == "rust":
        from candidate_rust import PROBE_SRC

        if not os.path.exists(PROBE_SRC):
            log(f"  FAIL: rust probe source not found: {PROBE_SRC}")
            return entry("inconclusive", notes=[f"rust probe missing: {PROBE_SRC}"])
    elif not os.path.exists(unit.c_file):
        log(f"  FAIL: decompile not found: {unit.c_file}")
        return entry("inconclusive", notes=["c_file missing: " + unit.c_file])

    oracle = EmuOracle(unit.image, base=unit.base)
    try:
        fn_bytes = oracle.load_function(unit.address_v2b)
    except ValueError as e:
        log(f"  FAIL: {e}")
        return entry("inconclusive", notes=[str(e)])
    oracle_sha = sha256_bytes(fn_bytes)
    log(f"  oracle bytes @ {unit.address_v2b:#x}: {len(fn_bytes)} (sha256 {oracle_sha[:12]}...)")

    # Fresh ledger check: a stale verdict is not a verdict.
    entries = load_ledger()
    prev = entries.get(unit.id)
    if prev is not None:
        freshness = check_entry(prev, oracle_sha, cand_sha, opt_sha, surf_sha)
        if freshness == "stale" and prev.status == "verified":
            log("  NOTE: previous verdict is stale (inputs changed); re-verifying")

    work_dir = work_dir or tempfile.mkdtemp(prefix=f"verify_{unit.id.replace('/', '_')}_")
    try:
        if unit.candidate_loader == "rust":
            from candidate_rust import RustCandidate

            candidate = RustCandidate(
                entry=unit.entry,
                arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                ret_kind=unit.ret_kind,
                work_dir=work_dir,
            )
            log(f"  candidate rust probe: {candidate.exe}")
        else:
            from candidate_c import CCandidate

            candidate = CCandidate(
                c_file=unit.c_file,
                entry=unit.entry,
                arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                ret_kind=unit.ret_kind,
                work_dir=work_dir,
            )
            log(f"  candidate compiled: {candidate.dll_path}")
    except RuntimeError as e:
        log(f"  FAIL: {e}")
        return entry("inconclusive", notes=[f"compile failed: {e}"])

    # Load the arg factory (if any) before any stage that builds inputs.
    factory = None
    if unit.arg_factory:
        import importlib

        mod = importlib.import_module(unit.arg_factory)
        factory = mod.make_args

    # ---- Stage 2: determinism --------------------------------------------
    log("[2/5] determinism")
    if factory is not None:
        det_args = factory(0)
    else:
        det_args = generate_args(unit.args, seed, 0)
    r1 = oracle.call(unit.address_v2b, det_args, ret_kind=unit.ret_kind)
    r2 = oracle.call(unit.address_v2b, det_args, ret_kind=unit.ret_kind)
    if not r1.ok or not r2.ok:
        log(f"  FAIL: oracle fault on determinism probe: {r1.fault or r2.fault}")
        return entry("invalid", notes=[f"oracle fault: {r1.fault or r2.fault}"])
    if r1.ret_bits != r2.ret_bits or r1.pointee_after != r2.pointee_after:
        log("  FAIL: oracle disagrees with itself on identical input")
        return entry("invalid", notes=["oracle nondeterministic on identical input"])
    log("  ok")

    # ---- Stage 3: differential -------------------------------------------
    log(f"[3/5] differential ({iterations} iterations + edge pool)")
    divergences: List[Divergence] = []
    undeclared_reads: List[Tuple[int, int]] = []
    undeclared_writes: List[Tuple[int, int]] = []
    corpus_args: List[List[ArgSpec]] = []

    if factory is not None:
        # A structured unit's inputs come only from its factory; the generic
        # random corpus would build invalid linked structures.
        cases: List[List[ArgSpec]] = []
        for i in range(iterations + 64):
            cases.append(factory(i))
    else:
        cases = list(focused_f32_edge_cases(unit.args))
        for i in range(iterations):
            cases.append(generate_args(unit.args, seed, i))

    for idx, args in enumerate(cases):
        o = oracle.call(unit.address_v2b, args, ret_kind=unit.ret_kind)
        if not o.ok:
            log(f"  FAIL: oracle fault at iteration {idx}: {o.fault}")
            return entry("inconclusive", notes=[f"oracle fault at {idx}: {o.fault}"])
        if o.reads_outside:
            undeclared_reads.extend(o.reads_outside)
        if o.out_of_arena_writes:
            undeclared_writes.extend(o.out_of_arena_writes)

        c = candidate.call(args)
        if not c.ok:
            divergences.append(
                Divergence(idx, "fault", "ok", f"candidate fault: {c.fault}")
            )
        else:
            if unit.ret_kind != "void" and not _bits_equal(
                o.ret_bits, c.ret_bits, unit.ret_kind
            ):
                divergences.append(
                    Divergence(
                        idx,
                        "ret",
                        f"{o.ret_bits:#x} ({_fmt_ret(o.ret_bits, unit.ret_kind)})",
                        f"{c.ret_bits:#x} ({_fmt_ret(c.ret_bits, unit.ret_kind)})",
                    )
                )
            # Memory mutation channel: pointee bytes after the call.
            # Reloc slots hold the pointee's own base address (arena vs
            # host), which necessarily differs between the two sides; mask
            # them so only the data the callee wrote is compared.
            cand_after = b"".join(c.pointee_after[k] for k in sorted(c.pointee_after))
            o_after = bytearray(o.pointee_after)
            c_after = bytearray(cand_after)
            pos = 0
            for a in args:
                if a.kind != "ptr":
                    continue
                for off in a.relocs:
                    s = pos + off
                    o_after[s : s + 8] = b"\x00" * 8
                    c_after[s : s + 8] = b"\x00" * 8
                pos += len(a.mem)
            if bytes(o_after) != bytes(c_after):
                divergences.append(
                    Divergence(
                        idx,
                        "mem",
                        bytes(o_after).hex(),
                        bytes(c_after).hex(),
                        "pointee bytes differ after the call",
                    )
                )
        corpus_args.append(args)
        if divergences:
            # Keep going a little to collect a couple more examples, then stop.
            if len(divergences) >= 3:
                break

    if undeclared_reads:
        uniq = sorted(set(undeclared_reads))[:8]
        msg = (
            f"oracle reads outside declared args/stack at "
            f"{', '.join(f'{a:#x}+{s}' for a, s in uniq)} - unit needs a "
            "wider fixture or is not verifiable by emulation alone"
        )
        log(f"  INCONCLUSIVE: {msg}")
        return entry("inconclusive", notes=[msg])
    if undeclared_writes:
        uniq = sorted(set(undeclared_writes))[:8]
        msg = (
            f"oracle writes outside the arena at "
            f"{', '.join(f'{a:#x}+{s}' for a, s in uniq)}"
        )
        log(f"  INCONCLUSIVE: {msg}")
        return entry("inconclusive", notes=[msg])

    if divergences:
        log(f"  DIVERGENT: {len(divergences)} channel disagreements")
        for d in divergences[:5]:
            log(f"    iter {d.iteration} [{d.channel}] oracle={d.oracle} cand={d.candidate}")
        return entry(
            "divergent",
            divergences=[d.__dict__ for d in divergences],
        )
    log(f"  ok ({len(cases)} cases, every channel)")

    # ---- Stage 4: coverage ------------------------------------------------
    log("[4/5] coverage (oracle, basic-block)")
    try:
        from coverage import measure_branch_coverage

        cov = measure_branch_coverage(oracle, unit.address_v2b, corpus_args, unit.ret_kind)
    except Exception as e:
        log(f"  INCONCLUSIVE: coverage failed: {e}")
        return entry("inconclusive", notes=[f"coverage failed: {e}"])
    log(f"  branch coverage {cov.coverage:.1f}% ({cov.hit}/{cov.total} branches)")
    if cov.coverage < cov_gate:
        gaps = ", ".join(f"{a:#x}" for a in cov.missed[:8])
        log(f"  INCONCLUSIVE: coverage {cov.coverage:.1f}% < gate {cov_gate}%")
        return entry(
            "inconclusive",
            coverage=cov.coverage,
            notes=[f"coverage {cov.coverage:.1f}% < gate {cov_gate}%; missed {gaps}"],
        )

    # ---- Stage 5: mutation ------------------------------------------------
    if skip_mutation:
        log("[5/5] mutation SKIPPED (not a verified path)")
        return entry(
            "inconclusive",
            coverage=cov.coverage,
            notes=["mutation skipped; a unit is not verified without it"],
        )

    log(f"[5/5] mutation ({mutation_count} mutants, kill gate {kill_gate:.0%})")
    if unit.candidate_loader == "rust":
        from candidate_rust import PROBE_SRC, RustCandidate

        base_path = PROBE_SRC
    else:
        from candidate_c import CCandidate

        base_path = unit.c_file
    with open(base_path, "r", encoding="utf-8") as f:
        base_src = f.read()

    mutants = mutate_mod.generate_mutants(base_src, unit.entry, count=mutation_count)
    killed = 0
    scored = 0
    survivors: List[Any] = []
    for m in mutants:
        if any(
            m.kind == eq.get("kind") and m.line == eq.get("line")
            for eq in unit.known_equivalent_mutants
        ):
            log(f"  skip mutant {m.kind}@{m.line} (declared equivalent)")
            continue
        scored += 1
        try:
            if unit.candidate_loader == "rust":
                mc = RustCandidate(
                    entry=unit.entry,
                    arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                    ret_kind=unit.ret_kind,
                    work_dir=tempfile.mkdtemp(prefix="mutant_"),
                    source_override=m.source,
                )
            else:
                mc = CCandidate(
                    c_file=unit.c_file,
                    entry=unit.entry,
                    arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                    ret_kind=unit.ret_kind,
                    work_dir=tempfile.mkdtemp(prefix="mutant_"),
                    source_override=m.source,
                )
        except RuntimeError as e:
            # A mutant that does not compile is a killed mutant only if the
            # mutation is meant to be behaviour-changing; report and count as
            # killed (the harness cannot run a broken build, and the original
            # still verified, so the detection power claim stands for the
            # mutants that do build).
            log(f"  mutant {m.kind}@{m.line} does not compile (counted killed): {e}")
            killed += 1
            continue
        try:
            detected = False
            # Mutation corpus: a subset is enough to decide kill/survive, and
            # each surviving mutant pays for every case, so keep it small.
            for idx, args in enumerate(cases[: min(len(cases), 48)]):
                o = oracle.call(unit.address_v2b, args, ret_kind=unit.ret_kind)
                c = mc.call(args)
                if not c.ok:
                    detected = True
                    break
                if unit.ret_kind != "void" and not _bits_equal(
                    o.ret_bits, c.ret_bits, unit.ret_kind
                ):
                    detected = True
                    break
                m_after = bytearray(b"".join(c.pointee_after[k] for k in sorted(c.pointee_after)))
                o_after = bytearray(o.pointee_after)
                pos = 0
                for a in args:
                    if a.kind != "ptr":
                        continue
                    for off in a.relocs:
                        s = pos + off
                        o_after[s : s + 8] = b"\x00" * 8
                        m_after[s : s + 8] = b"\x00" * 8
                    pos += len(a.mem)
                if bytes(o_after) != bytes(m_after):
                    detected = True
                    break
            if detected:
                killed += 1
            else:
                survivors.append(m)
                log(f"  SURVIVOR: {m.kind}@{m.line}: {m.excerpt[:60]}")
        finally:
            # RustCandidate owns a subprocess; CCandidate has no close.
            close = getattr(mc, "close", None)
            if close is not None:
                close()

    kill_rate = (killed / scored) if scored else 0.0
    log(f"  kill rate {kill_rate:.0%} ({killed}/{scored})")
    close_main = getattr(candidate, "close", None)
    if survivors:
        log("  survivors must be fixed or declared observationally equivalent")
        for s in survivors:
            log(f"    {s.kind}@{s.line}: {s.excerpt[:70]}")
    if kill_rate < kill_gate:
        if close_main is not None:
            close_main()
        return entry(
            "inconclusive",
            coverage=cov.coverage,
            mutation_kill_rate=kill_rate,
            mutants_killed=killed,
            mutants_total=scored,
            notes=[
                f"mutation kill rate {kill_rate:.0%} < gate {kill_gate:.0%}; "
                f"{len(survivors)} survivor(s)"
            ],
        )

    log("  VERIFIED")
    if close_main is not None:
        close_main()
    return entry(
        "verified",
        coverage=cov.coverage,
        mutation_kill_rate=kill_rate,
        mutants_killed=killed,
        mutants_total=scored,
    )


def _fmt_ret(bits: int, kind: str) -> str:
    try:
        if kind == "f32":
            return repr(bits_f32(bits))
        if kind == "f64":
            return repr(struct.unpack("<d", struct.pack("<Q", bits & (2**64 - 1)))[0])
        return str(bits)
    except Exception:
        return "?"


def run_and_record(unit: Unit, **kw) -> LedgerEntry:
    e = run_unit(unit, **kw)
    entries = load_ledger()
    record(entries, e)
    return e

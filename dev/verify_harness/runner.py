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

A unit without an `argFactory` can set `options.autoCorpus`: when the branch
coverage measured on the compared runs is still under the gate after the base
corpus, the runner compares more draws from the same deterministic (seed, i)
sequence until the gate is met or the corpus cap is reached (stage 3's growth
loop). Growth only adds compared cases - it cannot mask a divergence, weaken
a gate, or replace a fixture - and the realized corpus is recorded in the
ledger entry (digest + case count) for audit.

`run_and_record(..., use_cache=True)` replays a ledger entry instead of
re-running the stages when every digest the ledger binds is unchanged (see
`_fresh_cached_entry`); the stages themselves are never skipped inside a run.
"""

from __future__ import annotations

import json
import math
import os
import shutil
import struct
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field, replace
from typing import Any, Dict, List, Optional, Sequence, Tuple

from emu_oracle import EmuOracle, ArgSpec, bits_f32, f32_bits, function_bytes
from gen import UnitArg, generate_args, focused_f32_edge_cases
from ledger import (
    HARNESS_VERSION,
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
LOCAL_IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
    "dev",
    "data",
    "offline_recover",
    "module_offline.bin",
)
# The canonical copy is repo-local (gitignored). The one-time copy that lived on
# the dumps drive is retired: every unit JSON now cites this path, and the bytes
# are pinned by sha256 in `emu_oracle.IMAGE_SHA256`, so a substituted image fails
# instead of being used.
DEFAULT_IMAGE = LOCAL_IMAGE
FALLBACK_IMAGE = r"D:\ow-dumps\offline_recover\module_offline.bin"
DEFAULT_BASE = 0x7FF7C4D20000


_WARNED_IMAGES: set = set()


def resolve_image(path: str) -> str:
    if os.path.exists(path):
        return path
    for alt in (FALLBACK_IMAGE, LOCAL_IMAGE):
        if alt != path and os.path.exists(alt):
            if path not in _WARNED_IMAGES:
                _WARNED_IMAGES.add(path)
                _warn(f"image {path} not found; using the same recovery image at {alt}")
            return alt
    return path

SHIM_PATH = os.path.join(os.path.dirname(__file__), "ghidra_shim.h")

# Mutation-stage budget. A mutant is broken code by construction, so an
# unbounded loop is an expected outcome, not a slow call: the mutant is killed
# at the first case that fails, and a timeout is a failure like any other.
# The default is >1000x the slowest observed non-hanging mutant call (see
# PERFORMANCE.md); the baseline candidate keeps DEFAULT_CALL_TIMEOUT.
DEFAULT_MUTATION_CALL_TIMEOUT = 10.0
# The pinned mutation corpus: the first 48 cases (declared specials first,
# then the focused edges). Do not lengthen - a mutant only has to be caught
# once. The one exception is a grown corpus (autoCorpus): the growth tail is
# appended to the mutation corpus, because a mutant can be distinguishable
# only on a branch a grown case uncovered, which the corpus head never sees.
MUTATION_CORPUS_CASES = 48
DEFAULT_MUTANT_JOBS = 4

# How many mutation sites the stale-declaration check scans. A unit probes
# `options.mutationCount` of them, but a declaration may name a site past
# that window (it is then inert, not wrong), so the check looks at every
# site the mutator can produce in the entry function.
MUTATION_SITE_SCAN = 4096


def _declares(eq: Dict[str, Any], m: Any) -> bool:
    """Does a `knownEquivalentMutants` entry name this mutant?

    The kind and the line are the key. A line can carry two mutants of the
    same kind (a decimal and a hex constant each get a boundary shift), so a
    declaration may add the mutated text to pick one of them; without it the
    declaration covers every mutant of that kind on that line.
    """
    if eq.get("kind") != m.kind or eq.get("line") != m.line:
        return False
    want = eq.get("mutated")
    return not want or want == getattr(m, "mutated", "")


def _decl_key(decls: Sequence[Dict[str, Any]]) -> List[Tuple[Any, Any, Any]]:
    """The part of a declaration that decides which mutants are skipped.

    The rationale is prose and is not part of the key: re-wording a
    rationale does not change what the mutation stage skips, so it does not
    invalidate a verdict. Which mutants are skipped does.
    """
    return sorted(
        (d.get("kind"), d.get("line"), d.get("mutated") or "")
        for d in decls
    )

# Coverage-guided corpus growth (`options.autoCorpus`, fixture-less units).
# The base corpus is the focused edge pool plus `iterations` seeded draws;
# when the branch coverage measured on those very runs is still under the
# gate, more draws from the same deterministic sequence are compared until
# the gate is met or `cap` cases exist in total. Growth only runs after the
# base corpus agreed on every channel, so it cannot mask a divergence, and
# the coverage it measures is the runs themselves, so it cannot inflate it.
DEFAULT_CORPUS_CAP = 1200
DEFAULT_CORPUS_STEP = 50

# Unit descriptor keys this harness understands. A typo'd key ("argfactory",
# "candidateLoader ":"rust ") would otherwise be silently ignored and the
# unit would quietly verify with a weaker surface - warn loudly instead.
_KNOWN_UNIT_KEYS = {
    "id", "title", "address", "image", "c_file", "entry", "args",
    "returns", "options", "base", "knownEquivalentMutants",
    "acknowledgedGaps", "notes", "argFactory", "candidateLoader", "state",
    "scaffold", "candidateCrate", "candidateProbe", "suite", "startRegs",
    "steps",
}
_KNOWN_ARG_KEYS = {"name", "kind", "ptr_len"}
_KNOWN_OPTION_KEYS = {
    "iterations", "coverageThreshold", "mutationCount", "mutationKillRate",
    "seed", "allowedImageReads", "allowedImageWrites", "unreachableBranches", "mutationCallTimeout",
    "autoCorpus",
    # Rebuild the emulator for every corpus case. A callee that leaves state
    # behind (an allocator the arena reset does not restore, say) then cannot
    # make one case's answer depend on the previous one. Slower, and only for
    # units that need it.
    "freshOraclePerCase",
    # A unit-declared exact function extent, for leaf functions the image's
    # .pdata does not describe. Without it the sweep uses the 0x2000 ceiling
    # and would disassemble trailing jump/dispatch tables as code.
    "coverageWindow",
}
_ARG_KINDS = ("f32", "f64", "u64")
_RET_KINDS = ("f32", "f64", "u64", "vec4", "u8", "void")


@dataclass
class StateSpec:
    """A controlled memory snapshot for a stateful unit.

    The snapshot is written at ``base`` before every oracle call, the whole
    region is compared after the call (the state channel), and the region is
    restored before the next call. ``data`` is the snapshot bytes; the file it
    came from is part of the surfaces digest.
    """

    base: int
    data: bytes
    source: str = ""


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
    # rust_probe.rs through the subprocess protocol (tier 2, legacy);
    # "cargo" builds crates/<crate>/src/bin/verify_probe.rs and runs the real
    # crate functions (tier 2, shipped path). `candidate_crate` and
    # `candidate_probe` select that crate/probe (default ow-sim).
    candidate_loader: str = "c"
    candidate_crate: str = ""
    candidate_probe: str = ""
    # v2b addresses of compiler literal-pool constants the decompile
    # already inlined (e.g. 1.0f, -0.0f for XORPS). Reads of these are
    # not "undeclared global state"; the candidate hardcodes the same
    # value. Declared per unit so the exception is visible, not silent.
    allowed_image_reads: List[int] = field(default_factory=list)
    # Inclusive ranges of allowed image reads, for state/global blocks. The
    # unit's own state region is added automatically.
    allowed_read_ranges: List[Tuple[int, int]] = field(default_factory=list)
    # Inclusive ranges of image addresses the callee may write. The client
    # keeps some runtime statistics in the image's zero-fill region, and a
    # unit that proves a leaf which updates one declares the global here.
    allowed_write_ranges: List[Tuple[int, int]] = field(default_factory=list)
    # Declared dead branch edges: (branch site, never-taken next address,
    # rationale), all in the unit's v2b frame. The coverage stage validates
    # each one (the site ran, exactly one edge was observed, and it is not the
    # declared-dead one) before taking it out of the denominator.
    unreachable_branches: List[Tuple[int, int, str]] = field(default_factory=list)
    # Preloaded state snapshot, if the unit is stateful.
    state: Optional[StateSpec] = None
    # A steps unit calls its entry several times per corpus case and compares
    # the declared state region after every call. The fixture module exports
    # `make_steps(i) -> list[list[ArgSpec]]` instead of `make_args(i)`, and the
    # oracle keeps the state region between the steps of one case
    # (`EmuOracle.persist_state`). One unit then proves a driver and every leaf
    # it reaches, on the step where each leaf is actually executed.
    steps: bool = False
    # Registers seeded after the normal argument placement. Values are
    # integers or "argN" (the resolved arena pointer of argument N). Used for
    # interior-start leaf blocks whose entry convention takes the pointer in a
    # non-argument register (e.g. RBX).
    start_regs: Dict[str, Any] = field(default_factory=dict)


@dataclass
class Divergence:
    iteration: int
    channel: str  # ret | mem | fault
    oracle: str
    candidate: str
    detail: str = ""


@dataclass
class MutantOutcome:
    """One mutant's result from the mutation worker pool.

    ``killed`` is the verdict; ``detail`` records why (the first diverging
    case and channel, including a timeout), and ``build_error`` is set when
    the mutant did not compile. A build failure caused by the mutated source
    is still killed, exactly as before, and counted in both numerator and
    denominator. ``infra`` marks a mutant the harness could not evaluate at
    all (a missing artifact, a transport failure, a timeout after a retry):
    that is not a kill and must make the unit inconclusive, never verified.
    """

    mutant: Any
    killed: bool
    detail: str = ""
    build_error: str = ""
    infra: bool = False


def _warn(msg: str) -> None:
    with _PRINT_LOCK:
        print(f"  WARNING: {msg}", file=sys.stderr)


# One print at a time: units can run in a thread pool (`cli.py all --unit-jobs`),
# and two half-written lines interleaved on a terminal cost more attention than
# the lock costs time (a unit logs a few dozen lines in total).
_PRINT_LOCK = threading.Lock()


def _print_line(msg: str) -> None:
    with _PRINT_LOCK:
        print(msg)


# `load_ledger(); record()` is a read-modify-write of ledger.json; units that
# finish concurrently must not drop each other's verdicts.
_LEDGER_LOCK = threading.Lock()


@dataclass
class _CandidateRef:
    """Everything the runner needs about the candidate artifact: the source
    file the mutation stage rewrites, the digest the verdict is bound to, and
    (for the shipped cargo path) the crate/probe the mutant builds use.

    Computed once per unit, both by `run_unit` (for the ledger entry) and by
    the fresh-verdict cache check, so the two can never disagree.
    """

    sha: str
    sources: List[str]
    crate_rev: str
    path: str
    crate_dir: str
    probe: str


def _candidate_ref(unit: Unit) -> _CandidateRef:
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    # A cargo unit names its crate and probe; the defaults preserve the old
    # ow-sim units. These paths are used again by the mutation builds, so
    # resolve them once here.
    cand_crate = unit.candidate_crate or "ow-sim"
    cand_probe_rel = unit.candidate_probe or "crates/ow-sim/src/bin/verify_probe.rs"
    cand_crate_dir = os.path.join(repo_root, "crates", cand_crate)
    cand_probe = (
        cand_probe_rel
        if os.path.isabs(cand_probe_rel)
        else os.path.join(repo_root, cand_probe_rel)
    )
    if unit.candidate_loader == "cargo":
        from candidate_rust import candidate_binding, entry_sources

        sha, sources, rev = candidate_binding(
            unit.entry, crate=cand_crate, probe=cand_probe
        )
        path = os.path.join(repo_root, entry_sources(unit.entry, cand_crate)[0])
        return _CandidateRef(
            sha=sha, sources=list(sources), crate_rev=rev, path=path,
            crate_dir=cand_crate_dir, probe=cand_probe,
        )
    if unit.candidate_loader == "rust":
        from candidate_rust import PROBE_SRC

        path = PROBE_SRC
        sha = sha256_file(path) if os.path.exists(path) else "missing"
    else:
        path = unit.c_file
        sha = sha256_file(path) if os.path.exists(path) else "missing"
    return _CandidateRef(
        sha=sha, sources=[], crate_rev="", path=path,
        crate_dir=cand_crate_dir, probe=cand_probe,
    )


def _fresh_cached_entry(unit: Unit) -> Optional[LedgerEntry]:
    """A verified ledger entry whose every bound digest still matches.

    The ledger's contract is that a verdict is a function of the oracle window
    (the image bytes at the function's address), the candidate artifact, the
    options, the surfaces, and the harness version that produced it. When all
    five are unchanged, re-running the ladder reproduces the recorded verdict
    by construction: the recorded run itself proved the oracle deterministic on
    identical inputs, and the digests cover every input the five stages
    consume. The entry is replayed instead of re-executed; `--force` re-proves.

    The cache is exactly as trustworthy as the ledger file (a produced,
    gitignored artifact): a hand-edited ledger can lie to it as easily as to a
    human reader. Only `verified` entries are replayed - a stale or failing
    verdict is always re-run.
    """
    try:
        oracle_sha = sha256_bytes(
            function_bytes(unit.image, unit.base, unit.address_v2b)
        )
    except (OSError, ValueError):
        # Missing image or a cold page: run_unit reports the failure properly.
        return None
    cref = _candidate_ref(unit)
    prev = load_ledger().get(unit.id)
    if prev is None or prev.status != "verified":
        return None
    # The declarations decide which mutants the mutation stage skips, so a
    # verdict produced under different declarations is not this unit's
    # verdict. (They also rot silently: a declaration that no longer names
    # a live site stops skipping anything, the equivalent mutant survives
    # and the kill rate drops - fn/Salsa20Xor, 2026-11-19.)
    if _decl_key(prev.equivalent_mutants) != _decl_key(unit.known_equivalent_mutants):
        return None
    if (
        check_entry(prev, oracle_sha, cref.sha, _options_sha(unit), _args_sha(unit))
        != "fresh"
    ):
        return None
    return replace(
        prev,
        notes=[
            "fresh cached verdict: every bound digest matches the ledger entry; "
            "the five stages were not re-run (--force re-proves)"
        ]
        + list(prev.notes),
    )


def load_unit(path: str) -> Unit:
    with open(path, "r", encoding="utf-8") as f:
        d = json.load(f)
    unknown = sorted(set(d) - _KNOWN_UNIT_KEYS)
    if unknown:
        _warn(f"unit {d.get('id', path)}: unknown keys {unknown} are ignored (typo?)")
    try:
        args = []
        for a in d["args"]:
            bad = sorted(set(a) - _KNOWN_ARG_KEYS)
            if bad:
                _warn(f"unit {d.get('id', path)}: arg {a.get('name')!r} has unknown keys {bad}")
            kind = a["kind"]
            if not (kind in _ARG_KINDS or kind.startswith("ptr")):
                raise ValueError(
                    f"unknown arg kind {kind!r} (expected f32/f64/u64 or ptr)"
                )
            args.append(UnitArg(**{k: a[k] for k in ("name", "kind") if k in a},
                                 ptr_len=a.get("ptr_len", 0)))
        ret_kind = d.get("returns", "f32")
        if ret_kind not in _RET_KINDS:
            raise ValueError(f"unknown returns {ret_kind!r} (expected one of {_RET_KINDS})")
        addr = d["address"]
        # A unit's address is in the frame its oracle runs in. Most units use
        # the v2b frame (the default base); a function that dereferences an
        # absolute pointer stored in the image (a spelling table, a vtable)
        # needs the image's own frame, because those values are baked in at
        # the link addresses. Such a unit declares `address.exe` plus
        # `base: 0x140000000`.
        if "v2b" in addr:
            address_v2b = int(addr["v2b"], 16)
        elif "exe" in addr:
            address_v2b = int(addr["exe"], 16)
            if int(d.get("base", "0x0"), 16) == 0:
                raise ValueError("address.exe requires an explicit base")
        else:
            raise ValueError("address needs a v2b or exe key")
    except (KeyError, ValueError, TypeError) as e:
        raise ValueError(f"unit {path}: malformed descriptor: {e}") from e
    # Unit paths are repo-relative; resolve against the repo root (two levels
    # up from dev/verify_harness/).
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    def abspath(p: str) -> str:
        return p if os.path.isabs(p) else os.path.join(repo_root, p)
    base_raw = d.get("base", DEFAULT_BASE)
    base = int(base_raw, 16) if isinstance(base_raw, str) else int(base_raw)
    # c_file is tier-1 only; rust units carry candidateLoader and omit it.
    c_file = d.get("c_file", "")
    opts = d.get("options", {})
    unknown_opts = sorted(set(opts) - _KNOWN_OPTION_KEYS)
    if unknown_opts:
        _warn(f"unit {d.get('id', path)}: unknown options {unknown_opts} are ignored (typo?)")
    allowed_reads_raw = opts.get("allowedImageReads", []) or []
    allowed_image_reads: List[int] = []
    allowed_read_ranges: List[Tuple[int, int]] = []
    try:
        for a in allowed_reads_raw:
            if isinstance(a, str) and "-" in a:
                lo, hi = a.split("-", 1)
                allowed_read_ranges.append((int(lo, 16), int(hi, 16)))
            elif isinstance(a, (list, tuple)) and len(a) == 2:
                allowed_read_ranges.append((int(a[0], 16) if isinstance(a[0], str) else int(a[0]),
                                            int(a[1], 16) if isinstance(a[1], str) else int(a[1])))
            else:
                allowed_image_reads.append(int(a, 16) if isinstance(a, str) else int(a))
    except (TypeError, ValueError) as e:
        raise ValueError(f"unit {path}: bad allowedImageReads: {e}") from e
    allowed_write_ranges: List[Tuple[int, int]] = []
    for a in opts.get("allowedImageWrites", []) or []:
        try:
            if isinstance(a, str) and "-" in a:
                lo, hi = a.split("-", 1)
                allowed_write_ranges.append((int(lo, 16), int(hi, 16)))
            elif isinstance(a, (list, tuple)) and len(a) == 2:
                allowed_write_ranges.append(
                    (
                        int(a[0], 16) if isinstance(a[0], str) else int(a[0]),
                        int(a[1], 16) if isinstance(a[1], str) else int(a[1]),
                    )
                )
            else:
                raise ValueError(f"not a range: {a!r}")
        except (TypeError, ValueError) as e:
            raise ValueError(f"unit {path}: bad allowedImageWrites: {e}") from e
    unreachable: List[Tuple[int, int, str]] = []
    for u in opts.get("unreachableBranches", []) or []:
        try:
            site = int(u["site"], 16) if isinstance(u["site"], str) else int(u["site"])
            edge = int(u["edge"], 16) if isinstance(u["edge"], str) else int(u["edge"])
            why = str(u["why"])
        except (KeyError, TypeError, ValueError) as e:
            raise ValueError(f"unit {path}: bad unreachableBranches entry: {e}") from e
        unreachable.append((site, edge, why))
    state = None
    st = d.get("state")
    if st is not None:
        try:
            sbase = int(st["base"], 16) if isinstance(st["base"], str) else int(st["base"])
            if "hex" in st:
                data = bytes.fromhex(st["hex"])
                source = "inline"
            else:
                spath = abspath(st["image"])
                with open(spath, "rb") as f:
                    data = f.read()
                source = spath
            if "size" in st:
                data = data[: int(st["size"])]
            if not data:
                raise ValueError("empty snapshot")
            state = StateSpec(base=sbase, data=data, source=source)
        except (KeyError, OSError, ValueError, TypeError) as e:
            raise ValueError(f"unit {path}: bad state block: {e}") from e
    return Unit(
        id=d["id"],
        title=d.get("title", ""),
        address_v2b=address_v2b,
        c_file=abspath(c_file) if c_file else "",
        entry=d["entry"],
        args=args,
        ret_kind=ret_kind,
        options=opts,
        image=resolve_image(abspath(d.get("image", DEFAULT_IMAGE))),
        base=base,
        known_equivalent_mutants=d.get("knownEquivalentMutants", []),
        acknowledged_gaps=d.get("acknowledgedGaps", []),
        notes=d.get("notes", []),
        arg_factory=d.get("argFactory"),
        candidate_loader=d.get("candidateLoader", "c"),
        candidate_crate=d.get("candidateCrate", ""),
        candidate_probe=d.get("candidateProbe", ""),
        start_regs=d.get("startRegs", {}),
        allowed_image_reads=allowed_image_reads,
        allowed_read_ranges=allowed_read_ranges,
        allowed_write_ranges=allowed_write_ranges,
        unreachable_branches=unreachable,
        state=state,
        steps=bool(d.get("steps", False)),
    )


def _args_sha(unit: Unit) -> str:
    """Digest of everything that determines the INPUT SIDE: the declared
    surface, the fixture module that builds the structured inputs, and (for
    tier-1) the shim header the candidate compiles against. Editing any of
    them invalidates the verdict - the fixture IS the corpus."""
    payload = json.dumps(
        [{"name": a.name, "kind": a.kind, "ptr_len": a.ptr_len} for a in unit.args],
        sort_keys=True,
    ).encode()
    parts = [sha256_bytes(payload)]
    if unit.arg_factory:
        fac_path = os.path.join(os.path.dirname(__file__), unit.arg_factory + ".py")
        if os.path.exists(fac_path):
            parts.append(sha256_file(fac_path))
        else:
            _warn(f"unit {unit.id}: argFactory module file not found for digest: {fac_path}")
    if unit.state is not None:
        payload = f"{unit.state.base:#x}:{len(unit.state.data)}".encode()
        parts.append(sha256_bytes(payload))
        if unit.state.source and unit.state.source != "inline":
            if os.path.exists(unit.state.source):
                parts.append(sha256_file(unit.state.source))
            else:
                _warn(f"unit {unit.id}: state snapshot file not found for digest")
        else:
            parts.append(sha256_bytes(unit.state.data))
    if unit.candidate_loader not in ("rust", "cargo") and os.path.exists(SHIM_PATH):
        parts.append(sha256_file(SHIM_PATH))
    return sha256_bytes(b"".join(p.encode() for p in parts))


def _options_sha(unit: Unit) -> str:
    return sha256_bytes(json.dumps(unit.options, sort_keys=True).encode())


def _bits_equal(a: int, b: int, ret_kind: str) -> bool:
    """The equivalence relation. NaN equal to NaN (f32 and f64: NaN payloads
    differ across codegen paths and carry no contract), +0 distinct from -0,
    everything else bit-exact. Printed normalisations: none silent."""
    if a == b:
        return True
    if ret_kind == "f32":
        fa, fb = bits_f32(a), bits_f32(b)
        return math.isnan(fa) and math.isnan(fb)
    if ret_kind == "f64":
        mask = (1 << 64) - 1
        fa = struct.unpack("<d", struct.pack("<Q", a & mask))[0]
        fb = struct.unpack("<d", struct.pack("<Q", b & mask))[0]
        return math.isnan(fa) and math.isnan(fb)
    if ret_kind == "vec4":
        # Four f32 lanes; each lane is compared exactly, including NaN
        # payloads. A position is state, not an arithmetic result: a payload
        # change is a divergence.
        return a == b
    if ret_kind == "u8":
        # The client contracts only AL (status byte) and leaves RAX's upper
        # bits holding unrelated arithmetic. Masking is the contract, not a
        # tolerance: the two values are compared bit-exactly inside the byte.
        return (a & 0xFF) == (b & 0xFF)
    return False


def _args_key(args: Sequence[ArgSpec]) -> str:
    """Stable identity of one input. The oracle is verified deterministic
    (stage 2) and stateless (arena/stack/registers reset per call), so a
    cached CallResult for this key IS the result of another call with it."""
    return json.dumps(
        [
            [a.kind, a.mem.hex(), repr(a.value), a.bits, a.arena_offset,
             getattr(a, "guest", None), list(a.relocs)]
            for a in args
        ]
    )


def _oracle_call_cached(
    oracle: EmuOracle,
    v2b_addr: int,
    args: Sequence[ArgSpec],
    ret_kind: str,
    cache: Dict[str, Any],
    start_regs: Optional[Dict[str, Any]] = None,
) -> Any:
    key = _args_key(args) + "|" + repr(sorted((start_regs or {}).items()))
    r = cache.get(key)
    if r is None:
        r = oracle.call(
            v2b_addr, args, ret_kind=ret_kind, start_regs=start_regs or {}
        )
        cache[key] = r
    return r


def _mem_channel(
    o_pointee_after: bytes,
    cand_pointee_after: Dict[int, bytes],
    args: Sequence[ArgSpec],
) -> Optional[Tuple[str, str, str]]:
    """Return (oracle_hex, candidate_hex, detail) when the memory channel
    disagrees, else None. Reloc slots hold the pointee's own base address
    (arena vs host), which necessarily differs between the two sides; mask
    them so only the data the callee wrote is compared."""
    cand_bytes = b"".join(cand_pointee_after[k] for k in sorted(cand_pointee_after))
    o_after = bytearray(o_pointee_after)
    c_after = bytearray(cand_bytes)
    pos = 0
    for a in args:
        if a.kind != "ptr":
            continue
        for off in a.relocs:
            s = pos + off
            o_after[s : s + 8] = b"\x00" * 8
            c_after[s : s + 8] = b"\x00" * 8
        pos += len(a.mem)
    if bytes(o_after) == bytes(c_after):
        return None
    diff_at = next(
        (i for i, (x, y) in enumerate(zip(o_after, c_after)) if x != y),
        min(len(o_after), len(c_after)),
    )
    return (
        bytes(o_after).hex(),
        bytes(c_after).hex(),
        f"pointee bytes differ after the call (first at byte {diff_at})",
    )


def _state_channel(
    o_after: Optional[bytes],
    c_after: Optional[bytes],
    unit: Unit,
    args: Sequence[ArgSpec],
) -> Optional[Tuple[str, str, str]]:
    """Compare the whole preloaded state region after the call.

    Declared reloc slots inside the state hold a pointer to the state itself;
    the oracle and the probe use different host addresses there, so those
    8-byte slots are masked exactly as the pointee channel masks them. Any
    other difference is a state divergence.
    """
    if unit.state is None:
        return None
    if o_after is None or c_after is None:
        return ("<missing>", "<missing>", "state channel missing on one side")
    o = bytearray(o_after)
    c = bytearray(c_after)
    if len(o) != len(c):
        return (o_after.hex(), c_after.hex(), "state region length differs")
    for a in args:
        if a.kind != "ptr" or getattr(a, "guest", None) is None:
            continue
        base_off = a.guest - unit.state.base
        for off in a.relocs:
            s = base_off + off
            if 0 <= s and s + 8 <= len(o):
                o[s : s + 8] = b"\x00" * 8
                c[s : s + 8] = b"\x00" * 8
    if bytes(o) == bytes(c):
        return None
    diff_at = next(
        (i for i, (x, y) in enumerate(zip(o, c)) if x != y),
        min(len(o), len(c)),
    )
    return (
        bytes(o).hex(),
        bytes(c).hex(),
        f"state bytes differ after the call (first at state+0x{diff_at:x})",
    )


def _read_allowed(unit: Unit, addr: int, size: int = 1) -> bool:
    if addr in unit.allowed_image_reads:
        return True
    if (
        unit.state is not None
        and unit.state.base <= addr
        and addr + size <= unit.state.base + len(unit.state.data)
    ):
        # The state region is declared by the unit's `state` block; every read
        # in it is part of the controlled struct, not an undeclared global.
        return True
    return any(lo <= addr and addr + size - 1 <= hi for lo, hi in unit.allowed_read_ranges)


# How many times the batch build may be rebuilt after the compiler rejected
# an arm. Each round drops the arms the compiler named; a unit whose mutants
# all compile needs one round.
BATCH_BUILD_ROUNDS = 6


def _batch_enabled() -> bool:
    """Is the batch build on?

    It is on by default: one crate build for a unit's whole mutant set
    instead of one per mutant. `VERIFY_NO_BATCH=1` turns it off, which is
    the escape hatch for a crate whose build refuses the generated dispatch
    and the way to A/B the two paths against each other.
    """
    return os.environ.get("VERIFY_NO_BATCH", "") == ""


def _build_batch(
    unit: Unit,
    cref: Any,
    cand_path: str,
    base_src: str,
    active: List[Any],
):
    """Compile a unit's whole mutant set in ONE crate build.

    Every mutant becomes a dispatch arm inside the function under test
    (`batch.py`); the harness picks the arm at run time with an environment
    variable. A mutant whose code does not compile takes the whole batch down
    with it - the compiler names the line, the line names the arm, and that
    arm is dropped and the batch rebuilt. When the error cannot be
    attributed to an arm, or the batch still does not build, the caller
    falls back to one build per mutant.

    Returns `(exe, work_dir, {position: arm index}, {position: build error})`.
    `exe` is None when the batch is not usable.
    """
    from batch import error_lines, plan_batch
    from candidate_rust import RustCandidate, mutation_entry

    base_dir = os.path.dirname(cand_path) or "."
    # The mutation entry is the function the mutator edits, which is not
    # always the probe's entry name (the probe's entry is a wrapper).
    entry = mutation_entry(unit.entry) if unit.candidate_loader == "cargo" else unit.entry
    remaining = list(active)
    arm_of: Dict[int, int] = {}
    broken: Dict[int, str] = {}
    pos_of = {id(m): pos for pos, m in enumerate(active)}
    work = ""
    for _round in range(BATCH_BUILD_ROUNDS):
        plan = plan_batch(base_src, entry, remaining, base_dir)
        if plan is None:
            return None, "", {}, {}
        work = tempfile.mkdtemp(prefix="batch_")
        try:
            builder = RustCandidate(
                entry=unit.entry,
                arg_kinds=[
                    a.kind if a.kind != "ptr" else "ptr" for a in unit.args
                ],
                ret_kind=unit.ret_kind,
                work_dir=work,
                source_override=plan.source,
                source_path=cand_path,
                loader="cargo",
                state_base=unit.state.base if unit.state is not None else None,
                state_bytes=unit.state.data if unit.state is not None else None,
                crate_dir=cref.crate_dir,
                cargo_probe=cref.probe,
                start_process=False,
            )
            exe = builder.exe
        except RuntimeError as e:
            msg = str(e)
            lines = error_lines(msg)
            dropped = set()
            for arm in plan.arms:
                if any(arm.line_start <= ln <= arm.line_end for ln in lines):
                    dropped.add(id(arm.mutant))
            if not dropped:
                shutil.rmtree(work, ignore_errors=True)
                return None, "", {}, {}
            for pos, m in enumerate(active):
                if id(m) in dropped:
                    broken[pos] = msg
            remaining = [m for m in remaining if id(m) not in dropped]
            shutil.rmtree(work, ignore_errors=True)
            work = ""
            continue
        for arm in plan.arms:
            pos = pos_of.get(id(arm.mutant))
            if pos is not None:
                arm_of[pos] = arm.index
        return exe, work, arm_of, broken
    shutil.rmtree(work, ignore_errors=True)
    return None, "", {}, {}


def _batch_matches_baseline(
    unit: Unit,
    cref: Any,
    batch_exe: str,
    mutation_oracle: List[Tuple[Any, Any]],
) -> bool:
    """Does the batch binary, with no arm selected, behave like the baseline?

    The batch's default arm is the original body, so a batch binary with no
    `OW_MUTANT_INDEX` set must answer exactly what the unmutated candidate
    answers. If it does not, the batch generation is not faithful and the
    unit falls back to one build per mutant - the batch is an optimisation,
    never a change of what is measured.
    """
    if not mutation_oracle:
        return True
    from candidate_rust import RustCandidate

    work = tempfile.mkdtemp(prefix="batch_check_")
    probe = None
    try:
        probe = RustCandidate(
            entry=unit.entry,
            arg_kinds=[
                a.kind if a.kind != "ptr" else "ptr" for a in unit.args
            ],
            ret_kind=unit.ret_kind,
            work_dir=work,
            prebuilt_exe=batch_exe,
            loader="cargo",
            state_base=unit.state.base if unit.state is not None else None,
            state_bytes=unit.state.data if unit.state is not None else None,
            crate_dir=cref.crate_dir,
            cargo_probe=cref.probe,
        )
        # Does the batch binary reproduce the baseline? A steps unit is
        # replayed step by step on one process, the same way the base case ran.
        for args, o in mutation_oracle:
            c = probe.call(args)
            if not c.ok:
                return False
            if unit.ret_kind != "void" and not _bits_equal(
                o.ret_bits, c.ret_bits, unit.ret_kind
            ):
                return False
            if _mem_channel(o.pointee_after, c.pointee_after, args) is not None:
                return False
            if (
                _state_channel(
                    getattr(o, "state_after", None),
                    getattr(c, "state_after", None),
                    unit,
                    args,
                )
                is not None
            ):
                return False
        return True
    except Exception:
        return False
    finally:
        if probe is not None:
            try:
                probe.close()
            except Exception:
                pass
        shutil.rmtree(work, ignore_errors=True)


def run_unit(
    unit: Unit,
    work_dir: Optional[str] = None,
    skip_mutation: bool = False,
    verbose: bool = True,
    mutant_workers: int = DEFAULT_MUTANT_JOBS,
    mutation_call_timeout: Optional[float] = None,
    log_prefix: str = "",
) -> LedgerEntry:
    def log(msg: str) -> None:
        if verbose:
            _print_line(log_prefix + msg)

    opts = unit.options
    iterations = int(opts.get("iterations", 300))
    cov_gate = float(opts.get("coverageThreshold", 85.0))
    mutation_count = int(opts.get("mutationCount", 12))
    kill_gate = float(opts.get("mutationKillRate", 0.8))
    seed = int(opts.get("seed", 4241))
    fresh_oracle = bool(opts.get("freshOraclePerCase", False))
    cov_window = opts.get("coverageWindow")
    if cov_window is not None:
        cov_window = int(cov_window)
        if not (0 < cov_window <= 0x2000):
            raise ValueError(
                f"unit {unit.id}: coverageWindow must be 1..0x2000 bytes, "
                f"got {cov_window:#x}"
            )
    if mutation_call_timeout is None:
        mutation_call_timeout = float(
            opts.get("mutationCallTimeout", DEFAULT_MUTATION_CALL_TIMEOUT)
        )
    if not (mutation_call_timeout > 0):
        raise ValueError(
            f"unit {unit.id}: mutationCallTimeout must be positive, "
            f"got {mutation_call_timeout!r}"
        )
    # Coverage-guided corpus growth. `autoCorpus` is true/false or a dict
    # {"cap": int, "step": int}; either form lands in the options digest,
    # so editing it stales the verdict like any other option. It only applies
    # to units without an argFactory - a factory's corpus is the fixture's
    # claim, and growing past it would silently rewrite that claim.
    auto_raw = opts.get("autoCorpus", False)
    if isinstance(auto_raw, bool):
        auto_corpus = auto_raw
        corpus_cap = DEFAULT_CORPUS_CAP
        corpus_step = DEFAULT_CORPUS_STEP
    elif isinstance(auto_raw, dict):
        bad = sorted(set(auto_raw) - {"cap", "step"})
        if bad:
            raise ValueError(
                f"unit {unit.id}: unknown autoCorpus keys {bad} "
                "(expected cap/step)"
            )
        auto_corpus = True
        corpus_cap = int(auto_raw.get("cap", DEFAULT_CORPUS_CAP))
        corpus_step = int(auto_raw.get("step", DEFAULT_CORPUS_STEP))
    else:
        raise ValueError(
            f"unit {unit.id}: autoCorpus must be true/false or {{cap, step}}, "
            f"got {auto_raw!r}"
        )
    if auto_corpus and (corpus_cap < 1 or corpus_step < 1):
        raise ValueError(
            f"unit {unit.id}: autoCorpus cap and step must be positive "
            f"(got cap {corpus_cap}, step {corpus_step})"
        )
    if auto_corpus and unit.arg_factory is not None:
        _warn(f"unit {unit.id}: autoCorpus ignored - the argFactory defines the corpus")
        auto_corpus = False

    oracle_sha = "unloaded"
    # The realized corpus, recorded in the ledger once stage 3 has finished
    # comparing (base + any growth). Empty until then: an early exit never
    # reached a final corpus. This is an audit fingerprint, not a freshness
    # input - the corpus is a deterministic function of what the ledger
    # already binds (the args surface, the options incl. seed/iterations/
    # cap/step, the oracle window, the harness version), which is why a
    # fresh replay is still the verdict the same digests produced.
    corpus_sha = ""
    corpus_n = 0
    # The candidate artifact the ledger binds to: the decompile for tier 1,
    # the probe source for tier 2 rustc, the crate function source + crate
    # revision for the shipped cargo probe. One place computes this (also
    # used by the fresh-verdict cache), so the entry and the cache can never
    # disagree about what a verdict is bound to.
    cref = _candidate_ref(unit)
    cand_path = cref.path
    cand_sha = cref.sha
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
            harness_version=HARNESS_VERSION,
            equivalent_mutants=unit.known_equivalent_mutants,
            acknowledged_gaps=unit.acknowledged_gaps,
            notes=list(unit.notes) + list(kw_notes),
            candidate_sources=list(cref.sources),
            crate_rev=cref.crate_rev,
            corpus_sha256=corpus_sha,
            corpus_cases=corpus_n,
            **kw,
        )

    # Each stage reports its own wall time. The harness's cost is not
    # uniform: a cargo unit's mutation stage is dominated by crate builds,
    # a tier-1 unit's by the C compiler, and both by nothing at all when
    # the ledger replays the verdict. Without the numbers, "make it quick"
    # is a guess.
    _stage_t0 = time.perf_counter()

    def _stage_done(name: str) -> None:
        nonlocal _stage_t0
        now = time.perf_counter()
        log(f"  [{name}] {now - _stage_t0:.1f}s")
        _stage_t0 = now

    # ---- Stage 1: load ----------------------------------------------------
    log(f"[1/5] load {unit.id}")
    if not os.path.exists(unit.image):
        log(f"  FAIL: image not found: {unit.image}")
        return entry("inconclusive", notes=["image missing: " + unit.image])
    if unit.candidate_loader == "cargo":
        if not os.path.exists(cref.probe):
            log(f"  FAIL: cargo probe source not found: {cref.probe}")
            return entry("inconclusive", notes=[f"cargo probe missing: {cref.probe}"])
        if not os.path.isdir(cref.crate_dir):
            log(f"  FAIL: cargo crate not found: {cref.crate_dir}")
            return entry("inconclusive", notes=[f"cargo crate missing: {cref.crate_dir}"])
        if not os.path.exists(cand_path):
            log(f"  FAIL: candidate source not found: {cand_path}")
            return entry("inconclusive", notes=[f"candidate source missing: {cand_path}"])
    elif unit.candidate_loader == "rust":
        if not os.path.exists(cand_path):
            log(f"  FAIL: rust probe source not found: {cand_path}")
            return entry("inconclusive", notes=[f"rust probe missing: {cand_path}"])
    elif not os.path.exists(unit.c_file):
        log(f"  FAIL: decompile not found: {unit.c_file}")
        return entry("inconclusive", notes=["c_file missing: " + unit.c_file])

    oracle = EmuOracle(unit.image, base=unit.base)
    oracle.allowed_writes = list(unit.allowed_write_ranges)
    if unit.state is not None:
        try:
            oracle.set_state(unit.state.base, unit.state.data)
        except ValueError as e:
            log(f"  FAIL: state preload: {e}")
            return entry("inconclusive", notes=[f"state preload failed: {e}"])
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
            log("  NOTE: previous verdict is stale (inputs or harness changed); re-verifying")

    own_workdir = work_dir is None
    work_dir = work_dir or tempfile.mkdtemp(prefix=f"verify_{unit.id.replace('/', '_')}_")
    candidate = None
    # Install the coverage tracker BEFORE the first oracle.call. Unicorn's
    # CODE hooks become unreliable if added after call() has cycled its
    # MEM hooks (the handle table is rebuilt); measuring coverage of the
    # compared runs requires the tracker to be in place from the start.
    from coverage import BranchTracker

    if cov_window is not None:
        log(f"  coverage window: {cov_window:#x} bytes (unit-declared function extent)")
    try:
        tracker = BranchTracker(oracle, unit.address_v2b, window=cov_window)
    except Exception as e:
        log(f"  INCONCLUSIVE: coverage tracking failed: {e}")
        return entry("inconclusive", notes=[f"coverage tracking failed: {e}"])
    try:
        if unit.candidate_loader in ("rust", "cargo"):
            from candidate_rust import RustCandidate

            candidate = RustCandidate(
                entry=unit.entry,
                arg_kinds=[a.kind if a.kind != "ptr" else "ptr" for a in unit.args],
                ret_kind=unit.ret_kind,
                work_dir=work_dir,
                loader=unit.candidate_loader,
                state_base=unit.state.base if unit.state is not None else None,
                state_bytes=unit.state.data if unit.state is not None else None,
                crate_dir=cref.crate_dir,
                cargo_probe=cref.probe,
            )
            log(f"  candidate {unit.candidate_loader} probe: {candidate.exe}")
            # A steps unit keeps the state region between its calls; every other
            # unit answers from the snapshot on every call. The probe has to be
            # told, because the oracle does exactly that on its own side.
            candidate.set_persist(bool(unit.steps))
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
        factory = mod.make_steps if unit.steps else mod.make_args

    try:
        # ---- Stage 2: determinism ----------------------------------------
        _stage_done("load")
        log("[2/5] determinism")
        if factory is not None:
            det_args = factory(0)
            if unit.steps:
                det_args = det_args[0]
        else:
            det_args = generate_args(unit.args, seed, 0)
        cache: Dict[str, Any] = {}
        tracker.begin_call()
        r1 = _oracle_call_cached(
            oracle, unit.address_v2b, det_args, unit.ret_kind, cache, unit.start_regs
        )
        # The second run must be a REAL second run - a cache hit would make
        # the determinism claim true by construction.
        tracker.begin_call()
        r2 = oracle.call(
            unit.address_v2b,
            det_args,
            ret_kind=unit.ret_kind,
            start_regs=unit.start_regs,
        )
        if not r1.ok or not r2.ok:
            log(f"  FAIL: oracle fault on determinism probe: {r1.fault or r2.fault}")
            return entry("invalid", notes=[f"oracle fault: {r1.fault or r2.fault}"])
        if r1.ret_bits != r2.ret_bits or r1.pointee_after != r2.pointee_after:
            log("  FAIL: oracle disagrees with itself on identical input")
            return entry("invalid", notes=["oracle nondeterministic on identical input"])
        if r1.state_after != r2.state_after:
            log("  FAIL: oracle state channel disagrees with itself on identical input")
            return entry("invalid", notes=["oracle state nondeterministic on identical input"])
        log("  ok")

        # ---- Stage 3: differential ---------------------------------------
        if auto_corpus:
            _stage_done("determinism")
            log(
                f"[3/5] differential ({iterations} draws + edge pool, "
                f"autoCorpus cap {corpus_cap})"
            )
        else:
            log(f"[3/5] differential ({iterations} iterations + edge pool)")
        divergences: List[Divergence] = []
        undeclared_reads: List[Tuple[int, int]] = []
        undeclared_writes: List[Tuple[int, int]] = []
        corpus_args: List[List[ArgSpec]] = []
        hard_fail: Optional[str] = None

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

        # Coverage is measured on THESE runs (stage 4 analyses the tracker;
        # no second pass over the corpus). The tracker was installed before
        # stage 2 so Unicorn's CODE hook is live for every oracle.call.

        # Compiler literal-pool constants the unit has declared (and the
        # decompile already inlined). A read of one of these is not
        # undeclared global state: the candidate hardcodes the same value.

        # One emulator serves every steps case unless the unit asks for a
        # fresh one: building it is the expensive part (measured 12x the cost
        # of the calls it then runs). Only the state region has to go back to
        # its snapshot per case, which set_state does.
        steps_oracle: List[EmuOracle] = []

        def _run_steps(idx: int, steps: Sequence[List[ArgSpec]]) -> bool:
            """One corpus case of a steps unit: the entry called N times.

            The oracle and the candidate are called alternately on the SAME
            state, so the state channel compares what each call left behind
            and the next call inherits it. Every channel is compared after
            every step, with the step number in the channel name (`ret@2`),
            because a driver's third call is a different fact from its first.
            """
            nonlocal hard_fail
            created = fresh_oracle or not steps_oracle
            if created:
                case_oracle = EmuOracle(unit.image, base=unit.base)
                if steps_oracle:
                    steps_oracle[0] = case_oracle
                else:
                    steps_oracle.append(case_oracle)
                tracker.attach(case_oracle)
            case_oracle = steps_oracle[0]
            if unit.state is not None:
                case_oracle.set_state(unit.state.base, unit.state.data)
            case_oracle.persist_state = True
            # The oracle starts every case from the snapshot; the candidate
            # process is shared, so it has to be told the same thing - and it
            # has to keep the region between the steps, which is what
            # set_persist turns on for this unit.
            if unit.state is not None and hasattr(candidate, "reset_state"):
                candidate.reset_state()
            for step, args in enumerate(steps):
                tracker.begin_call()
                o = case_oracle.call(
                    unit.address_v2b,
                    args,
                    ret_kind=unit.ret_kind,
                    start_regs=unit.start_regs or {},
                )
                if not o.ok:
                    hard_fail = f"oracle fault at {idx} step {step}: {o.fault}"
                    return False
                if o.reads_outside:
                    undeclared_reads.extend(
                        r for r in o.reads_outside if not _read_allowed(unit, r[0], r[1])
                    )
                if o.out_of_arena_writes:
                    undeclared_writes.extend(o.out_of_arena_writes)

                c = candidate.call(args)
                if not c.ok:
                    divergences.append(
                        Divergence(
                            idx,
                            f"fault@{step}",
                            "ok",
                            f"candidate fault: {c.fault}",
                        )
                    )
                else:
                    if unit.ret_kind != "void" and not _bits_equal(
                        o.ret_bits, c.ret_bits, unit.ret_kind
                    ):
                        divergences.append(
                            Divergence(
                                idx,
                                f"ret@{step}",
                                f"{o.ret_bits:#x} ({_fmt_ret(o.ret_bits, unit.ret_kind)})",
                                f"{c.ret_bits:#x} ({_fmt_ret(c.ret_bits, unit.ret_kind)})",
                            )
                        )
                    mem = _mem_channel(o.pointee_after, c.pointee_after, args)
                    if mem is not None:
                        divergences.append(Divergence(idx, f"mem@{step}", mem[0], mem[1], mem[2]))
                    st = _state_channel(
                        getattr(o, "state_after", None),
                        getattr(c, "state_after", None),
                        unit,
                        args,
                    )
                    if st is not None:
                        divergences.append(Divergence(idx, f"state@{step}", st[0], st[1], st[2]))
                corpus_args.append(args)
                if len(divergences) >= 3:
                    return False
            return True

        def _run_case(idx: int, args: List[ArgSpec]) -> bool:
            """Compare one case on every channel. Returns False when the
            caller must stop (the divergence cap is hit, or the oracle faulted
            via `hard_fail`). The base corpus and every growth batch run
            through this same body, so a grown case is compared exactly like
            a base case: the same channels, the same undeclared-access
            accounting, the same counterexample format."""
            nonlocal hard_fail
            if unit.steps:
                return _run_steps(idx, args)
            tracker.begin_call()
            if fresh_oracle:
                call_oracle = EmuOracle(unit.image, base=unit.base)
                if unit.state is not None:
                    call_oracle.set_state(unit.state.base, unit.state.data)
                tracker.attach(call_oracle)
                o = call_oracle.call(
                    unit.address_v2b,
                    args,
                    ret_kind=unit.ret_kind,
                    start_regs=unit.start_regs or {},
                )
            else:
                o = _oracle_call_cached(
                    oracle, unit.address_v2b, args, unit.ret_kind, cache, unit.start_regs
                )
            if not o.ok:
                hard_fail = f"oracle fault at {idx}: {o.fault}"
                return False
            if o.reads_outside:
                undeclared_reads.extend(
                    r for r in o.reads_outside if not _read_allowed(unit, r[0], r[1])
                )
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
                mem = _mem_channel(o.pointee_after, c.pointee_after, args)
                if mem is not None:
                    divergences.append(Divergence(idx, "mem", mem[0], mem[1], mem[2]))
                st = _state_channel(
                    getattr(o, "state_after", None),
                    getattr(c, "state_after", None),
                    unit,
                    args,
                )
                if st is not None:
                    divergences.append(Divergence(idx, "state", st[0], st[1], st[2]))
            corpus_args.append(args)
            return len(divergences) < 3

        def _post_cases_check() -> Optional[LedgerEntry]:
            """The checks after a compared batch: an oracle fault, undeclared
            accesses, divergences. Returns the entry to return, or None when
            the ladder continues."""
            if hard_fail:
                log(f"  FAIL: {hard_fail}")
                return entry("inconclusive", notes=[hard_fail])
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
                    if d.detail:
                        log(f"      {d.detail}")
                return entry(
                    "divergent",
                    divergences=[d.__dict__ for d in divergences],
                )
            return None

        try:
            for idx, args in enumerate(cases):
                if not _run_case(idx, args):
                    break
            early = _post_cases_check()
            if early is not None:
                return early

            # ---- Stage 3b: coverage-guided growth (autoCorpus) -----------
            # Growth only adds compared cases, and only after the base corpus
            # agreed on every channel - it cannot mask a divergence. The
            # draws continue the same deterministic (seed, i) sequence the
            # base corpus used, so the realized corpus stays a function of
            # the digests the ledger already binds. A batch that diverges or
            # faults ends growth; the checks above then report it exactly as
            # they would have for the base corpus.
            grown = 0
            base_case_count = len(cases)
            if auto_corpus and factory is None:
                next_i = iterations
                while True:
                    probe = tracker.result(unreachable=unit.unreachable_branches)
                    if probe.invalid_declarations:
                        break  # stage 4 reports the invalid declaration
                    if probe.coverage >= cov_gate:
                        break
                    if len(cases) >= corpus_cap:
                        log(
                            f"  growth: cap {corpus_cap} reached at branch "
                            f"coverage {probe.coverage:.1f}%; stage 4 decides"
                        )
                        break
                    log(
                        f"  growth: branch coverage {probe.coverage:.1f}% < gate "
                        f"{cov_gate:.1f}% at {len(cases)} cases; drawing "
                        f"{corpus_step} more"
                    )
                    batch: List[List[ArgSpec]] = []
                    target = min(corpus_cap, len(cases) + corpus_step)
                    while len(cases) + len(batch) < target:
                        batch.append(generate_args(unit.args, seed, next_i))
                        next_i += 1
                    stop = False
                    for args in batch:
                        cases.append(args)
                        if not _run_case(len(cases) - 1, args):
                            stop = True
                            break
                    grown += len(batch)
                    if stop:
                        break
                early = _post_cases_check()
                if early is not None:
                    return early
                if grown:
                    final = tracker.result(unreachable=unit.unreachable_branches)
                    log(
                        f"  growth: +{grown} case(s) -> branch coverage "
                        f"{final.coverage:.1f}% ({len(cases)} cases)"
                    )

            # The corpus compared is now final; bind it into every entry
            # this run can still record.
            corpus_sha = sha256_bytes(
                "\n".join(_args_key(a) for a in corpus_args).encode("utf-8")
            )
            corpus_n = len(corpus_args)
            log(f"  ok ({len(cases)} cases, every channel)")

            # ---- Stage 4: coverage -------------------------------------------
            _stage_done("differential")
            log("[4/5] coverage (oracle, both branch edges of the compared runs)")
            cov = tracker.result(unreachable=unit.unreachable_branches)
        finally:
            tracker.close()

        if cov.invalid_declarations:
            msg = "; ".join(cov.invalid_declarations[:4])
            log(f"  INCONCLUSIVE: unreachable-branch declaration does not hold: {msg}")
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[f"invalid unreachableBranches declaration: {msg}"],
            )
        log(
            f"  branch coverage {cov.coverage:.1f}% ({cov.hit}/{cov.total} reachable "
            f"branches of {cov.discovered} sites)"
        )
        if cov.declared_unreachable:
            log(
                f"  {cov.declared_unreachable} branch site(s) declared with one dead "
                "edge; each site ran and the dead edge was never observed"
            )
        if cov.coverage < cov_gate:
            gaps = ", ".join(f"{a:#x}" for a in cov.missed[:8])
            log(
                f"  INCONCLUSIVE: coverage {cov.coverage:.1f}% < gate {cov_gate}%; "
                f"missed {gaps}"
            )
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[f"coverage {cov.coverage:.1f}% < gate {cov_gate}%; missed {gaps}"],
            )

        # ---- Stage 5: mutation -------------------------------------------
        if skip_mutation:
            log("[5/5] mutation SKIPPED (not a verified path)")
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=["mutation skipped; a unit is not verified without it"],
            )

        _stage_done("coverage")
        log(f"[5/5] mutation ({mutation_count} mutants, kill gate {kill_gate:.0%})")
        with open(cand_path, "r", encoding="utf-8") as f:
            base_src = f.read()

        from candidate_rust import mutation_entry

        mutants = mutate_mod.generate_mutants(
            base_src,
            mutation_entry(unit.entry) if unit.candidate_loader == "cargo" else unit.entry,
            count=mutation_count,
            base_dir=os.path.dirname(cand_path) or ".",
        )
        # A declaration that names no live mutation site is stale: the
        # candidate's code moved under it. The equivalent mutant is then
        # probed again, survives (it is equivalent) and drags the kill rate
        # down - enough to drop the unit below its gate, which is how
        # fn/Salsa20Xor failed on 2026-11-19 after a refactor moved two
        # lines. A stale declaration is a claim about code that is no longer
        # there, so the unit fails instead of quietly verifying against a
        # weaker mutant set. (fn/ArchiveSetContains and eight other units
        # carried the same rot on the same day.)
        if unit.known_equivalent_mutants:
            all_sites = mutate_mod.generate_mutants(
                base_src,
                mutation_entry(unit.entry)
                if unit.candidate_loader == "cargo"
                else unit.entry,
                count=MUTATION_SITE_SCAN,
                base_dir=os.path.dirname(cand_path) or ".",
            )
            stale = [
                f"{eq.get('kind')}@{eq.get('line')}"
                for eq in unit.known_equivalent_mutants
                if eq.get("kind") is not None
                and eq.get("line") is not None
                and not any(_declares(eq, m) for m in all_sites)
            ]
            if stale:
                return entry(
                    "inconclusive",
                    coverage=cov.coverage,
                    notes=[
                        "declared equivalent mutants name sites that no longer "
                        "exist in the candidate: "
                        + ", ".join(stale)
                        + "; the code moved, update the unit's "
                        "knownEquivalentMutants lines"
                    ],
                )
        active = [
            m
            for m in mutants
            if not any(_declares(eq, m) for eq in unit.known_equivalent_mutants)
        ]
        for m in mutants:
            if m not in active:
                log(f"  skip mutant {m.kind}@{m.line} (declared equivalent)")

        # Mutation corpus: a subset is enough to decide kill/survive. The
        # first 48 are the pinned specials (factories put discriminating cases
        # at the front) plus the focused edges. When the corpus grew, the
        # grown tail is appended: a mutant can be distinguishable only on a
        # branch a grown case uncovered, which the head of the corpus never
        # sees. Oracle results come from the stage-3 cache - identical inputs
        # were already executed and stage 2 proved the oracle agrees with
        # itself. The results are gathered here, in the main thread, because
        # the emulator is not shared with the mutant workers and a cache miss
        # must not change that.
        if grown:
            mut_cases = (
                cases[: min(base_case_count, MUTATION_CORPUS_CASES)]
                + cases[base_case_count:]
            )
        else:
            mut_cases = cases[: min(len(cases), MUTATION_CORPUS_CASES)]
        _t_oracle = time.perf_counter()
        mutation_oracle: List[Tuple[Any, Any]] = []
        if unit.steps:
            # A steps unit's mutant is replayed step by step on one probe
            # process, so the state the mutant wrote is what its next call
            # reads - the same contract the base case has. The list holds
            # (step args, that step's oracle result) in order; the kill loop
            # below walks it in order against a single candidate process.
            replay = EmuOracle(unit.image, base=unit.base)
            if unit.state is not None:
                replay.set_state(unit.state.base, unit.state.data)
            replay.persist_state = True
            # Every case, not only the first: a steps unit's corpus is where
            # the discriminating inputs are, and one case is a whole chain of
            # calls that the flat corpus of a single-call unit spends dozens of
            # cases on. The third element marks a case's first step, where the
            # mutant's state region has to go back to the snapshot.
            for case in mut_cases:
                for step, args in enumerate(case):
                    o = replay.call(
                        unit.address_v2b,
                        args,
                        ret_kind=unit.ret_kind,
                        start_regs=unit.start_regs or {},
                    )
                    mutation_oracle.append((args, o, step == 0))
        else:
            for args in mut_cases:
                key = _args_key(args)
                o = cache.get(key)
                if o is None:  # defensive; every case ran in stage 3
                    o = oracle.call(unit.address_v2b, args, ret_kind=unit.ret_kind)
                    cache[key] = o
                mutation_oracle.append((args, o, False))
        log(f"  [mutation corpus] {time.perf_counter() - _t_oracle:.1f}s ({len(mut_cases)} cases)")

        # ---- Stage 5a: the batch build -----------------------------------
        # A cargo unit's mutants are each a full crate rebuild (measured:
        # 4.4 s with a warm target dir), and a unit has a dozen of them -
        # ~80% of the harness's wall time. The batch compiles the crate ONCE
        # with every mutant as a dispatch arm inside the function under test
        # (`batch.py`); the harness picks the arm with an environment
        # variable. The kill verdict is unchanged: the same corpus, the same
        # comparisons, the same gates.
        batch_exe: Optional[str] = None
        batch_work = ""
        arm_of: Dict[int, int] = {}      # position in `active` -> arm index
        broken: Dict[int, str] = {}       # position in `active` -> build error
        if unit.candidate_loader == "cargo" and len(active) > 1 and _batch_enabled():
            _t_batch = time.perf_counter()
            batch_exe, batch_work, arm_of, broken = _build_batch(
                unit, cref, cand_path, base_src, active
            )
            log(f"  [batch build] {time.perf_counter() - _t_batch:.1f}s")
            if batch_exe is not None:
                log(
                    f"  batch build: {len(arm_of)} mutant(s) in one crate build"
                    + (
                        f", {len(broken)} dropped by the compiler"
                        if broken
                        else ""
                    )
                )
                if not _batch_matches_baseline(
                    unit, cref, batch_exe, mutation_oracle
                ):
                    log(
                        "  batch build does not reproduce the baseline; falling "
                        "back to one build per mutant"
                    )
                    batch_exe = None
                    arm_of = {}
                    broken = {}
                    shutil.rmtree(batch_work, ignore_errors=True)
                    batch_work = ""

        # Each mutant is an independent probe subprocess and the oracle
        # results above are read-only, so build AND run each mutant in a
        # worker. `pool.map` keeps the report in mutant order; the verdicts
        # are per-mutant and do not depend on scheduling.
        def _probe(item) -> MutantOutcome:
            pos, m = item
            # A mutant the harness could not build, start or drive is
            # retried once with a fresh work dir and a doubled call timeout.
            # A second infrastructure failure is NOT a kill: the mutant was
            # never evaluated, so the unit cannot be verified on it.
            last_infra = ""
            for attempt in range(2):
                mdir = tempfile.mkdtemp(prefix="mutant_")
                mc = None
                try:
                    try:
                        if unit.candidate_loader in ("rust", "cargo"):
                            from candidate_rust import RustCandidate

                            if batch_exe is not None and pos in arm_of:
                                # The batch arm: the binary is already built.
                                from batch import ENV_NAME

                                mc = RustCandidate(
                                    entry=unit.entry,
                                    arg_kinds=[
                                        a.kind if a.kind != "ptr" else "ptr"
                                        for a in unit.args
                                    ],
                                    ret_kind=unit.ret_kind,
                                    work_dir=mdir,
                                    prebuilt_exe=batch_exe,
                                    env={ENV_NAME: str(arm_of[pos])},
                                    loader=unit.candidate_loader,
                                    state_base=unit.state.base
                                    if unit.state is not None
                                    else None,
                                    state_bytes=unit.state.data
                                    if unit.state is not None
                                    else None,
                                    call_timeout=mutation_call_timeout
                                    * (attempt + 1),
                                    crate_dir=cref.crate_dir,
                                    cargo_probe=cref.probe,
                                )
                                mc.set_persist(bool(unit.steps))
                            else:
                                mc = RustCandidate(
                                    entry=unit.entry,
                                    arg_kinds=[
                                        a.kind if a.kind != "ptr" else "ptr"
                                        for a in unit.args
                                    ],
                                    ret_kind=unit.ret_kind,
                                    work_dir=mdir,
                                    source_override=m.source,
                                    source_path=cand_path
                                    if unit.candidate_loader == "cargo"
                                    else None,
                                    loader=unit.candidate_loader,
                                    state_base=unit.state.base
                                    if unit.state is not None
                                    else None,
                                    state_bytes=unit.state.data
                                    if unit.state is not None
                                    else None,
                                    call_timeout=mutation_call_timeout
                                    * (attempt + 1),
                                    crate_dir=cref.crate_dir,
                                    cargo_probe=cref.probe,
                                )
                                mc.set_persist(bool(unit.steps))
                        else:
                            from candidate_c import CCandidate

                            mc = CCandidate(
                                c_file=unit.c_file,
                                entry=unit.entry,
                                arg_kinds=[
                                    a.kind if a.kind != "ptr" else "ptr"
                                    for a in unit.args
                                ],
                                ret_kind=unit.ret_kind,
                                work_dir=mdir,
                                source_override=m.source,
                            )
                    except RuntimeError as e:
                        msg = str(e)
                        # A source that does not compile is a killed mutant
                        # (the compiler is the detector). Anything else is an
                        # infrastructure failure and gets the retry.
                        compile_failed = (
                            unit.candidate_loader == "c"
                            or "build failed" in msg
                            or "rustc" in msg
                        )
                        if compile_failed:
                            return MutantOutcome(m, killed=True, build_error=msg)
                        last_infra = msg
                        continue
                    evaluated = True
                    for idx, (args, o, case_start) in enumerate(mutation_oracle):
                        if case_start:
                            reset = getattr(mc, "reset_state", None)
                            if reset is not None:
                                reset()
                        c = mc.call(args)
                        if not c.ok:
                            if getattr(c, "infra", False):
                                # A timeout is retried once with a doubled
                                # budget; a second timeout is a non-terminating
                                # mutant, which is a divergence (killed), not a
                                # harness failure. A closed pipe / missing
                                # artifact is a harness failure.
                                timed_out = "timed out" in (c.fault or "")
                                if timed_out and attempt == 1:
                                    return MutantOutcome(
                                        m,
                                        killed=True,
                                        detail=(
                                            f"case {idx}: no return within "
                                            f"{mutation_call_timeout * 2:g}s"
                                        ),
                                    )
                                last_infra = f"case {idx}: {c.fault}"
                                evaluated = False
                                break
                            # A fault is a divergence.
                            return MutantOutcome(
                                m,
                                killed=True,
                                detail=f"case {idx}: candidate fault: {c.fault}",
                            )
                        if unit.ret_kind != "void" and not _bits_equal(
                            o.ret_bits, c.ret_bits, unit.ret_kind
                        ):
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: return differs"
                            )
                        if _mem_channel(o.pointee_after, c.pointee_after, args) is not None:
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: pointee differs"
                            )
                        if (
                            _state_channel(
                                getattr(o, "state_after", None),
                                getattr(c, "state_after", None),
                                unit,
                                args,
                            )
                            is not None
                        ):
                            return MutantOutcome(
                                m, killed=True, detail=f"case {idx}: state differs"
                            )
                    if evaluated:
                        return MutantOutcome(m, killed=False)
                finally:
                    close = getattr(mc, "close", None)
                    if close is not None:
                        close()
                    shutil.rmtree(mdir, ignore_errors=True)
            return MutantOutcome(m, killed=False, infra=True, detail=last_infra)

        workers = max(1, min(mutant_workers, len(active)))
        log(
            f"  probing {len(active)} mutant(s) on {workers} worker(s), "
            f"mutation call timeout {mutation_call_timeout:g}s"
        )
        # A mutant the compiler rejected in the batch build is already a kill
        # (the compiler is the detector), exactly as in the one-build-per-
        # mutant path. It is counted in both numerator and denominator.
        pre_killed = {
            pos: MutantOutcome(
                active[pos], killed=True, build_error=msg, detail="does not compile"
            )
            for pos, msg in broken.items()
        }
        todo = [
            (pos, m)
            for pos, m in enumerate(active)
            if pos not in pre_killed
        ]
        def _probe_timed(item):
            # A mutant that hangs is the only slow one (it waits out the call
            # timeout), so the slow ones are the ones worth naming.
            _t0 = time.perf_counter()
            out = _probe(item)
            _dt = time.perf_counter() - _t0
            if _dt >= 1.0:
                log(
                    f"  (mutant {item[1].kind}@{item[1].line} took {_dt:.1f}s)"
                )
            return out

        if workers > 1:
            with ThreadPoolExecutor(max_workers=workers) as pool:
                probed = list(pool.map(_probe_timed, todo))
        else:
            probed = [_probe_timed(item) for item in todo]
        outcomes = []
        for pos, m in enumerate(active):
            outcomes.append(pre_killed.get(pos) or probed.pop(0))
        if batch_work:
            shutil.rmtree(batch_work, ignore_errors=True)
            batch_work = ""

        killed = 0
        scored = 0
        survivors: List[Any] = []
        # A mutant that could not be built/started/driven was never evaluated.
        # Counting it as killed would inflate the kill rate and could turn an
        # untested unit into a false "verified". Report it and stop.
        infra = [r for r in outcomes if r.infra]
        if infra:
            for r in infra:
                log(
                    f"  mutant {r.mutant.kind}@{r.mutant.line} not evaluated: "
                    f"{r.detail}"
                )
            return entry(
                "inconclusive",
                coverage=cov.coverage,
                notes=[
                    f"{len(infra)} mutant(s) could not be evaluated "
                    f"(infrastructure, not a kill): {infra[0].detail}"
                ],
            )
        for res in outcomes:
            m = res.mutant
            scored += 1
            if res.build_error:
                # A mutant that does not compile is a killed mutant: the
                # harness cannot run a broken build, and the original still
                # verified, so the detection-power claim stands for the
                # mutants that do build. Count it in BOTH numerator and
                # denominator so the kill rate stays in [0, 1].
                log(
                    f"  mutant {m.kind}@{m.line} does not compile "
                    f"(counted killed): {res.build_error}"
                )
                killed += 1
            elif res.killed:
                killed += 1
                log(f"  mutant {m.kind}@{m.line} killed: {res.detail}")
            else:
                survivors.append(m)
                log(f"  SURVIVOR: {m.kind}@{m.line}: {m.excerpt[:60]}")

        _stage_done("mutation")
        kill_rate = (killed / scored) if scored else 0.0
        log(f"  kill rate {kill_rate:.0%} ({killed}/{scored})")
        if survivors:
            log("  survivors must be fixed or declared observationally equivalent")
            for s in survivors:
                log(f"    {s.kind}@{s.line}: {s.excerpt[:70]}")
        if kill_rate < kill_gate:
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
        return entry(
            "verified",
            coverage=cov.coverage,
            mutation_kill_rate=kill_rate,
            mutants_killed=killed,
            mutants_total=scored,
        )
    finally:
        close = getattr(candidate, "close", None)
        if close is not None:
            close()
        if own_workdir:
            shutil.rmtree(work_dir, ignore_errors=True)


def _fmt_ret(bits: int, kind: str) -> str:
    try:
        if kind == "f32":
            return repr(bits_f32(bits))
        if kind == "f64":
            return repr(struct.unpack("<d", struct.pack("<Q", bits & (2**64 - 1)))[0])
        if kind == "vec4":
            return f"0x{bits:032x}"
        return str(bits)
    except Exception:
        return "?"


def run_and_record(unit: Unit, use_cache: bool = False, **kw) -> LedgerEntry:
    """Run the ladder for one unit and record the verdict.

    `use_cache=True` first asks `_fresh_cached_entry`: when the ledger holds a
    `verified` entry whose every bound digest still matches, that entry is
    replayed (with an explanatory note) instead of re-running the five stages.
    The ledger is not rewritten on a cache hit, so the entry keeps the
    timestamp of the run that actually proved the verdict.

    Units may be verified concurrently (the CLI runs `all`/`casc` through a
    thread pool); the load-then-record pair is serialised by `_LEDGER_LOCK`
    because it is a read-modify-write of ledger.json - without the lock, two
    units finishing together would each record over the other's verdict.
    """
    if use_cache:
        cached = _fresh_cached_entry(unit)
        if cached is not None:
            return cached
    e = run_unit(unit, **kw)
    with _LEDGER_LOCK:
        entries = load_ledger()
        record(entries, e)
    return e
